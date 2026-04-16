//! Application state shared across Tauri commands.
//!
//! Layered state with clear ownership:
//! - `registry`: persistent config (Arc<Mutex<Registry>>) — single source of truth
//! - `session`: volatile hardware state (resets each launch)
//! - `acquisition`: in-flight acquisition data (only during recording)
//! - `threads`: channel handles for background threads

use std::sync::{Arc, Mutex};

use crossbeam_channel::{Receiver, Sender};

use crate::params::{Registry, RegistrySnapshot};
use crate::export::{AcquisitionAccumulator, HardwareSnapshot};
use crate::messages::{CameraCmd, CameraEvt, StimulusCmd, StimulusEvt};
use crate::session::{MonitorInfo, Session};
use crate::timing::TimingCharacterization;

// =============================================================================
// Thread handles
// =============================================================================

/// Channel endpoints and spawn state for background threads.
pub struct ThreadHandles {
    /// Send commands to stimulus thread.
    pub stimulus_tx: Option<Sender<StimulusCmd>>,
    /// Receive events from stimulus thread (consumed by event forwarder).
    pub stimulus_rx: Option<Receiver<StimulusEvt>>,
    /// Send commands to camera thread.
    pub camera_tx: Option<Sender<CameraCmd>>,
    /// Receive events from camera thread (consumed by event forwarder).
    pub camera_rx: Option<Receiver<CameraEvt>>,
    /// Held until a display is selected and the stimulus thread is spawned.
    pub stim_cmd_rx: Option<Receiver<StimulusCmd>>,
    pub stim_evt_tx: Option<Sender<StimulusEvt>>,
    /// Whether the stimulus thread has been spawned.
    pub stimulus_thread_spawned: bool,
}

// =============================================================================
// Acquisition state (exists only during recording)
// =============================================================================

/// In-flight acquisition state. Created at acquisition start, consumed at end.
/// Contains acquisition-time snapshots frozen at start — never updated during recording.
pub struct AcquisitionState {
    /// Accumulates camera frames tagged by stimulus cycle.
    pub accumulator: AcquisitionAccumulator,
    // ── Acquisition-time snapshots (frozen at start) ────────────────
    /// All parameter values frozen at acquisition start.
    pub snapshot: RegistrySnapshot,
    /// Hardware snapshot (monitor + camera identity) frozen at acquisition start.
    pub hardware_snapshot: Option<HardwareSnapshot>,
    /// Timing characterization frozen at acquisition start.
    pub timing_characterization: Option<TimingCharacterization>,
}

// =============================================================================
// Camera preview cache
// =============================================================================

/// Cached camera frame for UI preview.
pub struct CameraFrameCache {
    pub pixels: Vec<u16>,
    pub width: u32,
    pub height: u32,
    pub sequence_number: u64,
}

// =============================================================================
// Acquisition summary
// =============================================================================

/// Summary of a completed acquisition (persists until next run).
#[derive(Debug, Clone, serde::Serialize)]
pub struct AcquisitionSummary {
    pub total_sweeps: usize,
    pub total_frames: usize,
    pub dropped_frames: usize,
    pub duration_sec: f64,
    pub file_path: Option<String>,
}

// =============================================================================
// Top-level app state
// =============================================================================

/// Application state managed by Tauri. Layered by lifecycle:
/// - `registry`: persistent config, read at startup, written on explicit user changes
/// - `session`: volatile hardware state, resets each launch
/// - `acquisition`: only exists during recording
pub struct AppState {
    // ── Parameter registry (single source of truth for all config) ──
    pub registry: Arc<Mutex<Registry>>,

    // ── Session (volatile hardware state) ───────��───────────────────
    pub session: Session,
    pub monitors: Vec<MonitorInfo>,

    // ── Thread handles ───────��────────────────────────────────���─────
    pub threads: ThreadHandles,

    // ── Camera preview ──────────────────────────────────────────────
    pub latest_camera_frame: Option<CameraFrameCache>,

    // ── Camera timing ring buffer (for timing validation) ────────────
    /// Recent camera hardware timestamps (µs since midnight), ring buffer.
    /// Populated by event forwarder from every frame. Used by validate_timing.
    pub camera_hw_timestamps_ring: Vec<i64>,
    /// Recent camera system timestamps (QPC µs), matching ring buffer.
    pub camera_sys_timestamps_ring: Vec<i64>,
    /// Maximum ring size.
    pub camera_ring_capacity: usize,

    // ── Acquisition (in-flight, only during recording) ──────────────
    pub acquisition: Option<AcquisitionState>,
    pub last_acquisition_summary: Option<AcquisitionSummary>,

    // ── Anatomical image (captured during Focus) ──────────────────
    pub anatomical_image: Option<ndarray::Array2<u8>>,

    // ── Pending save (awaiting user confirmation) ─────────────────
    pub pending_save: Option<PendingSave>,
}

