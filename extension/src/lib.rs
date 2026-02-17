//! OpenISI Shared Memory GDExtension
//!
//! Provides:
//! - SharedMemoryReader: Background thread for reading camera frames from shared memory
//! - MonitorInfo: Cross-platform physical monitor dimension detection
//! - VsyncTimestampProvider: Hardware vsync timestamps via Vulkan VK_GOOGLE_display_timing

use godot::prelude::*;
use shared_memory::ShmemConf;
use std::sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU32, Ordering}};
use std::thread::{self, JoinHandle};

pub mod vsync_provider;
pub use vsync_provider::VsyncTimestampProvider;

struct OpenIsiExtension;

#[gdextension]
unsafe impl ExtensionLibrary for OpenIsiExtension {}

/// Control region header in shared memory (must match Python daemon protocol.py)
/// Total size: 64 bytes (padded for alignment)
/// Using packed to prevent automatic padding that would misalign with Python struct
#[repr(C, packed)]
struct ControlRegion {
    write_index: u32,        // Offset 0
    read_index: u32,         // Offset 4
    frame_width: u32,        // Offset 8
    frame_height: u32,       // Offset 12
    frame_count: u32,        // Offset 16
    num_buffers: u32,        // Offset 20
    status: u8,              // Offset 24
    command: u8,             // Offset 25
    latest_timestamp_us: u64, // Offset 26 - Hardware timestamp of most recent frame
    daemon_pid: u32,         // Offset 34 - Daemon PID for cleanup
    _reserved: [u8; 26],     // Offset 38-63 (padding to 64 bytes)
}

const CONTROL_SIZE: usize = 64;

/// Thread-safe frame buffer
struct FrameBuffer {
    data: Vec<u8>,
    frame_count: u32,
    width: u32,
    height: u32,
    timestamp_us: u64,  // Hardware timestamp of this frame
    daemon_pid: u32,    // Daemon PID for cleanup
}

impl FrameBuffer {
    fn new() -> Self {
        Self {
            data: Vec::new(),
            frame_count: 0,
            width: 0,
            height: 0,
            timestamp_us: 0,
            daemon_pid: 0,
        }
    }
}

/// Shared memory reader for OpenISI frames with background thread
#[derive(GodotClass)]
#[class(base=RefCounted)]
pub struct SharedMemoryReader {
    // Thread-safe shared state
    frame_buffer: Arc<Mutex<FrameBuffer>>,
    running: Arc<AtomicBool>,
    last_read_count: AtomicU32,

    // Thread handle (Option so we can take it on close)
    worker_thread: Option<JoinHandle<()>>,

    // Cached dimensions
    frame_width: u32,
    frame_height: u32,

    // Shared memory name (for sending commands)
    shm_name: String,
}

#[godot_api]
impl IRefCounted for SharedMemoryReader {
    fn init(_base: Base<RefCounted>) -> Self {
        godot_print!("SharedMemoryReader created");
        Self {
            frame_buffer: Arc::new(Mutex::new(FrameBuffer::new())),
            running: Arc::new(AtomicBool::new(false)),
            last_read_count: AtomicU32::new(0),
            worker_thread: None,
            frame_width: 0,  // Set from shared memory in open()
            frame_height: 0, // Set from shared memory in open()
            shm_name: String::new(),
        }
    }
}

#[godot_api]
impl SharedMemoryReader {
    #[func]
    fn open(&mut self, name: GString) -> bool {
        let name_str = name.to_string();
        let shm_name = if name_str.starts_with('/') {
            name_str
        } else {
            format!("/{}", name_str)
        };

        // Store name for later use (e.g., send_stop_command)
        self.shm_name = shm_name.clone();

        godot_print!("Opening shared memory: {}", shm_name);

        // Open shared memory to get initial dimensions
        let shm = match ShmemConf::new().os_id(&shm_name).open() {
            Ok(s) => s,
            Err(e) => {
                godot_print!("Failed to open shared memory: {}", e);
                return false;
            }
        };

        godot_print!("Shared memory opened successfully, size: {} bytes", shm.len());

        // Read initial frame dimensions
        unsafe {
            let ptr = shm.as_ptr();
            let control = &*(ptr as *const ControlRegion);
            self.frame_width = control.frame_width;
            self.frame_height = control.frame_height;
            godot_print!("Frame dimensions: {}x{}", self.frame_width, self.frame_height);
        }

        // Set up shared state for thread
        let frame_buffer = Arc::clone(&self.frame_buffer);
        let running = Arc::clone(&self.running);
        let width = self.frame_width;
        let height = self.frame_height;

        // Start running
        self.running.store(true, Ordering::SeqCst);

        // Spawn worker thread
        let shm_name_clone = shm_name.clone();
        let handle = thread::spawn(move || {
            // Open shared memory in this thread
            let shm = match ShmemConf::new().os_id(&shm_name_clone).open() {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Worker thread: Failed to open shared memory: {}", e);
                    return;
                }
            };

            let frame_size = (width * height) as usize;
            let mut last_frame_count: u32 = 0;
            let mut converted_buffer = vec![0u8; frame_size];

            while running.load(Ordering::SeqCst) {
                unsafe {
                    let ptr = shm.as_ptr();
                    let control = &*(ptr as *const ControlRegion);

                    // Check for new frame
                    let current_count = control.frame_count;
                    if current_count != last_frame_count && current_count > 0 {
                        last_frame_count = current_count;

                        // Calculate frame location - read the most recently written buffer
                        // write_index points to the NEXT buffer to write, so we read (write_index - 1) % num_buffers
                        let frame_size_bytes = frame_size * 2;
                        let num_buffers = control.num_buffers as usize;
                        let latest_buffer = if control.write_index == 0 {
                            num_buffers - 1
                        } else {
                            (control.write_index - 1) as usize % num_buffers
                        };
                        let frame_offset = CONTROL_SIZE + (latest_buffer * frame_size_bytes);

                        // Read and convert frame
                        let frame_ptr = ptr.add(frame_offset) as *const u16;
                        let frame_slice = std::slice::from_raw_parts(frame_ptr, frame_size);

                        for (i, &val) in frame_slice.iter().enumerate() {
                            converted_buffer[i] = (val / 257) as u8;
                        }

                        // Read timestamp and daemon PID from control region
                        let timestamp_us = control.latest_timestamp_us;
                        let daemon_pid = control.daemon_pid;

                        // Update shared buffer
                        if let Ok(mut buffer) = frame_buffer.lock() {
                            buffer.data.clear();
                            buffer.data.extend_from_slice(&converted_buffer);
                            buffer.frame_count = current_count;
                            buffer.width = width;
                            buffer.height = height;
                            buffer.timestamp_us = timestamp_us;
                            buffer.daemon_pid = daemon_pid;
                        }
                    }
                }

                // Small sleep to avoid busy-waiting - use 100µs for low latency
                thread::sleep(std::time::Duration::from_micros(100));
            }
        });

