//! Monitor detection module.
//!
//! On Windows, enumerates connected monitors via Win32 APIs, retrieves
//! resolution, position, refresh rate, and physical dimensions (via EDID
//! parsing). Also provides DXGI output lookup for WaitForVBlank support.
//!
//! On other platforms, returns empty results — hardware monitor detection
//! is not yet implemented.

use crate::session::MonitorInfo;

// =============================================================================
// EDID parsing — pure data, works on all platforms
// =============================================================================

/// Parse EDID binary data to extract physical size in millimeters.
pub fn parse_edid_physical_size(edid: &[u8]) -> Option<(f64, f64, &'static str)> {
    if edid.len() < 128 {
        return None;
    }

    // Try Detailed Timing Descriptors (bytes 54..126, four 18-byte blocks)
    for i in 0..4 {
        let base = 54 + i * 18;
        if base + 18 > edid.len() {
            break;
        }

        let pixel_clock = u16::from_le_bytes([edid[base], edid[base + 1]]);
        if pixel_clock == 0 {
            continue;
        }

        let h_size_lo = edid[base + 12] as u32;
        let v_size_lo = edid[base + 13] as u32;
        let hv_size_hi = edid[base + 14] as u32;

        let h_mm = (h_size_lo | ((hv_size_hi >> 4) << 8)) as f64;
        let v_mm = (v_size_lo | ((hv_size_hi & 0x0F) << 8)) as f64;

        if h_mm > 0.0 && v_mm > 0.0 {
            return Some((h_mm, v_mm, "edid_detailed_timing"));
        }
    }

    // Fallback: basic EDID bytes 21-22 (horizontal/vertical size in cm)
    let h_cm = edid[21] as f64;
    let v_cm = edid[22] as f64;
    if h_cm > 0.0 && v_cm > 0.0 {
        return Some((h_cm * 10.0, v_cm * 10.0, "edid_basic"));
    }

    None
}

// =============================================================================
// Windows implementation
// =============================================================================