/// Data awaiting user save confirmation after acquisition.
/// Contains acquisition-time snapshots (frozen at start) plus stimulus results.
/// Metadata (animal_id, notes, anatomical) is read from live state at save time.
pub struct PendingSave {
    pub camera_data: crate::export::AccumulatedData,
    pub stimulus_dataset: openisi_stimulus::dataset::StimulusDataset,
    pub schedule: crate::export::SweepSchedule,
    pub completed_normally: bool,
    // ── Acquisition-time snapshots (frozen at start) ──────────────
    /// All parameter values frozen at acquisition start.
    pub snapshot: RegistrySnapshot,
    /// Hardware snapshot (monitor + camera identity) frozen at acquisition start.
    pub hardware_snapshot: Option<HardwareSnapshot>,
    /// Timing characterization frozen at acquisition start.
    pub timing_characterization: Option<TimingCharacterization>,
}

impl AppState {
    /// Create initial state with a loaded registry.
    pub fn new(registry: Registry) -> Self {
        let session = Session::new();

        Self {
            registry: Arc::new(Mutex::new(registry)),
            session,
            monitors: Vec::new(),
            threads: ThreadHandles {
                stimulus_tx: None,
                stimulus_rx: None,
                camera_tx: None,
                camera_rx: None,
                stim_cmd_rx: None,
                stim_evt_tx: None,
                stimulus_thread_spawned: false,
            },
            latest_camera_frame: None,
            camera_hw_timestamps_ring: Vec::new(),
            camera_sys_timestamps_ring: Vec::new(),
            camera_ring_capacity: 500,
            anatomical_image: None,
            acquisition: None,
            last_acquisition_summary: None,
            pending_save: None,
        }
    }

    /// Spawn the stimulus thread for the given monitor.
    pub fn spawn_stimulus_thread(&mut self, monitor: &MonitorInfo) {
        if self.threads.stimulus_thread_spawned {
            eprintln!("[state] stimulus thread already spawned");
            return;
        }

        let cmd_rx = match self.threads.stim_cmd_rx.take() {
            Some(rx) => rx,
            None => {
                eprintln!("[state] no stim_cmd_rx available");
                return;
            }
        };
        let evt_tx = match self.threads.stim_evt_tx.take() {
            Some(tx) => tx,
            None => {
                eprintln!("[state] no stim_evt_tx available");
                return;
            }
        };

        let monitor_index = monitor.index;
        let width = monitor.width_px;
        let height = monitor.height_px;
        let position = monitor.position;

        // Read system tuning and initial background from registry.
        let (preview_width_px, preview_interval_ms, preview_cycle_sec,
             idle_sleep_ms, fps_window_frames, drop_detection_warmup_frames,
             drop_detection_threshold, initial_bg) = match self.registry.lock() {
            Ok(reg) => (
                reg.preview_width_px(),
                reg.preview_interval_ms(),
                reg.preview_cycle_sec(),
                reg.idle_sleep_ms(),
                reg.fps_window_frames(),
                reg.drop_detection_warmup_frames(),
                reg.drop_detection_threshold(),
                reg.background_luminance(),
            ),
            Err(_) => {
                eprintln!("[state] registry lock poisoned in spawn_stimulus_thread");
                return;
            }
        };

        if let Err(e) = std::thread::Builder::new()
            .name("stimulus".into())
            .spawn(move || {
                crate::stimulus_thread::run(
                    cmd_rx, evt_tx, monitor_index, width, height, position,
                    preview_width_px, preview_interval_ms, preview_cycle_sec,
                    idle_sleep_ms, fps_window_frames, drop_detection_warmup_frames,
                    drop_detection_threshold, initial_bg,
                );
            })
        {
            eprintln!("[state] failed to spawn stimulus thread: {e}");
            return;
        }

        self.threads.stimulus_thread_spawned = true;
        eprintln!("[state] stimulus thread spawned for monitor {}", monitor_index);
    }

    /// Start a new acquisition. Creates the accumulator and freezes all config snapshots.
    pub fn start_acquisition(
        &mut self,
        cam_width: u32,
        cam_height: u32,
        snapshot: RegistrySnapshot,
        hardware_snapshot: Option<HardwareSnapshot>,
        timing_characterization: Option<TimingCharacterization>,
    ) {
        let mut accumulator = AcquisitionAccumulator::new();
        accumulator.start(cam_width, cam_height);
        self.acquisition = Some(AcquisitionState {
            accumulator,
            snapshot,
            hardware_snapshot,
            timing_characterization,
        });
        self.session.is_acquiring = true;
    }

    /// End the current acquisition. Returns the accumulator for export.
    pub fn end_acquisition(&mut self) -> Option<AcquisitionState> {
        self.session.is_acquiring = false;
        self.acquisition.take()
    }
}