        self.worker_thread = Some(handle);
        true
    }

    #[func]
    fn close(&mut self) {
        godot_print!("Closing shared memory");

        // Signal thread to stop
        self.running.store(false, Ordering::SeqCst);

        // Wait for thread to finish
        if let Some(handle) = self.worker_thread.take() {
            let _ = handle.join();
        }

        // Clear buffer
        if let Ok(mut buffer) = self.frame_buffer.lock() {
            buffer.data.clear();
            buffer.frame_count = 0;
        }

        godot_print!("Shared memory closed");
    }

    #[func]
    fn is_connected(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    #[func]
    fn get_latest_frame_u8(&mut self) -> PackedByteArray {
        let buffer = match self.frame_buffer.lock() {
            Ok(b) => b,
            Err(_) => return PackedByteArray::new(),
        };

        // Check if there's a new frame
        let current_count = buffer.frame_count;
        let last_count = self.last_read_count.load(Ordering::SeqCst);

        if current_count == last_count || buffer.data.is_empty() {
            return PackedByteArray::new();
        }

        self.last_read_count.store(current_count, Ordering::SeqCst);

        // Copy data to PackedByteArray
        let mut result = PackedByteArray::new();
        result.resize(buffer.data.len());
        result.as_mut_slice().copy_from_slice(&buffer.data);
        result
    }

    #[func]
    fn get_latest_frame(&mut self) -> PackedByteArray {
        self.get_latest_frame_u8()
    }

    #[func]
    fn get_frame_count(&self) -> u32 {
        if let Ok(buffer) = self.frame_buffer.lock() {
            buffer.frame_count
        } else {
            0
        }
    }

    #[func]
    fn get_status(&self) -> i32 {
        if self.running.load(Ordering::SeqCst) { 1 } else { 0 }
    }

    #[func]
    fn get_frame_width(&self) -> u32 {
        assert!(self.frame_width > 0, "SharedMemoryReader: not opened - call open() first");
        self.frame_width
    }

    #[func]
    fn get_frame_height(&self) -> u32 {
        assert!(self.frame_height > 0, "SharedMemoryReader: not opened - call open() first");
        self.frame_height
    }

    /// Get the hardware timestamp of the most recently read frame in microseconds.
    /// Returns 0 if no frame has been read or timestamp unavailable.
    #[func]
    fn get_latest_timestamp_us(&self) -> i64 {
        if let Ok(buffer) = self.frame_buffer.lock() {
            buffer.timestamp_us as i64
        } else {
            0
        }
    }

    /// Get the daemon's process ID from shared memory.
    /// Returns 0 if not connected or PID unavailable.
    #[func]
    fn get_daemon_pid(&self) -> i32 {
        if let Ok(buffer) = self.frame_buffer.lock() {
            buffer.daemon_pid as i32
        } else {
            0
        }
    }

    /// Send STOP command to the daemon via shared memory.
    /// The daemon polls for this command and will shut down gracefully.
    #[func]
    fn send_stop_command(&self) {
        if self.shm_name.is_empty() {
            godot_print!("send_stop_command: No shared memory opened");
            return;
        }

        // Open shared memory to write the command
        let shm = match ShmemConf::new().os_id(&self.shm_name).open() {
            Ok(s) => s,
            Err(e) => {
                godot_print!("send_stop_command: Failed to open shared memory: {}", e);
                return;
            }
        };

        // Write STOP command (value 2) to offset 25 (command field in ControlRegion)
        const COMMAND_OFFSET: usize = 25;
        const COMMAND_STOP: u8 = 2;

        unsafe {
            let ptr = shm.as_ptr() as *mut u8;
            std::ptr::write_volatile(ptr.add(COMMAND_OFFSET), COMMAND_STOP);
        }

        godot_print!("send_stop_command: STOP command sent to daemon");
    }
}

impl Drop for SharedMemoryReader {
    fn drop(&mut self) {
        // Ensure thread is stopped on drop
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.worker_thread.take() {
            let _ = handle.join();
        }
    }
}


// =============================================================================
// MonitorInfo - Cross-platform physical monitor dimension detection
// =============================================================================

/// Gets physical monitor dimensions using platform-specific APIs.
/// Returns (width_cm, height_cm) or (0.0, 0.0) if unavailable.
#[derive(GodotClass)]
#[class(base=RefCounted)]
pub struct MonitorInfo;

#[godot_api]
impl IRefCounted for MonitorInfo {
    fn init(_base: Base<RefCounted>) -> Self {
        Self
    }
}

#[godot_api]
impl MonitorInfo {
    /// Get the number of connected displays using native APIs.
    /// This bypasses Godot's DisplayServer which has issues on macOS.
    #[func]
    fn get_display_count() -> i32 {
        get_native_display_count() as i32
    }

    /// Get detailed info about a display using native APIs.
    /// Returns a Dictionary with: index, width, height, refresh_rate, position_x, position_y,
    /// width_mm, height_mm, is_primary, display_id
    #[func]
    fn get_display_info(display_index: i32) -> Dictionary {
        get_native_display_info(display_index as u32)
    }

