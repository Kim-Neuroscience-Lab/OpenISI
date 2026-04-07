//! Rust bindings for the PCO camera SDK.
//!
//! Provides safe wrappers around `sc2_cam.dll` (camera control) and
//! `pco_recorder.dll` (ring buffer recording). DLLs are loaded at runtime
//! via `libloading` — no compile-time .lib needed.
//!
//! # Architecture
//!
//! - `ffi` module: Raw FFI types and function signatures (packed structs, `extern "system"`)
//! - `Camera`: Safe wrapper for camera lifecycle (open → configure → arm → record → close)
//! - `Recorder`: Safe wrapper for ring-buffer recording with image retrieval
//!
//! # Usage
//!
//! ```no_run
//! # fn main() -> Result<(), pco_sdk::PcoError> {
//! let sdk = pco_sdk::Sdk::load()?;
//! let mut cam = sdk.open_camera(0)?;
//! let info = cam.info();
//! cam.set_exposure_us(33000)?;
//! cam.arm()?;
//!
//! let mut recorder = cam.create_recorder(10)?; // 10-frame ring buffer
//! recorder.start()?;
//! // ... poll for frames ...
//! recorder.stop()?;
//! # Ok(())
//! # }
//! ```

pub mod ffi;

use std::path::{Path, PathBuf};
use std::ptr;

use libloading::Library;
use thiserror::Error;

// ════════════════════════════════════════════════════════════════════════
// PCO Recorder constants (from PCO SDK documentation)
// ════════════════════════════════════════════════════════════════════════

/// Recorder mode: record to files on disk.
pub const RECORDER_MODE_FILE: u16 = 1;
/// Recorder mode: record to PC memory (RAM).
pub const RECORDER_MODE_MEMORY: u16 = 2;
/// Recorder mode: record to camera RAM (CamRAM).
pub const RECORDER_MODE_CAMRAM: u16 = 3;

/// Memory mode type: plain sequence buffer (no overwrite).
pub const RECORDER_TYPE_SEQUENCE: u16 = 1;
/// Memory mode type: ring buffer (overwrites oldest frames).
pub const RECORDER_TYPE_RING_BUFFER: u16 = 2;
/// Memory mode type: FIFO (blocks producer when full).
pub const RECORDER_TYPE_FIFO: u16 = 3;

// ════════════════════════════════════════════════════════════════════════
// Error types
// ════════════════════════════════════════════════════════════════════════

#[derive(Debug, Error)]
pub enum PcoError {
    #[error("PCO SDK error 0x{code:08X}: {message}")]
    Sdk { code: u32, message: String },

    #[error("Failed to load DLL: {0}")]
    DllLoad(String),

    #[error("Failed to load function {name}: {message}")]
    FnLoad { name: String, message: String },

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, PcoError>;

// ════════════════════════════════════════════════════════════════════════
// SDK loader
// ════════════════════════════════════════════════════════════════════════

/// Loaded PCO SDK. Holds the DLL handles and function pointers.
pub struct Sdk {
    _sdk_lib: Library,
    _rec_lib: Library,
    fns: SdkFunctions,
}

/// All function pointers we need from the two DLLs.
struct SdkFunctions {
    // sc2_cam.dll
    open_camera: unsafe extern "system" fn(*mut *mut std::ffi::c_void, u16) -> u32,
    open_camera_ex: Option<unsafe extern "system" fn(*mut *mut std::ffi::c_void, *mut ffi::PcoOpenStruct) -> u32>,
    close_camera: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
    arm_camera: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
    set_recording_state: unsafe extern "system" fn(*mut std::ffi::c_void, u16) -> u32,
    get_sizes: unsafe extern "system" fn(*mut std::ffi::c_void, *mut u16, *mut u16, *mut u16, *mut u16) -> u32,
    get_camera_description: unsafe extern "system" fn(*mut std::ffi::c_void, *mut ffi::PcoDescription) -> u32,
    set_pixel_rate: unsafe extern "system" fn(*mut std::ffi::c_void, u32) -> u32,
    set_delay_exposure_time: unsafe extern "system" fn(*mut std::ffi::c_void, u32, u32, u16, u16) -> u32,
    get_coc_runtime: unsafe extern "system" fn(*mut std::ffi::c_void, *mut u32, *mut u32) -> u32,
    set_timestamp_mode: unsafe extern "system" fn(*mut std::ffi::c_void, u16) -> u32,
    get_camera_health_status: unsafe extern "system" fn(*mut std::ffi::c_void, *mut u32, *mut u32, *mut u32) -> u32,
    get_camera_type: unsafe extern "system" fn(*mut std::ffi::c_void, *mut ffi::PcoCameraType) -> u32,
    set_binning: unsafe extern "system" fn(*mut std::ffi::c_void, u16, u16) -> u32,
    set_roi: unsafe extern "system" fn(*mut std::ffi::c_void, u16, u16, u16, u16) -> u32,

