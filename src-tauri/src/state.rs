//! Application state shared across Tauri commands.
//!
//! **Decomposed state (no god-mutex).** `AppState` is `Arc<AppState>` —
//! never locked as a whole. Each piece of mutable runtime state lives
//! behind its own `parking_lot::Mutex`, grouped by *co-access* (fields
//! always written together in one critical section share a lock; fields
//! touched in isolation get their own). The grouping is derived from the
//! measured command/event access map — see the state-decomposition
//! section of `docs/COMPUTE.md`.
//!
//! Lock groups:
//! - `config`    — persistent config (single source of truth for all params)
//! - `session`   — volatile hardware/display/camera session state
//! - `capture`   — the recording hot path: latest frame + timing ring +
//!   in-flight acquisition accumulator (the 60–100 fps
//!   `CameraEvt::Frame` writes all three, so one lock = one
//!   critical section = no multi-lock deadlock on the hot path)
//! - `handoff`   — post-acquisition handoff: pending save + last summary +
//!   anatomical image (low frequency, never co-accessed with hot fields)
//! - `active_oisi` — the `.oisi` the UI currently has open
//!
//! Immutable after startup (no lock): `threads` (channel handles; senders
//! and receivers are `Clone`, cloned out freely) and `monitors`
//! (detected once at launch, only read afterward).
//!
//! **Locking discipline** (enforced by review — the type system can't):
//! lock at most one group at a time; never hold a guard across IO / compute
//! / channel-send (lock → copy out → drop → do the work). The few remaining
//! multi-group sites have a documented, fixed lock order (see `events.rs`).

use std::path::PathBuf;
use std::sync::Arc;

use crossbeam_channel::{Receiver, Sender};
use parking_lot::Mutex;

use crate::export::{AcquisitionAccumulator, HardwareSnapshot};
use crate::messages::{AnalysisCmd, AnalysisEvt, CameraCmd, CameraEvt, StimulusCmd, StimulusEvt};
use openisi_params::config::{ConfigSnapshot, ConfigStore};
use crate::session::{MonitorInfo, Session};
use crate::timing::TimingCharacterization;

// =============================================================================
// Thread handles — immutable after startup
// =============================================================================

/// Channel endpoints for background threads. Senders and receivers are
/// `Clone` (crossbeam), so commands clone out the sender they need and the
/// event forwarder clones the receivers once at startup — no lock required
/// for steady-state use. The only mutable bit is the one-time stimulus-thread
/// spawn state, isolated behind its own small mutex.
pub struct ThreadHandles {
    /// Send commands to the stimulus thread.
    pub stimulus_tx: Sender<StimulusCmd>,
    /// Receive events from the stimulus thread (drained by event forwarder).
    pub stimulus_rx: Receiver<StimulusEvt>,
    /// Send commands to the camera thread.
    pub camera_tx: Sender<CameraCmd>,
    /// Receive events from the camera thread (drained by event forwarder).
    pub camera_rx: Receiver<CameraEvt>,
    /// Send commands to the analysis worker thread.
    pub analysis_tx: Sender<AnalysisCmd>,
    /// Receive events from the analysis worker thread (drained by event forwarder).
    pub analysis_rx: Receiver<AnalysisEvt>,
    /// One-time stimulus-thread spawn state — mutated exactly once, when a
    /// display is first selected. Isolated so the common case (cloning a
    /// sender) never touches a lock.
    pub stimulus_spawn: Mutex<StimulusSpawn>,
}

/// Held until a display is selected and the stimulus thread is spawned.
pub struct StimulusSpawn {
    pub cmd_rx: Option<Receiver<StimulusCmd>>,
    pub evt_tx: Option<Sender<StimulusEvt>>,
    pub spawned: bool,
}

// =============================================================================
// Capture group — the recording hot path
// =============================================================================

/// Camera timing ring buffer (for timing validation). The two timestamp
/// vectors always move in lockstep, so they live together with their cap.
pub struct CameraTimingRing {
    /// Recent camera hardware timestamps (µs since midnight).
    pub hw: Vec<i64>,
    /// Recent camera system timestamps (QPC µs), matching `hw`.
    pub sys: Vec<i64>,
    /// Maximum ring size.
    pub capacity: usize,
}