    /// Get physical dimensions of a monitor in centimeters.
    /// Returns a Vector2 with (width_cm, height_cm).
    /// Returns (0, 0) if physical dimensions cannot be determined.
    #[func]
    fn get_physical_size_cm(monitor_index: i32) -> Vector2 {
        let (width_mm, height_mm) = get_monitor_physical_size_mm(monitor_index as u32);
        Vector2::new(width_mm as f32 / 10.0, height_mm as f32 / 10.0)
    }

    /// Get physical dimensions of a monitor in millimeters.
    /// Returns a Vector2i with (width_mm, height_mm).
    /// Returns (0, 0) if physical dimensions cannot be determined.
    #[func]
    fn get_physical_size_mm(monitor_index: i32) -> Vector2i {
        let (width_mm, height_mm) = get_monitor_physical_size_mm(monitor_index as u32);
        Vector2i::new(width_mm as i32, height_mm as i32)
    }

    /// Check if physical monitor size detection is available on this platform.
    #[func]
    fn is_available() -> bool {
        cfg!(any(target_os = "macos", target_os = "windows", target_os = "linux"))
    }

    /// Get the platform name for debugging.
    #[func]
    fn get_platform() -> GString {
        #[cfg(target_os = "macos")]
        return GString::from("macos");
        #[cfg(target_os = "windows")]
        return GString::from("windows");
        #[cfg(target_os = "linux")]
        return GString::from("linux");
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        return GString::from("unknown");
    }
}

/// Platform-specific implementation to get monitor physical size in millimeters.
fn get_monitor_physical_size_mm(monitor_index: u32) -> (u32, u32) {
    #[cfg(target_os = "macos")]
    {
        get_monitor_size_macos(monitor_index)
    }
    #[cfg(target_os = "windows")]
    {
        get_monitor_size_windows(monitor_index)
    }
    #[cfg(target_os = "linux")]
    {
        get_monitor_size_linux(monitor_index)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = monitor_index;
        (0, 0)
    }
}

// =============================================================================
// Native Display Enumeration - Bypasses Godot's DisplayServer
// =============================================================================

/// Get number of connected displays using native APIs
fn get_native_display_count() -> u32 {
    #[cfg(target_os = "macos")]
    {
        get_display_count_macos()
    }
    #[cfg(target_os = "windows")]
    {
        get_display_count_windows()
    }
    #[cfg(target_os = "linux")]
    {
        get_display_count_linux()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        0
    }
}

/// Get detailed display info using native APIs
fn get_native_display_info(display_index: u32) -> Dictionary {
    #[cfg(target_os = "macos")]
    {
        get_display_info_macos(display_index)
    }
    #[cfg(target_os = "windows")]
    {
        get_display_info_windows(display_index)
    }
    #[cfg(target_os = "linux")]
    {
        get_display_info_linux(display_index)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = display_index;
        Dictionary::new()
    }
}

// =============================================================================
// macOS Implementation - Uses Core Graphics
// =============================================================================

#[cfg(target_os = "macos")]
fn get_display_count_macos() -> u32 {
    use core_graphics::display::CGDirectDisplayID;

    let max_displays: u32 = 16;
    let mut display_ids: Vec<CGDirectDisplayID> = vec![0; max_displays as usize];
    let mut display_count: u32 = 0;

    unsafe {
        let result = core_graphics::display::CGGetActiveDisplayList(
            max_displays,
            display_ids.as_mut_ptr(),
            &mut display_count,
        );
        if result != 0 {
            return 0;
        }
    }

    display_count
}

#[cfg(target_os = "macos")]
fn get_display_info_macos(display_index: u32) -> Dictionary {
    use core_graphics::display::{CGDisplay, CGDirectDisplayID};

    let mut result = Dictionary::new();

    // Get all active displays
    let max_displays: u32 = 16;
    let mut display_ids: Vec<CGDirectDisplayID> = vec![0; max_displays as usize];
    let mut display_count: u32 = 0;

    unsafe {
        let err = core_graphics::display::CGGetActiveDisplayList(
            max_displays,
            display_ids.as_mut_ptr(),
            &mut display_count,
        );
        if err != 0 {
            return result;
        }
    }

    if display_index >= display_count {
        return result;
    }

    let display_id = display_ids[display_index as usize];
    let display = CGDisplay::new(display_id);

    // Get display mode - required for resolution and refresh rate
    let mode = match display.display_mode() {
        Some(m) => m,
        None => return result,  // Can't get display info without mode
    };

    // Get display properties
    let bounds = display.bounds();
    let physical_size = display.screen_size();
    let is_main = display_id == CGDisplay::main().id;

    result.set("index", display_index as i64);
    result.set("display_id", display_id as i64);
    result.set("width", mode.width() as i64);
    result.set("height", mode.height() as i64);
    result.set("refresh_rate", mode.refresh_rate());
    result.set("position_x", bounds.origin.x as i64);
    result.set("position_y", bounds.origin.y as i64);
    result.set("width_mm", physical_size.width as i64);
    result.set("height_mm", physical_size.height as i64);
    result.set("is_primary", is_main);

    result
}

#[cfg(target_os = "macos")]
fn get_monitor_size_macos(monitor_index: u32) -> (u32, u32) {
    use core_graphics::display::{CGDisplay, CGDirectDisplayID};

    // Get all active displays
    let max_displays: u32 = 16;
    let mut display_ids: Vec<CGDirectDisplayID> = vec![0; max_displays as usize];
    let mut display_count: u32 = 0;

    unsafe {
        let result = core_graphics::display::CGGetActiveDisplayList(
            max_displays,
            display_ids.as_mut_ptr(),
            &mut display_count,
        );
        if result != 0 {
            return (0, 0);
        }
    }

    if monitor_index >= display_count {
        return (0, 0);
    }

    let display_id = display_ids[monitor_index as usize];
    let display = CGDisplay::new(display_id);

    // CGDisplayScreenSize returns size in millimeters
    let size = display.screen_size();
    (size.width as u32, size.height as u32)
}

// =============================================================================
// Windows Implementation - Uses EnumDisplayMonitors and GetMonitorInfo
// =============================================================================