    // pco_recorder.dll
    recorder_create: unsafe extern "system" fn(
        *mut *mut std::ffi::c_void, // phRecorder
        *mut *mut std::ffi::c_void, // phCamera (array)
        *mut u32,                    // pdwImageDistribution
        u16,                         // wArrayLength
        u16,                         // wRecorderMode
        *const u8,                   // szPath (null)
        *mut u32,                    // pdwMaxImageCount
    ) -> u32,
    recorder_delete: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
    recorder_init: unsafe extern "system" fn(
        *mut std::ffi::c_void, // hRecorder
        *mut u32,              // pdwImageCount
        u16,                   // wArrayLength
        u16,                   // wType
        u16,                   // wNoOverwrite
        *const u8,             // szPath (null)
        *mut u16,              // pwRamSegment
    ) -> u32,
    recorder_start_record: unsafe extern "system" fn(*mut std::ffi::c_void, *mut std::ffi::c_void) -> u32,
    recorder_stop_record: unsafe extern "system" fn(*mut std::ffi::c_void, *mut std::ffi::c_void) -> u32,
    recorder_get_status: unsafe extern "system" fn(
        *mut std::ffi::c_void,  // hRecorder
        *mut std::ffi::c_void,  // hCamera
        *mut u8,                // pbIsRunning (bool)
        *mut u8,                // pbAutoExpState
        *mut u32,               // pdwLastError
        *mut u32,               // pdwProcImgCount
        *mut u32,               // pdwReqImgCount
        *mut u8,                // pbBuffersFull
        *mut u8,                // pbFIFOOverflow
        *mut u32,               // pdwStartTime
        *mut u32,               // pdwStopTime
    ) -> u32,
    recorder_copy_image: unsafe extern "system" fn(
        *mut std::ffi::c_void,       // hRecorder
        *mut std::ffi::c_void,       // hCamera
        u32,                          // dwImageIndex
        u16, u16, u16, u16,          // ROI: x0, y0, x1, y1
        *mut u16,                     // pImageBuffer
        *mut u32,                     // pdwImageNumber
        *mut ffi::PcoMetadata,        // pMetadata
        *mut ffi::PcoTimestamp,       // pTimestamp
    ) -> u32,
}

/// Default DLL search path: the pco pip package's win_x64 directory.
fn default_dll_dir() -> Option<PathBuf> {
    // Look for the DLLs in common locations.
    let candidates = [
        // Bundled with the Python pco package in the OpenISI venv.
        PathBuf::from(r"C:\Program Files\Kim-Neuroscience-Lab\OpenISI\.venv\Lib\site-packages\pco\win_x64"),
        // Standalone PCO SDK install.
        PathBuf::from(r"C:\Program Files\PCO Digital Camera Toolbox\pco.sdk\bin64"),
        PathBuf::from(r"C:\Program Files (x86)\PCO Digital Camera Toolbox\pco.sdk\bin64"),
    ];
    candidates.into_iter().find(|p| p.join("sc2_cam.dll").exists())
}

impl Sdk {
    /// Load the PCO SDK from the default DLL directory.
    pub fn load() -> Result<Self> {
        let dir = default_dll_dir()
            .ok_or_else(|| PcoError::DllLoad("Could not find PCO SDK DLLs".into()))?;
        Self::load_from(&dir)
    }

    /// Load the PCO SDK from a specific directory.
    pub fn load_from(dll_dir: &Path) -> Result<Self> {
        let sdk_path = dll_dir.join("sc2_cam.dll");
        let rec_path = dll_dir.join("pco_recorder.dll");

        // SAFETY: We're loading well-known DLLs from a verified path.
        let sdk_lib = unsafe {
            Library::new(&sdk_path)
                .map_err(|e| PcoError::DllLoad(format!("{}: {e}", sdk_path.display())))?
        };
        let rec_lib = unsafe {
            Library::new(&rec_path)
                .map_err(|e| PcoError::DllLoad(format!("{}: {e}", rec_path.display())))?
        };

        // Load function pointers.
        let fns = unsafe { load_functions(&sdk_lib, &rec_lib)? };

        Ok(Self {
            _sdk_lib: sdk_lib,
            _rec_lib: rec_lib,
            fns,
        })
    }

