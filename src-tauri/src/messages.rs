//! Thread communication messages.
//!
//! Defines the command/event types for crossbeam channels between
//! the main thread, stimulus thread, and camera thread.

use crate::params::RegistrySnapshot;
use crate::session::MonitorInfo;

// =============================================================================
// Stimulus Thread
// =============================================================================

/// Commands sent TO the stimulus thread.
pub enum StimulusCmd {
    /// Configure and start stimulus acquisition.
    StartAcquisition(AcquisitionCommand),
    /// Stop the current acquisition.
    Stop,
    /// Show preview pattern (no recording).
    Preview(PreviewCommand),
    /// Stop preview.
    StopPreview,
    /// Shut down the thread entirely.
    Shutdown,
}

/// Full configuration for an acquisition run. Self-contained — the stimulus thread
/// receives everything it needs in this single message.
pub struct AcquisitionCommand {
    pub snapshot: RegistrySnapshot,
    pub monitor: MonitorInfo,
    pub measured_refresh_hz: f64,
}

/// Configuration for preview mode.
pub struct PreviewCommand {
    pub snapshot: RegistrySnapshot,
    pub monitor: MonitorInfo,
}

/// Events sent FROM the stimulus thread.
pub enum StimulusEvt {
    /// Thread initialized successfully (window created, GPU ready).
    Ready,
    /// A frame was rendered during acquisition.
    Frame(StimulusFrameRecord),
    /// Stimulus preview frame (PNG bytes for the scientist's preview panel).
    /// Sent periodically during acquisition and preview mode (~10 fps).
    PreviewFrame(StimulusPreviewFrame),
    /// Acquisition completed normally.
    Complete(AcquisitionResult),
    /// Acquisition stopped by user.
    Stopped,
    /// Thread error.
    Error(String),
}

/// Preview frame data for the scientist's sidebar.
#[derive(Debug, Clone)]
pub struct StimulusPreviewFrame {
    /// RGBA pixel data (small resolution, e.g. 320x180)
    pub rgba_pixels: Vec<u8>,
    /// Width of the preview image
    pub width: u32,
    /// Height of the preview image
    pub height: u32,
}

/// Per-frame data sent to the main thread for UI updates.
#[derive(Debug, Clone)]
pub struct StimulusFrameRecord {
    /// Hardware timestamp in microseconds
    pub timestamp_us: i64,
    /// Current sequencer state name
    pub state: String,
    /// Current sweep index
    pub sweep_index: usize,
    /// Total sweeps
    pub total_sweeps: usize,
    /// Progress within current state (0–1)
    pub state_progress: f64,
    /// Frame delta in microseconds
    pub frame_delta_us: i64,
    /// Total elapsed time in seconds
    pub elapsed_sec: f64,
    /// Total remaining time in seconds
    pub remaining_sec: f64,
    /// Current condition name (e.g. "LR", "RL") — from sweep sequence.
    pub condition: String,
    /// Condition occurrence (rep index, 0-based).
    pub condition_occurrence: u32,
}

/// Result of a completed acquisition.
pub struct AcquisitionResult {
    /// The completed stimulus dataset (for .oisi export)
    pub dataset: openisi_stimulus::dataset::StimulusDataset,
    /// Realized sweep schedule — condition name per sweep, in order.
    pub sweep_sequence: Vec<String>,
    /// QPC timestamp (microseconds) at start of each sweep.
    pub sweep_start_us: Vec<i64>,
    /// QPC timestamp (microseconds) at end of each sweep.
    pub sweep_end_us: Vec<i64>,
    /// Whether acquisition completed naturally (all sweeps done) vs stopped early.
    pub completed_normally: bool,
}

// =============================================================================
// Camera Thread
// =============================================================================

/// Commands sent TO the camera thread.
pub enum CameraCmd {
    /// Enumerate available cameras (results sent via CameraEvt::Enumerated).
    Enumerate,
    /// Connect to camera by index, with initial exposure and binning.
    Connect { index: u16, exposure_us: u32, binning: u16 },
    /// Disconnect from the camera.
    Disconnect,
    /// Set camera exposure in microseconds.
    SetExposure(u32),
    /// Shut down the thread entirely.
    Shutdown,
}

/// Events sent FROM the camera thread.
pub enum CameraEvt {
    /// Enumeration results.
    Enumerated(Vec<CameraDeviceInfo>),
    /// Camera connected successfully.
    Connected(CameraConnectedInfo),
    /// Camera disconnected.
    Disconnected,
    /// A new frame is available.
    Frame(CameraFrameData),
    /// Connection or error.
    Error(String),
}

/// Info about a detected camera (from enumeration).
#[derive(Debug, Clone, serde::Serialize)]
pub struct CameraDeviceInfo {
    pub index: u16,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub max_fps: f64,
}

/// Information sent when camera connects.
#[derive(Debug, Clone)]
pub struct CameraConnectedInfo {
    pub model: String,
    pub width_px: u32,
    pub height_px: u32,
    pub bits_per_pixel: u32,
}

/// Camera frame data.
#[derive(Debug, Clone)]
pub struct CameraFrameData {
    /// Raw pixel data (16-bit grayscale)
    pub pixels: Vec<u16>,
    /// Frame width
    pub width: u32,
    /// Frame height
    pub height: u32,
    /// Frame sequence number from camera hardware
    pub sequence_number: u64,
    /// Camera hardware timestamp in microseconds (from camera's internal clock)
    pub hardware_timestamp_us: i64,
    /// System timestamp in microseconds (QPC at frame read time, same clock as stimulus vsync)
    pub system_timestamp_us: i64,
}