#[cfg(target_os = "windows")]
fn get_display_count_windows() -> u32 {
    use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};
    use windows::Win32::Foundation::{BOOL, LPARAM, RECT};
    use std::sync::atomic::{AtomicU32, Ordering};

    static MONITOR_COUNT: AtomicU32 = AtomicU32::new(0);
    MONITOR_COUNT.store(0, Ordering::SeqCst);

    unsafe extern "system" fn enum_callback(
        _monitor: HMONITOR,
        _hdc: HDC,
        _rect: *mut RECT,
        _lparam: LPARAM,
    ) -> BOOL {
        MONITOR_COUNT.fetch_add(1, Ordering::SeqCst);
        BOOL::from(true)
    }

    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(enum_callback), LPARAM(0));
    }

    MONITOR_COUNT.load(Ordering::SeqCst)
}

#[cfg(target_os = "windows")]
fn get_display_info_windows(display_index: u32) -> Dictionary {
    use windows::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, EnumDisplaySettingsW, GetMonitorInfoW,
        HDC, HMONITOR, MONITORINFOEXW, ENUM_CURRENT_SETTINGS, DEVMODEW,
    };
    use windows::Win32::Foundation::{BOOL, LPARAM, RECT};
    use std::sync::atomic::{AtomicUsize, Ordering};

    let mut result = Dictionary::new();

    // Collect monitor handles
    static MONITOR_COUNT: AtomicUsize = AtomicUsize::new(0);
    static mut MONITOR_HANDLES: [isize; 16] = [0; 16];

    MONITOR_COUNT.store(0, Ordering::SeqCst);

    unsafe extern "system" fn enum_callback(
        monitor: HMONITOR,
        _hdc: HDC,
        _rect: *mut RECT,
        _lparam: LPARAM,
    ) -> BOOL {
        let count = MONITOR_COUNT.load(Ordering::SeqCst);
        if count < 16 {
            MONITOR_HANDLES[count] = monitor.0 as isize;
            MONITOR_COUNT.store(count + 1, Ordering::SeqCst);
        }
        BOOL::from(true)
    }

    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(enum_callback), LPARAM(0));
    }

    let count = MONITOR_COUNT.load(Ordering::SeqCst);
    if display_index as usize >= count {
        return result;
    }

    unsafe {
        let handle = HMONITOR(MONITOR_HANDLES[display_index as usize] as *mut _);

        // Get monitor info
        let mut info: MONITORINFOEXW = std::mem::zeroed();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;

        if GetMonitorInfoW(handle, &mut info as *mut _ as *mut _).as_bool() {
            let rect = info.monitorInfo.rcMonitor;
            let width = (rect.right - rect.left) as i64;
            let height = (rect.bottom - rect.top) as i64;

            result.set("index", display_index as i64);
            result.set("width", width);
            result.set("height", height);
            result.set("position_x", rect.left as i64);
            result.set("position_y", rect.top as i64);

            // Check if primary
            let is_primary = (info.monitorInfo.dwFlags & 1) != 0; // MONITORINFOF_PRIMARY = 1
            result.set("is_primary", is_primary);

            // Get refresh rate from display settings
            let mut devmode: DEVMODEW = std::mem::zeroed();
            devmode.dmSize = std::mem::size_of::<DEVMODEW>() as u16;

            if EnumDisplaySettingsW(
                windows::core::PCWSTR(info.szDevice.as_ptr()),
                ENUM_CURRENT_SETTINGS,
                &mut devmode,
            ).as_bool() {
                result.set("refresh_rate", devmode.dmDisplayFrequency as f64);
            }

            // Physical size - would need EDID parsing for accurate values
            // For now, return 0 to indicate unavailable
            result.set("width_mm", 0i64);
            result.set("height_mm", 0i64);
        }
    }

    result
}

#[cfg(target_os = "windows")]
fn get_monitor_size_windows(monitor_index: u32) -> (u32, u32) {
    use windows::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, GetDC, GetDeviceCaps, ReleaseDC,
        HORZSIZE, VERTSIZE, HDC, HMONITOR,
    };
    use windows::Win32::Foundation::{BOOL, LPARAM, RECT};
    use std::sync::atomic::{AtomicUsize, Ordering};

    // Collect monitor handles
    static MONITOR_COUNT: AtomicUsize = AtomicUsize::new(0);
    static mut MONITOR_HANDLES: [isize; 16] = [0; 16];

    MONITOR_COUNT.store(0, Ordering::SeqCst);

    unsafe extern "system" fn enum_callback(
        monitor: HMONITOR,
        _hdc: HDC,
        _rect: *mut RECT,
        _lparam: LPARAM,
    ) -> BOOL {
        let count = MONITOR_COUNT.load(Ordering::SeqCst);
        if count < 16 {
            MONITOR_HANDLES[count] = monitor.0 as isize;
            MONITOR_COUNT.store(count + 1, Ordering::SeqCst);
        }
        BOOL::from(true)
    }

    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(enum_callback), LPARAM(0));
    }

    let count = MONITOR_COUNT.load(Ordering::SeqCst);
    if monitor_index as usize >= count {
        return (0, 0);
    }

    // Get DC for the desktop and query physical size
    // Note: GetDeviceCaps returns size in mm for the primary display context
    unsafe {
        let hdc = GetDC(None);
        if hdc.is_invalid() {
            return (0, 0);
        }

        let width_mm = GetDeviceCaps(hdc, HORZSIZE) as u32;
        let height_mm = GetDeviceCaps(hdc, VERTSIZE) as u32;

        let _ = ReleaseDC(None, hdc);

        // GetDeviceCaps with desktop DC only returns primary monitor size
        if monitor_index == 0 {
            (width_mm, height_mm)
        } else {
            (0, 0)
        }
    }
}

// =============================================================================
// Linux Implementation - Reads from /sys/class/drm/
// =============================================================================