    /// Enumerate available PCO cameras.
    /// Tries opening cameras with indices 0, 1, 2... up to `max_cameras`.
    /// Each camera is opened briefly to query info, then closed.
    /// Enumerate available PCO cameras.
    ///
    /// Opens cameras with indices 0..max_cameras, queries serial number via
    /// PCO_GetCameraType, and deduplicates by serial number (PCO can open the
    /// same physical camera on multiple board numbers).
    pub fn enumerate_cameras(&self, max_cameras: u16) -> Vec<CameraInfo> {
        let mut cameras = Vec::new();
        let mut seen_serials = std::collections::HashSet::new();

        for i in 0..max_cameras {
            match self.open_camera(i) {
                Ok(mut cam) => {
                    let _ = cam.set_max_pixel_rate();
                    let _ = cam.arm();
                    let mut info = cam.info();
                    info.index = i;
                    if let Ok(fps) = cam.get_max_fps() {
                        info.max_fps = fps;
                    }

                    // Deduplicate by serial number — the only reliable unique
                    // identifier per physical camera.
                    if info.serial_number == 0 || seen_serials.insert(info.serial_number) {
                        cameras.push(info);
                    }
                    // Camera closes on Drop.
                }
                Err(_) => break,
            }
        }
        cameras
    }

    /// Open a PCO camera. Uses PCO_OpenCameraEx with dialog suppression if available,
    /// otherwise falls back to PCO_OpenCamera.
    pub fn open_camera(&self, camera_number: u16) -> Result<Camera<'_>> {
        let mut handle: *mut std::ffi::c_void = ptr::null_mut();

        if let Some(open_ex) = self.fns.open_camera_ex {
            let mut open_struct = ffi::PcoOpenStruct::new_silent(camera_number);
            let rc = unsafe { open_ex(&mut handle, &mut open_struct) };
            check_rc(rc)?;
        } else {
            let rc = unsafe { (self.fns.open_camera)(&mut handle, camera_number) };
            check_rc(rc)?;
        }

        if handle.is_null() {
            return Err(PcoError::Other("PCO_OpenCamera returned null handle".into()));
        }

        self.finish_open(handle)
    }

    /// Common post-open logic: query sizes and description.
    fn finish_open(&self, handle: *mut std::ffi::c_void) -> Result<Camera<'_>> {
        let mut width_act: u16 = 0;
        let mut height_act: u16 = 0;
        let mut width_max: u16 = 0;
        let mut height_max: u16 = 0;
        let rc = unsafe {
            (self.fns.get_sizes)(handle, &mut width_act, &mut height_act, &mut width_max, &mut height_max)
        };
        check_rc(rc)?;

        let mut desc = ffi::PcoDescription::zeroed();
        desc.w_size = std::mem::size_of::<ffi::PcoDescription>() as u16;
        let rc = unsafe { (self.fns.get_camera_description)(handle, &mut desc) };
        check_rc(rc)?;

        Ok(Camera {
            sdk: self,
            handle,
            width: width_act as u32,
            height: height_act as u32,
            width_max: width_max as u32,
            height_max: height_max as u32,
            desc,
        })
    }
}

// ════════════════════════════════════════════════════════════════════════
// Camera
// ════════════════════════════════════════════════════════════════════════

/// An opened PCO camera.
pub struct Camera<'sdk> {
    sdk: &'sdk Sdk,
    handle: *mut std::ffi::c_void,
    pub width: u32,
    pub height: u32,
    pub width_max: u32,
    pub height_max: u32,
    desc: ffi::PcoDescription,
}

/// Camera info returned from enumeration.
#[derive(Debug, Clone)]
pub struct CameraInfo {
    pub index: u16,
    pub serial_number: u32,
    pub name: String,
    pub interface_type: String,
    pub width: u32,
    pub height: u32,
    pub width_max: u32,
    pub height_max: u32,
    pub pixel_rates: Vec<u32>,
    pub min_exposure_ns: u32,
    pub max_exposure_ms: u32,
    pub max_fps: f64,
}

impl<'sdk> Camera<'sdk> {
    /// Copy pixel rates from the packed description struct.
    fn pixel_rates_array(&self) -> [u32; 4] {
        // Must copy from packed struct before creating references.
        let rates = self.desc.dw_pixel_rate_desc;
        rates
    }