#[cfg(windows)]
mod platform {
    use super::*;
    use std::mem;
    use windows::core::PCWSTR;
    use windows::Win32::Devices::DeviceAndDriverInstallation::{
        SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
        SetupDiOpenDevRegKey, DIGCF_PRESENT, DIREG_DEV, SP_DEVINFO_DATA,
    };
    use windows::Win32::Foundation::{BOOL, LPARAM, RECT};
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, IDXGIOutput};
    use windows::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, EnumDisplaySettingsW, GetMonitorInfoW, DEVMODEW,
        ENUM_CURRENT_SETTINGS, HDC, HMONITOR, MONITORINFOEXW,
    };
    use windows::Win32::System::Registry::{
        RegCloseKey, RegQueryValueExW, KEY_READ, REG_BINARY,
    };

    /// GUID_DEVCLASS_MONITOR: {4d36e96e-e325-11ce-bfc1-08002be10318}
    const GUID_DEVCLASS_MONITOR: windows::core::GUID = windows::core::GUID::from_u128(
        0x4d36e96e_e325_11ce_bfc1_08002be10318,
    );

    /// Collect all HMONITOR handles via EnumDisplayMonitors.
    fn enumerate_hmonitors() -> Vec<HMONITOR> {
        unsafe extern "system" fn enum_callback(
            hmonitor: HMONITOR,
            _hdc: HDC,
            _rect: *mut RECT,
            lparam: LPARAM,
        ) -> BOOL {
            unsafe {
                let monitors = &mut *(lparam.0 as *mut Vec<HMONITOR>);
                monitors.push(hmonitor);
            }
            BOOL(1)
        }

        let mut monitors: Vec<HMONITOR> = Vec::new();
        unsafe {
            let _ = EnumDisplayMonitors(
                None,
                None,
                Some(enum_callback),
                LPARAM(&mut monitors as *mut Vec<HMONITOR> as isize),
            );
        }
        monitors
    }

    /// Get MONITORINFOEXW for a given HMONITOR.
    fn get_monitor_info_ex(hmonitor: HMONITOR) -> Option<MONITORINFOEXW> {
        let mut info: MONITORINFOEXW = unsafe { mem::zeroed() };
        info.monitorInfo.cbSize = mem::size_of::<MONITORINFOEXW>() as u32;
        let ok = unsafe { GetMonitorInfoW(hmonitor, &mut info as *mut _ as *mut _) };
        if ok.as_bool() {
            Some(info)
        } else {
            None
        }
    }

    /// Extract a friendly name string from a WCHAR device name array.
    fn wchar_to_string(wchars: &[u16]) -> String {
        let len = wchars.iter().position(|&c| c == 0).unwrap_or(wchars.len());
        String::from_utf16_lossy(&wchars[..len])
    }

    /// Get resolution, position, and refresh rate from display settings.
    fn get_display_settings(device_name: &[u16]) -> Option<(u32, u32, u32, i32, i32)> {
        let mut devmode: DEVMODEW = unsafe { mem::zeroed() };
        devmode.dmSize = mem::size_of::<DEVMODEW>() as u16;

        let ok = unsafe {
            EnumDisplaySettingsW(
                PCWSTR(device_name.as_ptr()),
                ENUM_CURRENT_SETTINGS,
                &mut devmode,
            )
        };

        if ok.as_bool() {
            let width = devmode.dmPelsWidth;
            let height = devmode.dmPelsHeight;
            let refresh = devmode.dmDisplayFrequency;
            let x = unsafe { devmode.Anonymous1.Anonymous2.dmPosition.x };
            let y = unsafe { devmode.Anonymous1.Anonymous2.dmPosition.y };
            Some((width, height, refresh, x, y))
        } else {
            None
        }
    }

    /// Read EDID data from the registry for all monitor devices using SetupAPI.
    fn read_all_edid_entries() -> Vec<(f64, f64, String)> {
        let mut results = Vec::new();

        unsafe {
            let dev_info = SetupDiGetClassDevsW(
                Some(&GUID_DEVCLASS_MONITOR),
                PCWSTR::null(),
                None,
                DIGCF_PRESENT,
            );

            let dev_info = match dev_info {
                Ok(h) => h,
                Err(_) => return results,
            };

            let mut dev_index: u32 = 0;
            loop {
                let mut dev_info_data: SP_DEVINFO_DATA = mem::zeroed();
                dev_info_data.cbSize = mem::size_of::<SP_DEVINFO_DATA>() as u32;

                if SetupDiEnumDeviceInfo(dev_info, dev_index, &mut dev_info_data).is_err() {
                    break;
                }

                // Open the device's hardware registry key
                let hkey = SetupDiOpenDevRegKey(
                    dev_info,
                    &dev_info_data,
                    1, // DICS_FLAG_GLOBAL
                    0,
                    DIREG_DEV,
                    KEY_READ.0,
                );

                if let Ok(hkey) = hkey {
                    let value_name: Vec<u16> = "EDID\0".encode_utf16().collect();
                    let mut data_type = windows::Win32::System::Registry::REG_VALUE_TYPE(0);
                    let mut data_size: u32 = 0;

                    // First call to get size
                    let _ = RegQueryValueExW(
                        hkey,
                        PCWSTR(value_name.as_ptr()),
                        None,
                        Some(&mut data_type),
                        None,
                        Some(&mut data_size),
                    );

                    if data_type == REG_BINARY && data_size >= 128 {
                        let mut edid_buf = vec![0u8; data_size as usize];
                        let status = RegQueryValueExW(
                            hkey,
                            PCWSTR(value_name.as_ptr()),
                            None,
                            Some(&mut data_type),
                            Some(edid_buf.as_mut_ptr()),
                            Some(&mut data_size),
                        );

                        if status.is_ok() {
                            if let Some((w_mm, h_mm, source)) =
                                parse_edid_physical_size(&edid_buf)
                            {
                                results.push((w_mm, h_mm, source.to_string()));
                            }
                        }
                    }

                    let _ = RegCloseKey(hkey);
                }

                dev_index += 1;
            }

            let _ = SetupDiDestroyDeviceInfoList(dev_info);
        }

        results
    }

    /// Enumerate all connected monitors and return a Vec<MonitorInfo>.
    pub fn detect_monitors() -> Vec<MonitorInfo> {
        let hmonitors = enumerate_hmonitors();
        let edid_entries = read_all_edid_entries();

        let mut monitors = Vec::new();

        for (idx, &hmonitor) in hmonitors.iter().enumerate() {
            let info_ex = match get_monitor_info_ex(hmonitor) {
                Some(info) => info,
                None => continue,
            };

            let device_name = wchar_to_string(&info_ex.szDevice);

            let (width_px, height_px, refresh_hz, pos_x, pos_y) =
                match get_display_settings(&info_ex.szDevice) {
                    Some(vals) => vals,
                    None => {
                        let rc = info_ex.monitorInfo.rcMonitor;
                        let w = (rc.right - rc.left).max(0) as u32;
                        let h = (rc.bottom - rc.top).max(0) as u32;
                        (w, h, 60, rc.left, rc.top)
                    }
                };

            let (width_cm, height_cm, physical_source) = if idx < edid_entries.len() {
                let (w_mm, h_mm, ref source) = edid_entries[idx];
                (w_mm / 10.0, h_mm / 10.0, source.clone())
            } else {
                (0.0, 0.0, "unknown".to_string())
            };

            monitors.push(MonitorInfo {
                index: idx,
                name: device_name,
                width_px,
                height_px,
                width_cm,
                height_cm,
                refresh_hz,
                position: (pos_x, pos_y),
                physical_source,
            });
        }

        monitors
    }

    /// Find the DXGI output (IDXGIOutput) for a given monitor index.
    pub fn find_dxgi_output(monitor_index: usize) -> Result<IDXGIOutput, String> {
        let monitors = enumerate_hmonitors();
        if monitor_index >= monitors.len() {
            return Err(format!(
                "Monitor index {} out of range (have {})",
                monitor_index,
                monitors.len()
            ));
        }

        let target_info = get_monitor_info_ex(monitors[monitor_index])
            .ok_or_else(|| format!("Failed to get info for monitor {}", monitor_index))?;
        let target_rect = target_info.monitorInfo.rcMonitor;

        unsafe {
            let factory: IDXGIFactory1 = CreateDXGIFactory1()
                .map_err(|e| format!("CreateDXGIFactory1 failed: {}", e))?;

            let mut adapter_idx: u32 = 0;
            loop {
                let adapter = match factory.EnumAdapters1(adapter_idx) {
                    Ok(a) => a,
                    Err(_) => break,
                };

                let mut output_idx: u32 = 0;
                loop {
                    let output = match adapter.EnumOutputs(output_idx) {
                        Ok(o) => o,
                        Err(_) => break,
                    };

                    if let Ok(desc) = output.GetDesc() {
                        let rc = desc.DesktopCoordinates;
                        if rc.left == target_rect.left
                            && rc.top == target_rect.top
                            && rc.right == target_rect.right
                            && rc.bottom == target_rect.bottom
                        {
                            return Ok(output);
                        }
                    }

                    output_idx += 1;
                }
                adapter_idx += 1;
            }
        }

        Err(format!(
            "No DXGI output found for monitor index {}",
            monitor_index
        ))
    }

    /// Get the desktop position (x, y) of a monitor by index.
    pub fn get_monitor_position(monitor_index: usize) -> Result<(i32, i32), String> {
        let hmonitors = enumerate_hmonitors();
        if monitor_index >= hmonitors.len() {
            return Err(format!(
                "Monitor index {} out of range (have {})",
                monitor_index,
                hmonitors.len()
            ));
        }

        let info = get_monitor_info_ex(hmonitors[monitor_index])
            .ok_or_else(|| format!("Failed to get info for monitor {}", monitor_index))?;

        let rc = info.monitorInfo.rcMonitor;
        Ok((rc.left, rc.top))
    }
}