#[cfg(target_os = "linux")]
fn get_connected_displays_linux() -> Vec<std::path::PathBuf> {
    use std::fs;
    use std::path::Path;

    let drm_path = Path::new("/sys/class/drm");
    let mut connected: Vec<std::path::PathBuf> = Vec::new();

    if !drm_path.exists() {
        return connected;
    }

    if let Ok(entries) = fs::read_dir(drm_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            let status_path = path.join("status");

            if let Ok(status) = fs::read_to_string(&status_path) {
                if status.trim() == "connected" {
                    connected.push(path);
                }
            }
        }
    }

    connected.sort();
    connected
}

#[cfg(target_os = "linux")]
fn get_display_count_linux() -> u32 {
    get_connected_displays_linux().len() as u32
}

#[cfg(target_os = "linux")]
fn get_display_info_linux(display_index: u32) -> Dictionary {
    use std::fs;

    let mut result = Dictionary::new();
    let connected = get_connected_displays_linux();

    if display_index as usize >= connected.len() {
        return result;
    }

    let display_path = &connected[display_index as usize];

    result.set("index", display_index as i64);

    // Parse EDID for physical dimensions
    let edid_path = display_path.join("edid");
    if let Ok(edid_data) = fs::read(&edid_path) {
        if edid_data.len() >= 23 {
            let width_cm = edid_data[21] as i64;
            let height_cm = edid_data[22] as i64;
            result.set("width_mm", width_cm * 10);
            result.set("height_mm", height_cm * 10);
        }
    }

    // Parse modes file for resolution and refresh rate
    let modes_path = display_path.join("modes");
    if let Ok(modes_content) = fs::read_to_string(&modes_path) {
        // First line is usually the preferred/current mode
        if let Some(first_mode) = modes_content.lines().next() {
            // Format is typically "1920x1080" or "1920x1080@60"
            let parts: Vec<&str> = first_mode.split(|c| c == 'x' || c == '@').collect();
            if parts.len() >= 2 {
                if let Ok(width) = parts[0].trim().parse::<i64>() {
                    result.set("width", width);
                }
                if let Ok(height) = parts[1].trim().parse::<i64>() {
                    result.set("height", height);
                }
            }
            if parts.len() >= 3 {
                if let Ok(refresh) = parts[2].trim().parse::<f64>() {
                    result.set("refresh_rate", refresh);
                }
            }
        }
    }

    // Check if primary (card0 connectors are typically primary)
    let path_str = display_path.to_string_lossy();
    let is_primary = path_str.contains("card0") && display_index == 0;
    result.set("is_primary", is_primary);

    // Position - would need X11/Wayland APIs for accurate values
    result.set("position_x", 0i64);
    result.set("position_y", 0i64);

    result
}

#[cfg(target_os = "linux")]
fn get_monitor_size_linux(monitor_index: u32) -> (u32, u32) {
    use std::fs;
    use std::path::Path;

    // Find connected displays by looking for edid files
    let drm_path = Path::new("/sys/class/drm");
    if !drm_path.exists() {
        return (0, 0);
    }

    let mut connected_edids: Vec<std::path::PathBuf> = Vec::new();

    if let Ok(entries) = fs::read_dir(drm_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            let status_path = path.join("status");
            let edid_path = path.join("edid");

            // Check if this output is connected and has EDID
            if let Ok(status) = fs::read_to_string(&status_path) {
                if status.trim() == "connected" && edid_path.exists() {
                    connected_edids.push(edid_path);
                }
            }
        }
    }

    // Sort for consistent ordering
    connected_edids.sort();

    if monitor_index as usize >= connected_edids.len() {
        return (0, 0);
    }

    // Read EDID and extract physical dimensions
    // EDID bytes 21-22 contain width and height in cm
    if let Ok(edid_data) = fs::read(&connected_edids[monitor_index as usize]) {
        if edid_data.len() >= 23 {
            let width_cm = edid_data[21] as u32;
            let height_cm = edid_data[22] as u32;
            // Convert cm to mm
            return (width_cm * 10, height_cm * 10);
        }
    }

    (0, 0)
}


// =============================================================================
// TimingAnalyzer - Performance-critical timing analysis for acquisition data
// =============================================================================

/// Timing analysis utilities for acquisition validation.
/// Computes jitter, detects dropped frames, and measures drift between streams.
#[derive(GodotClass)]
#[class(base=RefCounted)]
pub struct TimingAnalyzer;

#[godot_api]
impl IRefCounted for TimingAnalyzer {
    fn init(_base: Base<RefCounted>) -> Self {
        Self
    }
}

#[godot_api]
impl TimingAnalyzer {
    /// Compute jitter statistics for a timestamp series.
    /// Returns: {mean_interval_us, std_dev_us, max_gap_us, min_gap_us, frame_count}
    #[func]
    fn compute_jitter(timestamps_us: PackedInt64Array) -> Dictionary {
        let mut result = Dictionary::new();

        let ts: Vec<i64> = timestamps_us.to_vec();
        if ts.len() < 2 {
            result.set("error","Need at least 2 timestamps");
            return result;
        }

        // Compute intervals
        let intervals: Vec<i64> = ts.windows(2)
            .map(|w| w[1] - w[0])
            .collect();

        let n = intervals.len() as f64;

        // Mean
        let sum: i64 = intervals.iter().sum();
        let mean = sum as f64 / n;

        // Std dev
        let sum_sq: f64 = intervals.iter()
            .map(|&x| {
                let diff = x as f64 - mean;
                diff * diff
            })
            .sum();
        let variance = sum_sq / n;
        let std_dev = variance.sqrt();

        // Min/max
        let max_gap = *intervals.iter().max().unwrap_or(&0);
        let min_gap = *intervals.iter().min().unwrap_or(&0);

        result.set("mean_interval_us",mean);
        result.set("std_dev_us",std_dev);
        result.set("max_gap_us",max_gap);
        result.set("min_gap_us",min_gap);
        result.set("frame_count",(ts.len() as i64));
        result
    }

    /// Detect dropped frames based on expected interval.
    /// Returns indices where gap > expected * (1 + tolerance).
    #[func]
    fn detect_drops(
        timestamps_us: PackedInt64Array,
        expected_interval_us: i64,
        tolerance: f64
    ) -> PackedInt32Array {
        let ts: Vec<i64> = timestamps_us.to_vec();
        let threshold = (expected_interval_us as f64 * (1.0 + tolerance)) as i64;

        let mut dropped_indices = PackedInt32Array::new();

        for (i, window) in ts.windows(2).enumerate() {
            let gap = window[1] - window[0];
            if gap > threshold {
                dropped_indices.push(i as i32);
            }
        }

        dropped_indices
    }