    /// Get camera info.
    pub fn info(&self) -> CameraInfo {
        let rates = self.pixel_rates_array();
        let pixel_rates: Vec<u32> = rates.iter().copied().filter(|&r| r > 0).collect();
        let min_exp = self.desc.dw_min_expos_desc;
        let max_exp = self.desc.dw_max_expos_desc;

        // Get camera type info (serial number, model name, interface).
        let (serial_number, name, interface_type) = self.get_camera_type_info()
            .map_err(|e| eprintln!("[pco] Warning: get_camera_type_info failed: {e}"))
            .expect("Failed to get camera type info from PCO SDK");

        CameraInfo {
            index: 0,
            serial_number,
            name,
            interface_type,
            width: self.width,
            height: self.height,
            width_max: self.width_max,
            height_max: self.height_max,
            pixel_rates,
            min_exposure_ns: min_exp,
            max_exposure_ms: max_exp,
            max_fps: 0.0, // Computed after arm + get_coc_runtime
        }
    }

    /// Query camera type, serial number, and interface from PCO_GetCameraType.
    fn get_camera_type_info(&self) -> Result<(u32, String, String)> {
        let mut cam_type = ffi::PcoCameraType::new();
        let rc = unsafe {
            (self.sdk.fns.get_camera_type)(self.handle, &mut cam_type)
        };
        check_rc(rc)?;

        // Copy fields from packed struct before referencing.
        let type_code = cam_type.w_cam_type;
        let serial = cam_type.dw_serial_number;
        let iface = cam_type.w_interface_type;

        // Camera type code → name (from PCO SDK sdk.py lines 2604-2627).
        let name = match type_code {
            0x0100 => "pco.1200HS",
            0x0200 => "pco.1300",
            0x0220 => "pco.1600",
            0x0240 => "pco.2000",
            0x0260 => "pco.4000",
            0x0830 => "pco.1400",
            0x0900 => "pco.pixelfly usb",
            0x1000 => "pco.dimax",
            0x1010 => "pco.dimax_TV",
            0x1020 => "pco.dimax CS",
            0x1300 => "pco.edge 5.5 CL",
            0x1302 => "pco.edge 4.2 CL",
            0x1304 => "pco.edge MT",
            0x1310 => "pco.edge GL",
            0x1320 => "pco.edge USB3",
            0x1340 => "pco.edge CLHS",
            0x1400 => "pco.flim",
            0x1600 => "pco.panda",
            0x1800 => "pco.edge family",
            0x1700 => "pco.dicam family",
            0x1900 => "pco.dimax family",
            0x1A00 => "pco.pixelfly family",
            other => return Err(PcoError::Other(format!("Unknown PCO camera type code: 0x{other:04X}"))),
        }.to_string();

        // Interface type code → name (from PCO SDK sdk.py lines 2629-2638).
        let interface_type = match iface {
            0x0001 => "FireWire",
            0x0002 => "Camera Link",
            0x0003 => "USB 2.0",
            0x0004 => "GigE",
            0x0005 => "Serial",
            0x0006 => "USB 3.0",
            0x0007 => "CLHS",
            0x0009 => "USB 3.1 Gen 1",
            other => return Err(PcoError::Other(format!("Unknown PCO interface type code: 0x{other:04X}"))),
        }.to_string();

        Ok((serial, name, interface_type))
    }

    /// Set pixel rate to the fastest available.
    pub fn set_max_pixel_rate(&mut self) -> Result<u32> {
        let rates = self.pixel_rates_array();
        let max_rate = rates.iter().copied()
            .filter(|&r| r > 0)
            .max()
            .ok_or_else(|| PcoError::Other("No valid pixel rates".into()))?;

        let rc = unsafe { (self.sdk.fns.set_pixel_rate)(self.handle, max_rate) };
        check_rc(rc)?;
        Ok(max_rate)
    }

    /// Set exposure time in microseconds.
    pub fn set_exposure_us(&mut self, exposure_us: u32) -> Result<()> {
        // timebase 1 = microseconds, delay = 0
        let rc = unsafe {
            (self.sdk.fns.set_delay_exposure_time)(self.handle, 0, exposure_us, 1, 1)
        };
        check_rc(rc)
    }

    /// Query available binning from the camera hardware description.
    /// Returns (max_horz, horz_stepping, max_vert, vert_stepping).
    /// Stepping=1 means any factor 1..max is valid.
    /// Stepping=2 means only powers of 2 (1, 2, 4, 8...) up to max.
    pub fn available_binning(&self) -> (u16, u16, u16, u16) {
        let max_h = self.desc.w_max_bin_horz_desc;
        let step_h = self.desc.w_bin_horz_stepping_desc;
        let max_v = self.desc.w_max_bin_vert_desc;
        let step_v = self.desc.w_bin_vert_stepping_desc;
        (max_h, step_h, max_v, step_v)
    }