impl CameraTimingRing {
    pub fn new(capacity: usize) -> Self {
        Self {
            hw: Vec::new(),
            sys: Vec::new(),
            capacity,
        }
    }

    /// Push a paired (hw, sys) sample, trimming to `capacity` from the front.
    pub fn push(&mut self, hw_us: i64, sys_us: i64) {
        self.hw.push(hw_us);
        self.sys.push(sys_us);
        let overflow = self.hw.len().saturating_sub(self.capacity);
        if overflow > 0 {
            self.hw.drain(0..overflow);
            self.sys.drain(0..overflow);
        }
    }
}

/// Cached camera frame for UI preview.
pub struct CameraFrameCache {
    pub pixels: Vec<u16>,
    pub width: u32,
    pub height: u32,
    pub sequence_number: u64,
}

/// In-flight acquisition state. Created at acquisition start, consumed at end.
/// Contains acquisition-time snapshots frozen at start — never updated during recording.
pub struct AcquisitionState {
    /// Accumulates camera frames tagged by stimulus cycle.
    pub accumulator: AcquisitionAccumulator,
    // ── Acquisition-time snapshots (frozen at start) ────────────────
    /// All parameter values frozen at acquisition start (typed config).
    pub snapshot: ConfigSnapshot,
    /// Hardware snapshot (monitor + camera identity) frozen at acquisition start.
    pub hardware_snapshot: Option<HardwareSnapshot>,
    /// Timing characterization frozen at acquisition start.
    pub timing_characterization: Option<TimingCharacterization>,
}

/// The recording hot path, under one lock. `CameraEvt::Frame` writes the
/// frame cache, pushes the timing ring, and appends to the accumulator in a
/// single critical section — grouping them makes that exactly one lock.
pub struct Capture {
    /// Latest camera frame for UI preview (hot: written every frame).
    pub latest_frame: Option<CameraFrameCache>,
    /// Camera timing ring (hot: pushed every frame, read by validate_timing).
    pub timing: CameraTimingRing,
    /// In-flight acquisition (only `Some` during recording; hot during it).
    pub acquisition: Option<AcquisitionState>,
}

// =============================================================================
// Handoff group — post-acquisition, low frequency
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

/// Data awaiting user save confirmation after acquisition.
/// Contains acquisition-time snapshots (frozen at start) plus stimulus results.
/// Metadata (animal_id, notes, anatomical) is read from live state at save time.
pub struct PendingSave {
    pub camera_data: crate::export::AccumulatedData,
    pub stimulus_dataset: openisi_stimulus::dataset::StimulusDataset,
    pub schedule: crate::export::SweepSchedule,
    pub completed_normally: bool,
    // ── Acquisition-time snapshots (frozen at start) ──────────────
    /// All parameter values frozen at acquisition start (typed config).
    pub snapshot: ConfigSnapshot,
    /// Hardware snapshot (monitor + camera identity) frozen at acquisition start.
    pub hardware_snapshot: Option<HardwareSnapshot>,
    /// Timing characterization frozen at acquisition start.
    pub timing_characterization: Option<TimingCharacterization>,
}

/// Post-acquisition handoff state. Written when a run completes, consumed by
/// save/discard. Never co-accessed with the capture hot path.
pub struct Handoff {
    /// Data awaiting user save confirmation.
    pub pending_save: Option<PendingSave>,
    /// Summary of the most recently saved acquisition.
    pub last_summary: Option<AcquisitionSummary>,
    /// Anatomical image captured during Focus (read at save time).
    pub anatomical: Option<ndarray::Array2<u8>>,
}

// =============================================================================
// Top-level app state — Arc<AppState>, never locked as a whole
// =============================================================================

/// Application state managed by Tauri. Decomposed into independently-locked
/// co-access groups (see module docs). Handed to threads by `Arc` clone.
pub struct AppState {
    /// Channel handles — immutable after startup.
    pub threads: ThreadHandles,
    /// Detected monitors — write-once at launch, read-only afterward.
    pub monitors: Arc<Vec<MonitorInfo>>,