    /// Compute nearest-neighbor offsets between two timestamp streams.
    /// For each timestamp in ts_a (typically slower stream like camera),
    /// find the nearest timestamp in ts_b (typically faster stream like stimulus).
    /// This handles different framerates naturally.
    /// Returns: {offset_mean_us, offset_max_us, offset_sd_us}
    #[func]
    fn compute_nearest_neighbor_offsets(
        ts_a: PackedInt64Array,
        ts_b: PackedInt64Array
    ) -> Dictionary {
        let mut result = Dictionary::new();

        let a: Vec<i64> = ts_a.to_vec();
        let b: Vec<i64> = ts_b.to_vec();

        if a.is_empty() || b.is_empty() {
            result.set("error", "Need at least 1 timestamp in each series");
            return result;
        }

        // ts_b should be sorted (timestamps are naturally sorted)
        let mut offsets: Vec<i64> = Vec::with_capacity(a.len());

        for &t_a in &a {
            // Binary search for nearest timestamp in ts_b
            let idx = b.partition_point(|&t| t < t_a);

            let nearest = if idx == 0 {
                b[0]
            } else if idx >= b.len() {
                b[b.len() - 1]
            } else {
                // Check both neighbors, pick closer one
                let before = b[idx - 1];
                let after = b[idx];
                if (t_a - before).abs() <= (after - t_a).abs() {
                    before
                } else {
                    after
                }
            };

            offsets.push(t_a - nearest);
        }

        // Compute statistics
        let n = offsets.len() as f64;
        let sum: i64 = offsets.iter().sum();
        let mean = sum as f64 / n;

        let max_abs = offsets.iter().map(|&x| x.abs()).max().unwrap_or(0);

        let sum_sq: f64 = offsets.iter().map(|&x| {
            let diff = x as f64 - mean;
            diff * diff
        }).sum();
        let sd = (sum_sq / n).sqrt();

        result.set("offset_mean_us", mean);
        result.set("offset_max_us", max_abs);
        result.set("offset_sd_us", sd);
        result.set("samples", a.len() as i64);
        result
    }

    /// Compute cross-correlation to find optimal alignment between streams.
    /// Converts timestamps to binary pulse trains and finds the lag that maximizes correlation.
    /// Returns: {optimal_lag_us, correlation}
    #[func]
    fn compute_cross_correlation(
        ts_a: PackedInt64Array,
        ts_b: PackedInt64Array,
        max_lag_us: i64
    ) -> Dictionary {
        let mut result = Dictionary::new();

        let a: Vec<i64> = ts_a.to_vec();
        let b: Vec<i64> = ts_b.to_vec();

        if a.len() < 2 || b.len() < 2 {
            result.set("error", "Need at least 2 timestamps in each series");
            return result;
        }

        // Use 1ms bins for cross-correlation
        let bin_size_us: i64 = 1000;

        // Find time range
        let start = a[0].min(b[0]);
        let end = *a.last().unwrap().max(b.last().unwrap());
        let n_bins = ((end - start) / bin_size_us + 1) as usize;

        // Limit bin count to prevent memory issues (max ~10 seconds at 1ms resolution)
        if n_bins > 10000 {
            result.set("optimal_lag_us", 0i64);
            result.set("correlation", 0.0);
            result.set("warning", "Recording too long for cross-correlation");
            return result;
        }

        // Create binary signals (1.0 where frame occurred, 0.0 otherwise)
        let mut sig_a = vec![0.0f64; n_bins];
        let mut sig_b = vec![0.0f64; n_bins];

        for &t in &a {
            let bin = ((t - start) / bin_size_us) as usize;
            if bin < n_bins {
                sig_a[bin] = 1.0;
            }
        }
        for &t in &b {
            let bin = ((t - start) / bin_size_us) as usize;
            if bin < n_bins {
                sig_b[bin] = 1.0;
            }
        }

        // Compute cross-correlation for each lag
        let max_lag_bins = (max_lag_us / bin_size_us) as i32;
        let mut best_lag: i64 = 0;
        let mut best_corr: f64 = f64::NEG_INFINITY;

        for lag in -max_lag_bins..=max_lag_bins {
            let corr = Self::correlate_at_lag(&sig_a, &sig_b, lag);
            if corr > best_corr {
                best_corr = corr;
                best_lag = (lag as i64) * bin_size_us;
            }
        }

        // Normalize correlation to roughly 0-1 range
        let norm_a: f64 = sig_a.iter().map(|&x| x * x).sum::<f64>().sqrt();
        let norm_b: f64 = sig_b.iter().map(|&x| x * x).sum::<f64>().sqrt();
        let norm_corr = if norm_a > 0.0 && norm_b > 0.0 {
            best_corr / (norm_a * norm_b)
        } else {
            0.0
        };

        result.set("optimal_lag_us", best_lag);
        result.set("correlation", norm_corr);
        result
    }

    /// Helper: compute correlation at a specific lag
    fn correlate_at_lag(sig_a: &[f64], sig_b: &[f64], lag: i32) -> f64 {
        let n = sig_a.len() as i32;
        let mut sum = 0.0;

        for i in 0..n {
            let j = i + lag;
            if j >= 0 && j < n {
                sum += sig_a[i as usize] * sig_b[j as usize];
            }
        }
        sum
    }