    /// Check if a binning factor is valid for this camera.
    pub fn is_valid_binning(&self, binning: u16) -> bool {
        let (max_h, step_h, max_v, step_v) = self.available_binning();
        // Stepping: 0 or 1 = any value up to max. 2 = powers of 2 only.
        let is_power_of_2 = binning > 0 && (binning & (binning - 1)) == 0;
        let valid_h = binning >= 1 && binning <= max_h && (step_h <= 1 || is_power_of_2);
        let valid_v = binning >= 1 && binning <= max_v && (step_v <= 1 || is_power_of_2);
        valid_h && valid_v
    }

    /// Set pixel binning. Must be called before arm().
    /// Validates against hardware capabilities and sets ROI to full binned resolution.
    pub fn set_binning(&mut self, binning_x: u16, binning_y: u16) -> Result<()> {
        let (max_h, _, max_v, _) = self.available_binning();
        if binning_x > max_h || binning_y > max_v {
            return Err(PcoError::Other(format!(
                "Binning {}x{} exceeds camera max {}x{}", binning_x, binning_y, max_h, max_v
            )));
        }
        let rc = unsafe { (self.sdk.fns.set_binning)(self.handle, binning_x, binning_y) };
        check_rc(rc)?;
        eprintln!("[pco] binning set to {}x{}", binning_x, binning_y);

        // Set ROI to full binned resolution.
        // PCO ROI is 1-based: (x1, y1, x2, y2).
        let roi_w = self.width_max / binning_x as u32;
        let roi_h = self.height_max / binning_y as u32;
        let rc = unsafe { (self.sdk.fns.set_roi)(self.handle, 1, 1, roi_w as u16, roi_h as u16) };
        check_rc(rc)?;
        eprintln!("[pco] ROI set to 1,1..{},{} (full binned)", roi_w, roi_h);

        Ok(())
    }

    /// Enable binary timestamp mode (embedded in first 14 pixels of each frame).
    pub fn set_timestamp_binary(&mut self) -> Result<()> {
        let rc = unsafe { (self.sdk.fns.set_timestamp_mode)(self.handle, 1) };
        check_rc(rc)
    }

    /// Arm the camera (apply settings, prepare for recording).
    pub fn arm(&mut self) -> Result<()> {
        let rc = unsafe { (self.sdk.fns.arm_camera)(self.handle) };
        check_rc(rc)?;

        // Re-read sizes after arm (they may change).
        let mut w: u16 = 0;
        let mut h: u16 = 0;
        let mut wm: u16 = 0;
        let mut hm: u16 = 0;
        let rc = unsafe { (self.sdk.fns.get_sizes)(self.handle, &mut w, &mut h, &mut wm, &mut hm) };
        check_rc(rc)?;
        self.width = w as u32;
        self.height = h as u32;

        Ok(())
    }

    /// Get the cycle-of-command runtime (readout + exposure time).
    /// Returns (seconds_part, nanoseconds_part).
    pub fn get_coc_runtime(&self) -> Result<(u32, u32)> {
        let mut time_s: u32 = 0;
        let mut time_ns: u32 = 0;
        let rc = unsafe { (self.sdk.fns.get_coc_runtime)(self.handle, &mut time_s, &mut time_ns) };
        check_rc(rc)?;
        Ok((time_s, time_ns))
    }

    /// Get the maximum FPS based on current settings.
    pub fn get_max_fps(&self) -> Result<f64> {
        let (s, ns) = self.get_coc_runtime()?;
        let cycle_sec = s as f64 + ns as f64 * 1e-9;
        if cycle_sec > 0.0 {
            Ok(1.0 / cycle_sec)
        } else {
            Ok(0.0)
        }
    }

    /// Get camera health status (warning, error, status flags).
    pub fn health_status(&self) -> Result<(u32, u32, u32)> {
        let mut warning: u32 = 0;
        let mut error: u32 = 0;
        let mut status: u32 = 0;
        let rc = unsafe {
            (self.sdk.fns.get_camera_health_status)(self.handle, &mut warning, &mut error, &mut status)
        };
        check_rc(rc)?;
        Ok((warning, error, status))
    }