// =============================================================================
// Non-Windows stubs
// =============================================================================

#[cfg(not(windows))]
mod platform {
    use super::*;

    /// Returns an empty list — hardware monitor detection requires Windows APIs.
    pub fn detect_monitors() -> Vec<MonitorInfo> {
        eprintln!("[monitor] Hardware monitor detection is not available on this platform");
        Vec::new()
    }

    /// Not available on this platform.
    pub fn get_monitor_position(_monitor_index: usize) -> Result<(i32, i32), String> {
        Err("Monitor position detection requires Windows".into())
    }
}

// =============================================================================
// Re-exports from platform module
// =============================================================================

pub use platform::detect_monitors;
pub use platform::get_monitor_position;

#[cfg(windows)]
pub use platform::find_dxgi_output;

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_edid_detailed_timing() {
        let mut edid = vec![0u8; 128];
        edid[54] = 0x01;
        edid[55] = 0x00;
        edid[66] = 0xD0;
        edid[67] = 0x22;
        edid[68] = 0x11;

        let result = parse_edid_physical_size(&edid);
        assert!(result.is_some());
        let (w, h, source) = result.unwrap();
        assert_eq!(w, 464.0);
        assert_eq!(h, 290.0);
        assert_eq!(source, "edid_detailed_timing");
    }

    #[test]
    fn test_parse_edid_basic_fallback() {
        let mut edid = vec![0u8; 128];
        edid[21] = 53;
        edid[22] = 30;

        let result = parse_edid_physical_size(&edid);
        assert!(result.is_some());
        let (w, h, source) = result.unwrap();
        assert_eq!(w, 530.0);
        assert_eq!(h, 300.0);
        assert_eq!(source, "edid_basic");
    }

    #[test]
    fn test_parse_edid_too_short() {
        let edid = vec![0u8; 64];
        assert!(parse_edid_physical_size(&edid).is_none());
    }
}