    /// Compute relative clock drift between two timestamp streams.
    /// Compares total elapsed time to detect if clocks are running at different rates.
    /// Returns: {relative_drift_ppm}
    #[func]
    fn compute_relative_drift(
        ts_a: PackedInt64Array,
        ts_b: PackedInt64Array
    ) -> Dictionary {
        let mut result = Dictionary::new();

        let a: Vec<i64> = ts_a.to_vec();
        let b: Vec<i64> = ts_b.to_vec();

        if a.len() < 2 || b.len() < 2 {
            result.set("error", "Need at least 2 timestamps in each series");
            return result;
        }

        let elapsed_a = (a.last().unwrap() - a[0]) as f64;
        let elapsed_b = (b.last().unwrap() - b[0]) as f64;

        let max_elapsed = elapsed_a.max(elapsed_b);
        let drift_ppm = if max_elapsed > 0.0 {
            ((elapsed_a - elapsed_b) / max_elapsed) * 1_000_000.0
        } else {
            0.0
        };

        result.set("relative_drift_ppm", drift_ppm);
        result.set("elapsed_a_us", elapsed_a);
        result.set("elapsed_b_us", elapsed_b);
        result
    }

    /// Full sync analysis combining all methods.
    /// IMPORTANT: Normalizes timestamps to relative time (from recording start) since
    /// camera and stimulus may use different time bases (hardware vs Godot time).
    /// Returns: {offset_mean_us, offset_max_us, offset_sd_us, optimal_lag_us, correlation, relative_drift_ppm}
    #[func]
    fn analyze_sync(
        ts_a: PackedInt64Array,
        ts_b: PackedInt64Array
    ) -> Dictionary {
        let mut result = Dictionary::new();

        let a: Vec<i64> = ts_a.to_vec();
        let b: Vec<i64> = ts_b.to_vec();

        if a.len() < 2 || b.len() < 2 {
            result.set("error", "Need at least 2 timestamps in each series");
            return result;
        }

        // Normalize both timestamp arrays to be relative to their first timestamp.
        // This handles different time bases (e.g., camera uses hardware time from boot,
        // stimulus uses Godot's Time.get_ticks_usec() which starts at 0).
        let a_start = a[0];
        let b_start = b[0];
        let a_normalized: Vec<i64> = a.iter().map(|&t| t - a_start).collect();
        let b_normalized: Vec<i64> = b.iter().map(|&t| t - b_start).collect();

        // Convert back to PackedInt64Array for the analysis functions
        let mut ts_a_norm = PackedInt64Array::new();
        let mut ts_b_norm = PackedInt64Array::new();
        for &t in &a_normalized {
            ts_a_norm.push(t);
        }
        for &t in &b_normalized {
            ts_b_norm.push(t);
        }

        // Nearest-neighbor offset analysis (on normalized timestamps)
        let nn_result = Self::compute_nearest_neighbor_offsets(ts_a_norm.clone(), ts_b_norm.clone());
        if nn_result.get("offset_mean_us").is_some() {
            result.set("offset_mean_us", nn_result.get("offset_mean_us").unwrap());
            result.set("offset_max_us", nn_result.get("offset_max_us").unwrap());
            result.set("offset_sd_us", nn_result.get("offset_sd_us").unwrap());
        }

        // Cross-correlation analysis (search +/- 100ms)
        let cc_result = Self::compute_cross_correlation(ts_a_norm.clone(), ts_b_norm.clone(), 100_000);
        if cc_result.get("optimal_lag_us").is_some() {
            result.set("optimal_lag_us", cc_result.get("optimal_lag_us").unwrap());
            result.set("correlation", cc_result.get("correlation").unwrap());
        }

        // Relative drift (uses original timestamps for elapsed time calculation)
        let drift_result = Self::compute_relative_drift(ts_a, ts_b);
        if drift_result.get("relative_drift_ppm").is_some() {
            result.set("relative_drift_ppm", drift_result.get("relative_drift_ppm").unwrap());
        }

        result
    }

    /// Full analysis of acquisition timing.
    /// Returns complete TimingReport as Dictionary.
    #[func]
    fn analyze(
        camera_ts: PackedInt64Array,
        stimulus_ts: PackedInt64Array,
        expected_camera_fps: f64,
        expected_stimulus_fps: f64
    ) -> Dictionary {
        let mut result = Dictionary::new();

        // Camera analysis
        let camera_jitter = Self::compute_jitter(camera_ts.clone());
        let expected_camera_interval_us = if expected_camera_fps > 0.0 {
            (1_000_000.0 / expected_camera_fps) as i64
        } else {
            33333 // Default 30fps
        };
        let camera_drops = Self::detect_drops(camera_ts.clone(), expected_camera_interval_us, 0.5);

        // Stimulus analysis
        let stimulus_jitter = Self::compute_jitter(stimulus_ts.clone());
        let expected_stimulus_interval_us = if expected_stimulus_fps > 0.0 {
            (1_000_000.0 / expected_stimulus_fps) as i64
        } else {
            16667 // Default 60fps
        };
        let stimulus_drops = Self::detect_drops(stimulus_ts.clone(), expected_stimulus_interval_us, 0.5);

        // Sync analysis (using new framerate-agnostic methods)
        let sync_report = Self::analyze_sync(camera_ts, stimulus_ts);

        // Quality assessment (must be done before moving jitter dicts)
        let mut quality = Dictionary::new();
        let mut issues: Vec<String> = Vec::new();

        // Check camera quality
        let camera_ok = Self::check_camera_quality(&camera_jitter, &camera_drops, expected_camera_fps, &mut issues);
        quality.set("camera_ok", camera_ok);

        // Check stimulus quality
        let stimulus_ok = Self::check_stimulus_quality(&stimulus_jitter, &stimulus_drops, expected_stimulus_fps, &mut issues);
        quality.set("stimulus_ok", stimulus_ok);

        // Check sync quality
        let sync_ok = Self::check_sync_quality(&sync_report, &mut issues);
        quality.set("sync_ok", sync_ok);

        // Now build camera report (moves camera_jitter)
        let mut camera_report = Dictionary::new();
        camera_report.set("frame_count", camera_jitter.get("frame_count").unwrap_or(Variant::from(0i64)));

        // Compute actual camera FPS before moving jitter
        if let Some(mean_interval) = camera_jitter.get("mean_interval_us") {
            let mean_us: f64 = mean_interval.try_to().unwrap_or(0.0);
            if mean_us > 0.0 {
                camera_report.set("actual_fps", 1_000_000.0 / mean_us);
            }
        }
        camera_report.set("jitter", camera_jitter);
        camera_report.set("dropped_count", camera_drops.len() as i64);
        camera_report.set("dropped_indices", camera_drops);

        result.set("camera", camera_report);

        // Build stimulus report (moves stimulus_jitter)
        let mut stimulus_report = Dictionary::new();
        stimulus_report.set("frame_count", stimulus_jitter.get("frame_count").unwrap_or(Variant::from(0i64)));

        // Compute actual stimulus FPS before moving jitter
        if let Some(mean_interval) = stimulus_jitter.get("mean_interval_us") {
            let mean_us: f64 = mean_interval.try_to().unwrap_or(0.0);
            if mean_us > 0.0 {
                stimulus_report.set("actual_fps", 1_000_000.0 / mean_us);
            }
        }
        stimulus_report.set("jitter", stimulus_jitter);
        stimulus_report.set("dropped_count", stimulus_drops.len() as i64);
        stimulus_report.set("dropped_indices", stimulus_drops);

        result.set("stimulus", stimulus_report);

        // Set sync report (moves sync_report)
        result.set("sync", sync_report);

        quality.set("overall_ok",(camera_ok && stimulus_ok && sync_ok));

        // Store issues as semicolon-separated string (simpler than array conversion)
        let issues_str = issues.join("; ");
        quality.set("issues", GString::from(issues_str.as_str()));
        quality.set("issue_count", issues.len() as i64);

        result.set("quality",quality);

        result
    }