    /// Create a ring-buffer recorder with the given number of frames.
    pub fn create_recorder(&mut self, num_frames: u32) -> Result<Recorder<'_, 'sdk>> {
        let mut rec_handle: *mut std::ffi::c_void = ptr::null_mut();
        let mut cam_handle_arr = [self.handle];
        let mut max_img_count: u32 = 0;

        let rc = unsafe {
            (self.sdk.fns.recorder_create)(
                &mut rec_handle,
                cam_handle_arr.as_mut_ptr(),
                ptr::null_mut(), // NULL = auto-distribute images
                1,                      // wArrayLength (1 camera)
                RECORDER_MODE_MEMORY,   // PC memory recording
                ptr::null(),
                &mut max_img_count,
            )
        };
        check_rc(rc)?;

        eprintln!("[pco] recorder created, max images: {max_img_count}");

        // Init the recorder as a ring buffer in memory mode.
        let mut img_count = [num_frames];
        let mut ram_seg: u16 = 0;
        let rc = unsafe {
            (self.sdk.fns.recorder_init)(
                rec_handle,
                img_count.as_mut_ptr(),
                1,                          // wArrayLength
                RECORDER_TYPE_RING_BUFFER,  // ring buffer (overwrites oldest)
                0,                          // wNoOverwrite (0 = allow overwrite)
                ptr::null(),
                &mut ram_seg,
            )
        };
        check_rc(rc)?;

        Ok(Recorder {
            camera: self,
            handle: rec_handle,
            num_frames,
            last_proc_count: 0,
        })
    }

    /// Raw camera handle (for advanced use).
    pub fn raw_handle(&self) -> *mut std::ffi::c_void {
        self.handle
    }
}

impl<'sdk> Drop for Camera<'sdk> {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            // Stop recording if active.
            unsafe { (self.sdk.fns.set_recording_state)(self.handle, 0) };
            unsafe { (self.sdk.fns.close_camera)(self.handle) };
            self.handle = ptr::null_mut();
        }
    }
}

// ════════════════════════════════════════════════════════════════════════
// Recorder
// ════════════════════════════════════════════════════════════════════════

/// Ring-buffer recorder for continuous image acquisition.
pub struct Recorder<'cam, 'sdk> {
    camera: &'cam mut Camera<'sdk>,
    handle: *mut std::ffi::c_void,
    #[allow(dead_code)]
    num_frames: u32,
    last_proc_count: u32,
}

/// Status of the recorder.
#[derive(Debug, Clone)]
pub struct RecorderStatus {
    pub is_running: bool,
    pub last_error: u32,
    pub processed_count: u32,
    pub requested_count: u32,
    pub buffers_full: bool,
    pub fifo_overflow: bool,
}

/// A captured frame.
pub struct Frame {
    /// Raw 16-bit pixel data (width × height).
    pub pixels: Vec<u16>,
    pub width: u32,
    pub height: u32,
    /// Image number from the camera.
    pub image_number: u32,
    /// Hardware timestamp.
    pub timestamp: FrameTimestamp,
    /// Metadata from the camera.
    pub metadata: ffi::PcoMetadata,
}

/// Decoded hardware timestamp.
#[derive(Debug, Clone, Default)]
pub struct FrameTimestamp {
    pub year: u16,
    pub month: u16,
    pub day: u16,
    pub hour: u16,
    pub minute: u16,
    pub second: u16,
    pub microseconds: u32,
}

impl FrameTimestamp {
    /// Convert to microseconds since midnight.
    pub fn to_us_since_midnight(&self) -> i64 {
        let h = self.hour as i64;
        let m = self.minute as i64;
        let s = self.second as i64;
        let us = self.microseconds as i64;
        h * 3_600_000_000 + m * 60_000_000 + s * 1_000_000 + us
    }
}

/// Constant for "latest image" in the ring buffer.
const PCO_LATEST_IMAGE: u32 = 0xFFFFFFFF;

impl<'cam, 'sdk> Recorder<'cam, 'sdk> {
    /// Start recording. The recorder internally sets the camera recording state.
    pub fn start(&mut self) -> Result<()> {
        let rc = unsafe {
            (self.camera.sdk.fns.recorder_start_record)(self.handle, self.camera.handle)
        };
        check_rc(rc)?;
        self.last_proc_count = 0;
        Ok(())
    }

    /// Stop recording.
    pub fn stop(&mut self) -> Result<()> {
        let rc = unsafe {
            (self.camera.sdk.fns.recorder_stop_record)(self.handle, self.camera.handle)
        };
        check_rc(rc)?;
        let rc = unsafe {
            (self.camera.sdk.fns.set_recording_state)(self.camera.handle, 0)
        };
        check_rc(rc)?;
        Ok(())
    }