    /// Persistent typed configuration store (single source of truth for all config).
    pub config: Arc<Mutex<ConfigStore>>,
    /// Volatile hardware/display/camera session state.
    pub session: Arc<Mutex<Session>>,
    /// Recording hot path: frame + timing ring + acquisition accumulator.
    pub capture: Arc<Mutex<Capture>>,
    /// Post-acquisition handoff: pending save + last summary + anatomical.
    pub handoff: Arc<Mutex<Handoff>>,

    /// Path to the `.oisi` file the UI currently has open. Set by
    /// `set_active_oisi`; read by `get_/set_analysis_params` to target the
    /// right file. `None` when no file is open.
    ///
    /// **Concurrency invariant:** callers MUST capture this at the start of
    /// any long-running operation (lock, clone the `PathBuf`, drop the
    /// guard). Re-reading mid-run would let a UI `set_active_oisi(other_file)`
    /// silently reroute writes to a different file.
    pub active_oisi: Arc<Mutex<Option<PathBuf>>>,
}

/// Default ring capacity for the camera timing buffer.
const CAMERA_RING_CAPACITY: usize = 500;

impl AppState {
    /// Create initial state from the loaded config store and the freshly-created
    /// thread channels + detected monitors. Setup mutates the (un-cloned)
    /// `Arc` contents before managing the state.
    pub fn new(
        config: ConfigStore,
        threads: ThreadHandles,
        monitors: Vec<MonitorInfo>,
    ) -> Self {
        Self {
            threads,
            monitors: Arc::new(monitors),
            config: Arc::new(Mutex::new(config)),
            session: Arc::new(Mutex::new(Session::new())),
            capture: Arc::new(Mutex::new(Capture {
                latest_frame: None,
                timing: CameraTimingRing::new(CAMERA_RING_CAPACITY),
                acquisition: None,
            })),
            handoff: Arc::new(Mutex::new(Handoff {
                pending_save: None,
                last_summary: None,
                anatomical: None,
            })),
            active_oisi: Arc::new(Mutex::new(None)),
        }
    }

    /// Spawn the stimulus thread for the given monitor. Reads system tuning
    /// from the config store, then spawns. The caller must NOT hold any other
    /// lock — this takes the config and stimulus-spawn locks internally.
    pub fn spawn_stimulus_thread(&self, monitor: &MonitorInfo) {
        // Take the one-time spawn handles. If already spawned or missing, bail.
        let (cmd_rx, evt_tx) = {
            let mut spawn = self.threads.stimulus_spawn.lock();
            if spawn.spawned {
                tracing::warn!("stimulus thread already spawned");
                return;
            }
            match (spawn.cmd_rx.take(), spawn.evt_tx.take()) {
                (Some(rx), Some(tx)) => {
                    spawn.spawned = true;
                    (rx, tx)
                }
                _ => {
                    tracing::error!("stimulus spawn handles unavailable");
                    return;
                }
            }
        };

        let monitor_index = monitor.index;
        let width = monitor.width_px;
        let height = monitor.height_px;
        let position = monitor.position;

        // Read system tuning and initial background into the stimulus thread's
        // config from the typed config snapshot. `drop_detection_threshold` is
        // intentionally not passed — the live stimulus thread uses absolute DWM
        // present-count gaps.
        let config = {
            let cs = self.config.lock().snapshot();
            crate::stimulus_thread::StimulusConfig {
                monitor_index,
                monitor_width_px: width,
                monitor_height_px: height,
                monitor_position: position,
                preview_width_px: cs.rig.system.preview_width_px,
                preview_interval_ms: cs.rig.system.preview_interval_ms,
                preview_cycle_sec: cs.rig.system.preview_cycle_sec,
                idle_sleep_ms: cs.rig.system.idle_sleep_ms,
                drop_detection_warmup_frames: cs.rig.system.drop_detection_warmup_frames,
                initial_bg_luminance: cs.experiment.stimulus.params.background_luminance,
            }
        };

        if let Err(e) = std::thread::Builder::new()
            .name("stimulus".into())
            .spawn(move || {
                crate::stimulus_thread::run(cmd_rx, evt_tx, config);
            })
        {
            tracing::error!(error = %e, "failed to spawn stimulus thread");
            return;
        }

        tracing::info!(monitor = monitor_index, "stimulus thread spawned");
    }
}