    /// Check camera quality against thresholds
    fn check_camera_quality(
        jitter: &Dictionary,
        drops: &PackedInt32Array,
        expected_fps: f64,
        issues: &mut Vec<String>
    ) -> bool {
        let mut ok = true;

        // Check jitter (threshold: 2000µs = 2ms)
        if let Some(std_dev) = jitter.get("std_dev_us") {
            let jitter_us: f64 = std_dev.try_to().unwrap_or(0.0);
            if jitter_us > 2000.0 {
                issues.push(format!("Camera jitter too high: {:.0}µs (max: 2000µs)", jitter_us));
                ok = false;
            }
        }

        // Check dropped frames (threshold: 1%)
        if let Some(frame_count) = jitter.get("frame_count") {
            let total: i64 = frame_count.try_to().unwrap_or(0);
            let dropped = drops.len() as i64;
            if total > 0 {
                let drop_pct = (dropped as f64 / total as f64) * 100.0;
                if drop_pct > 1.0 {
                    issues.push(format!("Camera dropped frames: {:.1}% (max: 1%)", drop_pct));
                    ok = false;
                }
            }
        }

        // Check FPS deviation (threshold: ±1 FPS)
        if let Some(mean_interval) = jitter.get("mean_interval_us") {
            let mean_us: f64 = mean_interval.try_to().unwrap_or(0.0);
            if mean_us > 0.0 && expected_fps > 0.0 {
                let actual_fps = 1_000_000.0 / mean_us;
                let fps_diff = (actual_fps - expected_fps).abs();
                if fps_diff > 1.0 {
                    issues.push(format!("Camera FPS off target: {:.1} (expected: {:.1})", actual_fps, expected_fps));
                    ok = false;
                }
            }
        }

        ok
    }

    /// Check stimulus quality against thresholds
    fn check_stimulus_quality(
        jitter: &Dictionary,
        drops: &PackedInt32Array,
        expected_fps: f64,
        issues: &mut Vec<String>
    ) -> bool {
        let mut ok = true;

        // Check jitter (threshold: 1000µs = 1ms for stimulus)
        if let Some(std_dev) = jitter.get("std_dev_us") {
            let jitter_us: f64 = std_dev.try_to().unwrap_or(0.0);
            if jitter_us > 1000.0 {
                issues.push(format!("Stimulus jitter too high: {:.0}µs (max: 1000µs)", jitter_us));
                ok = false;
            }
        }

        // Check dropped frames (threshold: 0 for stimulus)
        if drops.len() > 0 {
            issues.push(format!("Stimulus dropped {} frames (max: 0)", drops.len()));
            ok = false;
        }

        // Check FPS deviation (threshold: ±0.5 FPS for stimulus)
        if let Some(mean_interval) = jitter.get("mean_interval_us") {
            let mean_us: f64 = mean_interval.try_to().unwrap_or(0.0);
            if mean_us > 0.0 && expected_fps > 0.0 {
                let actual_fps = 1_000_000.0 / mean_us;
                let fps_diff = (actual_fps - expected_fps).abs();
                if fps_diff > 0.5 {
                    issues.push(format!("Stimulus FPS off target: {:.2} (expected: {:.1})", actual_fps, expected_fps));
                    ok = false;
                }
            }
        }

        ok
    }

    /// Check sync quality against thresholds
    fn check_sync_quality(
        sync: &Dictionary,
        issues: &mut Vec<String>
    ) -> bool {
        let mut ok = true;

        // Check relative drift (threshold: 1000 ppm = 0.1%)
        if let Some(drift) = sync.get("relative_drift_ppm") {
            let drift_ppm: f64 = drift.try_to().unwrap_or(0.0);
            if drift_ppm.abs() > 1000.0 {
                issues.push(format!("Camera-stimulus drift too high: {:.0} ppm (max: 1000 ppm)", drift_ppm));
                ok = false;
            }
        }

        // Check max offset (threshold: 100ms = 100000µs)
        if let Some(offset) = sync.get("offset_max_us") {
            let max_offset: i64 = offset.try_to().unwrap_or(0);
            if max_offset > 100000 {
                issues.push(format!("Camera-stimulus max offset too high: {}µs (max: 100000µs)", max_offset));
                ok = false;
            }
        }

        // Check correlation (threshold: 0.5 - should be reasonably correlated)
        if let Some(corr) = sync.get("correlation") {
            let correlation: f64 = corr.try_to().unwrap_or(0.0);
            if correlation < 0.5 {
                issues.push(format!("Camera-stimulus correlation too low: {:.2} (min: 0.5)", correlation));
                ok = false;
            }
        }

        ok
    }
}