    /// Get recorder status.
    pub fn status(&self) -> Result<RecorderStatus> {
        let mut is_running: u8 = 0;
        let mut auto_exp: u8 = 0;
        let mut last_error: u32 = 0;
        let mut proc_count: u32 = 0;
        let mut req_count: u32 = 0;
        let mut buf_full: u8 = 0;
        let mut fifo_overflow: u8 = 0;
        let mut start_time: u32 = 0;
        let mut stop_time: u32 = 0;

        let rc = unsafe {
            (self.camera.sdk.fns.recorder_get_status)(
                self.handle, self.camera.handle,
                &mut is_running, &mut auto_exp,
                &mut last_error, &mut proc_count, &mut req_count,
                &mut buf_full, &mut fifo_overflow,
                &mut start_time, &mut stop_time,
            )
        };
        check_rc(rc)?;

        Ok(RecorderStatus {
            is_running: is_running != 0,
            last_error,
            processed_count: proc_count,
            requested_count: req_count,
            buffers_full: buf_full != 0,
            fifo_overflow: fifo_overflow != 0,
        })
    }

    /// Check if new frames are available since the last call to `get_latest_frame`.
    pub fn has_new_frame(&self) -> Result<bool> {
        let status = self.status()?;
        Ok(status.processed_count > self.last_proc_count)
    }

    /// Get the latest frame from the ring buffer.
    /// Returns `None` if no new frame is available.
    pub fn get_latest_frame(&mut self) -> Result<Option<Frame>> {
        let status = self.status()?;
        if status.processed_count <= self.last_proc_count {
            return Ok(None);
        }
        self.last_proc_count = status.processed_count;

        let w = self.camera.width;
        let h = self.camera.height;
        let pixel_count = (w * h) as usize;
        let mut pixels = vec![0u16; pixel_count];
        let mut image_number: u32 = 0;
        let mut metadata = ffi::PcoMetadata::zeroed();
        metadata.w_size = std::mem::size_of::<ffi::PcoMetadata>() as u16;
        let mut timestamp = ffi::PcoTimestamp::zeroed();
        timestamp.w_size = std::mem::size_of::<ffi::PcoTimestamp>() as u16;

        let rc = unsafe {
            (self.camera.sdk.fns.recorder_copy_image)(
                self.handle,
                self.camera.handle,
                PCO_LATEST_IMAGE,
                1, 1, w as u16, h as u16, // ROI: full frame (1-based)
                pixels.as_mut_ptr(),
                &mut image_number,
                &mut metadata,
                &mut timestamp,
            )
        };
        check_rc(rc)?;

        Ok(Some(Frame {
            pixels,
            width: w,
            height: h,
            image_number,
            timestamp: FrameTimestamp {
                year: timestamp.w_year,
                month: timestamp.w_month,
                day: timestamp.w_day,
                hour: timestamp.w_hour,
                minute: timestamp.w_minute,
                second: timestamp.w_second,
                microseconds: timestamp.dw_microseconds,
            },
            metadata,
        }))
    }
}

impl<'cam, 'sdk> Drop for Recorder<'cam, 'sdk> {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            // Best-effort stop and cleanup.
            unsafe {
                let _ = (self.camera.sdk.fns.recorder_stop_record)(self.handle, self.camera.handle);
                let _ = (self.camera.sdk.fns.set_recording_state)(self.camera.handle, 0);
                let _ = (self.camera.sdk.fns.recorder_delete)(self.handle);
            }
            self.handle = ptr::null_mut();
        }
    }
}

// ════════════════════════════════════════════════════════════════════════
// Helpers
// ════════════════════════════════════════════════════════════════════════

fn check_rc(rc: u32) -> Result<()> {
    if rc == 0 {
        Ok(())
    } else {
        Err(PcoError::Sdk {
            code: rc,
            message: format!("PCO error code 0x{rc:08X}"),
        })
    }
}

/// Load all function pointers from the DLLs.
///
/// SAFETY: Caller must ensure the Library handles remain alive as long as
/// the returned SdkFunctions is in use (guaranteed by Sdk struct lifetime).
unsafe fn load_functions(sdk: &Library, rec: &Library) -> Result<SdkFunctions> {
    macro_rules! load {
        ($lib:expr, $name:literal) => {{
            let sym = unsafe { $lib.get::<*const ()>($name.as_bytes()) }
                .map_err(|e| PcoError::FnLoad {
                    name: $name.into(),
                    message: e.to_string(),
                })?;
            unsafe { std::mem::transmute(*sym) }
        }};
    }

    // Optional function — returns None if not exported by this SDK version.
    macro_rules! load_opt {
        ($lib:expr, $name:literal) => {{
            match unsafe { $lib.get::<*const ()>($name.as_bytes()) } {
                Ok(sym) => Some(unsafe { std::mem::transmute(*sym) }),
                Err(_) => {
                    eprintln!("[pco] {} not available in this SDK version", $name);
                    None
                }
            }
        }};
    }

    Ok(SdkFunctions {
        // sc2_cam.dll
        open_camera: load!(sdk, "PCO_OpenCamera"),
        open_camera_ex: load_opt!(sdk, "PCO_OpenCameraEx"),
        close_camera: load!(sdk, "PCO_CloseCamera"),
        arm_camera: load!(sdk, "PCO_ArmCamera"),
        set_recording_state: load!(sdk, "PCO_SetRecordingState"),
        get_sizes: load!(sdk, "PCO_GetSizes"),
        get_camera_description: load!(sdk, "PCO_GetCameraDescription"),
        set_pixel_rate: load!(sdk, "PCO_SetPixelRate"),
        set_delay_exposure_time: load!(sdk, "PCO_SetDelayExposureTime"),
        get_coc_runtime: load!(sdk, "PCO_GetCOCRuntime"),
        set_timestamp_mode: load!(sdk, "PCO_SetTimestampMode"),
        get_camera_health_status: load!(sdk, "PCO_GetCameraHealthStatus"),
        get_camera_type: load!(sdk, "PCO_GetCameraType"),
        set_binning: load!(sdk, "PCO_SetBinning"),
        set_roi: load!(sdk, "PCO_SetROI"),

        // pco_recorder.dll
        recorder_create: load!(rec, "PCO_RecorderCreate"),
        recorder_delete: load!(rec, "PCO_RecorderDelete"),
        recorder_init: load!(rec, "PCO_RecorderInit"),
        recorder_start_record: load!(rec, "PCO_RecorderStartRecord"),
        recorder_stop_record: load!(rec, "PCO_RecorderStopRecord"),
        recorder_get_status: load!(rec, "PCO_RecorderGetStatus"),
        recorder_copy_image: load!(rec, "PCO_RecorderCopyImage"),
    })
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- Recorder mode constants (documenting correct PCO SDK values) ---

    #[test]
    fn recorder_mode_file_is_1() {
        assert_eq!(RECORDER_MODE_FILE, 1,
            "PCO_RecorderCreate wRecorderMode for file recording must be 1");
    }

    #[test]
    fn recorder_mode_memory_is_2() {
        assert_eq!(RECORDER_MODE_MEMORY, 2,
            "PCO_RecorderCreate wRecorderMode for memory recording must be 2 (NOT 0)");
    }

    #[test]
    fn recorder_mode_camram_is_3() {
        assert_eq!(RECORDER_MODE_CAMRAM, 3,
            "PCO_RecorderCreate wRecorderMode for CamRAM recording must be 3");
    }

    #[test]
    fn recorder_type_sequence_is_1() {
        assert_eq!(RECORDER_TYPE_SEQUENCE, 1,
            "PCO_RecorderInit wType for sequence buffer must be 1");
    }

    #[test]
    fn recorder_type_ring_buffer_is_2() {
        assert_eq!(RECORDER_TYPE_RING_BUFFER, 2,
            "PCO_RecorderInit wType for ring buffer must be 2 (NOT 0 or 1)");
    }

    #[test]
    fn recorder_type_fifo_is_3() {
        assert_eq!(RECORDER_TYPE_FIFO, 3,
            "PCO_RecorderInit wType for FIFO must be 3");
    }

    // --- FrameTimestamp ---

    #[test]
    fn timestamp_to_us_since_midnight() {
        let ts = FrameTimestamp {
            year: 2026,
            month: 3,
            day: 22,
            hour: 14,
            minute: 30,
            second: 45,
            microseconds: 123456,
        };
        // 14*3600 + 30*60 + 45 = 52245 seconds = 52_245_000_000 us + 123456
        let expected = 14 * 3_600_000_000i64 + 30 * 60_000_000 + 45 * 1_000_000 + 123456;
        assert_eq!(ts.to_us_since_midnight(), expected);
    }

    #[test]
    fn timestamp_midnight_is_zero() {
        let ts = FrameTimestamp::default();
        assert_eq!(ts.to_us_since_midnight(), 0);
    }

    // --- Error formatting ---

    #[test]
    fn check_rc_ok_on_zero() {
        assert!(check_rc(0).is_ok());
    }

    #[test]
    fn check_rc_err_on_nonzero() {
        let err = check_rc(0x80000001).unwrap_err();
        match err {
            PcoError::Sdk { code, .. } => assert_eq!(code, 0x80000001),
            _ => panic!("Expected PcoError::Sdk"),
        }
    }
}
