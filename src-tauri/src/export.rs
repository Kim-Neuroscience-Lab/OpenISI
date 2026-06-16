//! .oisi file export — writes acquisition data to HDF5.
//!
//! During acquisition, ALL camera frames are accumulated in order (including
//! baselines and inter-trial periods). Each frame carries both the camera
//! hardware timestamp and the system QPC timestamp for clock synchronization.
//! When the stimulus thread signals completion, the data is written to .oisi.

use std::path::Path;

use isi_analysis::oisi_schema::name;
use openisi_stimulus::dataset::StimulusDataset;
use serde::Serialize;

use crate::error::{AppError, AppResult};

/// Hardware configuration snapshot for embedding in .oisi files.
#[derive(Debug, Clone, Serialize)]
pub struct HardwareSnapshot {
    pub monitor_name: String,
    pub monitor_width_px: u32,
    pub monitor_height_px: u32,
    pub monitor_width_cm: f64,
    pub monitor_height_cm: f64,
    pub monitor_refresh_hz: f64,
    pub measured_refresh_hz: f64,
    pub gamma_corrected: bool,
    pub camera_model: String,
    pub camera_width_px: u32,
    pub camera_height_px: u32,
}

/// Wrap a true HDF5-API failure during .oisi export. Used for dataset /
/// group creation, attribute writes, and any libhdf5 call that produces
/// an `hdf5::Error`. Frontend sees `category: "Analysis", code: "E_HDF5"`.
fn hdf5_err(context: impl Into<String>, source: hdf5::Error) -> AppError {
    AppError::Analysis(isi_analysis::AnalysisError::hdf5(context, source))
}

/// Wrap a filesystem-layer failure during .oisi export (open, flush,
/// rename, the .partial-file dance, serde JSON serialization). Distinct
/// from `hdf5_err` so disk-full / permission / fs-rename failures
/// surface as `category: "Io", code: "E_IO"` in the frontend instead
/// of being miscategorized as analysis errors.
fn fs_err(msg: String) -> AppError {
    AppError::Io(std::io::Error::other(msg))
}

/// Accumulates ALL camera frames in acquisition order.
/// No grouping by condition — all frames kept, including baselines.
/// Stimulus state for each frame is computed analytically by analysis
/// from the camera timestamps + sweep schedule. No runtime alignment needed.
pub struct AcquisitionAccumulator {
    /// Raw u16 pixel data per frame, in acquisition order.
    frames: Vec<Vec<u16>>,
    /// Camera hardware timestamps (microseconds since midnight).
    hardware_timestamps_us: Vec<i64>,
    /// System QPC timestamps (microseconds, same clock as stimulus vsync).
    system_timestamps_us: Vec<i64>,
    /// Camera sequence numbers (for hardware-level gap detection).
    sequence_numbers: Vec<u64>,
    /// Camera sensor dimensions.
    pub width: u32,
    pub height: u32,
    /// Whether acquisition is active.
    active: bool,
}

impl Default for AcquisitionAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

impl AcquisitionAccumulator {
    pub fn new() -> Self {
        Self {
            frames: Vec::new(),
            hardware_timestamps_us: Vec::new(),
            system_timestamps_us: Vec::new(),
            sequence_numbers: Vec::new(),
            width: 0,
            height: 0,
            active: false,
        }
    }

    /// Start accumulating for a new acquisition.
    pub fn start(&mut self, width: u32, height: u32) {
        self.frames.clear();
        self.hardware_timestamps_us.clear();
        self.system_timestamps_us.clear();
        self.sequence_numbers.clear();
        self.width = width;
        self.height = height;
        self.active = true;
    }

    /// Add a camera frame. ALL frames are stored — no filtering by stimulus state.
    pub fn add_frame(
        &mut self,
        pixels: Vec<u16>,
        hardware_timestamp_us: i64,
        system_timestamp_us: i64,
        sequence_number: u64,
    ) {
        if !self.active {
            return;
        }
        self.frames.push(pixels);
        self.hardware_timestamps_us.push(hardware_timestamp_us);
        self.system_timestamps_us.push(system_timestamp_us);
        self.sequence_numbers.push(sequence_number);
    }

    /// Stop accumulating and return the data for export.
    pub fn finish(self) -> AccumulatedData {
        AccumulatedData {
            frames: self.frames,
            hardware_timestamps_us: self.hardware_timestamps_us,
            system_timestamps_us: self.system_timestamps_us,
            sequence_numbers: self.sequence_numbers,
            width: self.width,
            height: self.height,
        }
    }

    /// Check if accumulator is active.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Get stats for logging.
    pub fn stats(&self) -> String {
        format!(
            "{} frames, {}x{}",
            self.frames.len(),
            self.width,
            self.height
        )
    }
}

/// Data returned by the accumulator after acquisition ends.
pub struct AccumulatedData {
    pub frames: Vec<Vec<u16>>,
    pub hardware_timestamps_us: Vec<i64>,
    pub system_timestamps_us: Vec<i64>,
    pub sequence_numbers: Vec<u64>,
    pub width: u32,
    pub height: u32,
}

impl AccumulatedData {
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }
}

/// Write acquisition data to a .oisi file.
///
/// Creates a new .oisi file with:
/// - Root attributes (version, created_at, source_type)
/// - /acquisition/camera/frames — u16 (T, H, W) raw sensor data, gzip compressed
/// - /acquisition/camera/hardware_timestamps_us — i64 (T,)
/// - /acquisition/camera/system_timestamps_us — i64 (T,)
/// - /acquisition/camera/sequence_numbers — u64 (T,)
/// - /acquisition/stimulus/ — per-frame stimulus arrays
/// - /hardware — group with monitor/camera attributes
/// - /protocol — full protocol JSON (if provided)
///
/// Sweep schedule — when each sweep started and ended.
pub struct SweepSchedule {
    pub sweep_sequence: Vec<String>,
    pub sweep_start_us: Vec<i64>,
    pub sweep_end_us: Vec<i64>,
}

/// Session metadata (animal ID, notes) for embedding in .oisi files.
pub struct SessionMetadata {
    pub animal_id: String,
    pub notes: String,
}

/// Everything that goes into an `.oisi` file — the parameter object for
/// [`write_oisi`] (the destination `path` stays a separate argument, since it's
/// the "where", not the "what"). The two boolean fields are orthogonal
/// provenance flags, named here so they can't be transposed at the call site.
pub struct OisiBundle<'a> {
    pub stimulus_dataset: &'a StimulusDataset,
    pub camera_data: AccumulatedData,
    pub snapshot: &'a openisi_params::config::ConfigSnapshot,
    pub hardware: Option<&'a HardwareSnapshot>,
    pub schedule: &'a SweepSchedule,
    pub timing: Option<&'a crate::timing::TimingCharacterization>,
    pub session_meta: Option<&'a SessionMetadata>,
    pub anatomical: Option<&'a ndarray::Array2<u8>>,
    pub acquisition_complete: bool,
    pub stimulus_timing_validatable: bool,
}

pub fn write_oisi(path: &Path, bundle: OisiBundle) -> AppResult<String> {
    use isi_analysis::io;

    // Bind every field as a local so the body below — written against these
    // names — stays unchanged.
    let OisiBundle {
        stimulus_dataset,
        camera_data,
        snapshot,
        hardware,
        schedule,
        timing,
        session_meta,
        anatomical,
        acquisition_complete,
        stimulus_timing_validatable,
    } = bundle;

    // Atomic write protocol: write to `<path>.partial`, finalize, then
    // `fs::rename` to the final `path`. The canonical `.oisi` file at
    // `path` is never observed in a half-written state by another
    // process. On any error during write, the `*.partial` file is left
    // in place (forensic) — NEVER silently cleaned up. A user seeing
    // `<file>.oisi.partial` next to a missing `<file>.oisi` knows
    // exactly which acquisition crashed and can inspect what got
    // written before deciding how to proceed.
    let partial_path = path.with_extension(match path.extension().and_then(|e| e.to_str()) {
        Some("oisi") => "oisi.partial".to_string(),
        _ => "partial".to_string(),
    });

    // Create the file with metadata at the partial path.
    io::create(&partial_path, "raw_acquisition")
        .map_err(|e| fs_err(format!("Failed to create .oisi.partial: {e}")))?;

    // Open for writing.
    let file = hdf5::File::open_rw(&partial_path)
        .map_err(|e| fs_err(format!("Failed to open .oisi.partial for writing: {e}")))?;

    // Software version for provenance.
    isi_analysis::io::write_str_attr(&file, name::SOFTWARE_VERSION, env!("CARGO_PKG_VERSION"))?;

    // Write stimulus metadata.
    let metadata = stimulus_dataset.export_metadata();
    let meta_json = serde_json::to_string_pretty(&metadata)
        .map_err(|e| fs_err(format!("Failed to serialize metadata: {e}")))?;
    isi_analysis::io::write_str_attr(&file, name::STIMULUS_METADATA, &meta_json)?;

    // Capture provenance — serialize the typed `RigConfig` + `ExperimentConfig`
    // (derived from the acquisition snapshot) into the .oisi as `/rig_params` +
    // `/experiment_params` JSON attributes. serde is the single source of the
    // schema, so adding a config field appears automatically in the provenance.
    // (This is the canonical, schema-bearing form; readers navigate by key.)
    {
        let rig_str = serde_json::to_string_pretty(&snapshot.rig)
            .map_err(|e| fs_err(format!("Failed to serialize rig params: {e}")))?;
        isi_analysis::io::write_str_attr(&file, name::RIG_PARAMS, &rig_str)?;

        let exp_str = serde_json::to_string_pretty(&snapshot.experiment)
            .map_err(|e| fs_err(format!("Failed to serialize experiment params: {e}")))?;
        isi_analysis::io::write_str_attr(&file, name::EXPERIMENT_PARAMS, &exp_str)?;
    }

    // Write hardware snapshot.
    if let Some(hw) = hardware {
        write_hardware_group(&file, hw, snapshot)?;
    }

    // Write session metadata (animal ID, notes).
    if let Some(meta) = session_meta {
        if !meta.animal_id.is_empty() {
            isi_analysis::io::write_str_attr(&file, name::ANIMAL_ID, &meta.animal_id)?;
        }
        if !meta.notes.is_empty() {
            isi_analysis::io::write_str_attr(&file, name::NOTES, &meta.notes)?;
        }
    }

    // Write anatomical image if available.
    if let Some(anat) = anatomical {
        file.new_dataset_builder()
            .with_data(anat)
            .create(name::ANATOMICAL)
            .map_err(|e| hdf5_err("Failed to write anatomical", e))?;
    }

    // Create acquisition group.
    let acq_group = file
        .create_group(name::ACQUISITION)
        .map_err(|e| hdf5_err("Failed to create acquisition group", e))?;

    // ── Compute unified timeline ─────────────────────────────────
    // t=0 is the first camera frame's system (QPC) timestamp.
    // All timestamps converted to seconds from t=0 as f64.
    // Camera system timestamps and stimulus QPC timestamps are in the same clock domain.

    let t0_us: i64 = camera_data
        .system_timestamps_us
        .first()
        .copied()
        .unwrap_or(0);

    // Camera timestamps in unified seconds.
    let camera_sec: Vec<f64> = camera_data
        .system_timestamps_us
        .iter()
        .map(|&ts| (ts - t0_us) as f64 / 1_000_000.0)
        .collect();

    // Stimulus timestamps in unified seconds (same QPC clock as camera system timestamps).
    let stimulus_sec: Vec<f64> = stimulus_dataset
        .timestamps_us
        .iter()
        .map(|&ts| (ts - t0_us) as f64 / 1_000_000.0)
        .collect();

    // Sweep schedule in unified seconds.
    let sweep_start_sec: Vec<f64> = schedule
        .sweep_start_us
        .iter()
        .map(|&ts| (ts - t0_us) as f64 / 1_000_000.0)
        .collect();
    let sweep_end_sec: Vec<f64> = schedule
        .sweep_end_us
        .iter()
        .map(|&ts| (ts - t0_us) as f64 / 1_000_000.0)
        .collect();

    // Clock synchronization: offset between camera hardware clock and system clock.
    // offset = system_us - hardware_us. Computed at first and last frame for drift detection.
    let clock_sync = if camera_data.system_timestamps_us.len() >= 2
        && camera_data.hardware_timestamps_us.len() >= 2
    {
        let start_offset =
            camera_data.system_timestamps_us[0] - camera_data.hardware_timestamps_us[0];
        let n = camera_data.system_timestamps_us.len();
        let end_offset =
            camera_data.system_timestamps_us[n - 1] - camera_data.hardware_timestamps_us[n - 1];
        Some((start_offset, end_offset))
    } else {
        None
    };

    // ── Write per-frame stimulus arrays + unified timestamps ─────
    write_stimulus_arrays(&acq_group, stimulus_dataset)?;

    // Write unified stimulus timestamps.
    let stim_group = acq_group
        .group(name::STIMULUS)
        .map_err(|e| hdf5_err("Failed to open stimulus group", e))?;
    isi_analysis::io::write_checked_1d(&stim_group, name::TIMESTAMPS_SEC, stimulus_sec)?;

    // Write realized sweep schedule (unified seconds).
    write_sweep_schedule_sec(&acq_group, schedule, &sweep_start_sec, &sweep_end_sec)?;

    // Write quality metrics (before camera data writing, which consumes vecs).
    write_quality_metrics(
        &acq_group,
        &camera_data,
        stimulus_dataset,
        acquisition_complete,
        stimulus_timing_validatable,
    )?;

    // ── Write clock sync ─────────────────────────────────────────
    let sync_group = acq_group
        .create_group(name::CLOCK_SYNC)
        .map_err(|e| hdf5_err("Failed to create clock_sync group", e))?;
    isi_analysis::io::write_f64_attr(&sync_group, name::T0_SYSTEM_US, t0_us as f64)?;
    if let Some((start_off, end_off)) = clock_sync {
        isi_analysis::io::write_f64_attr(&sync_group, name::START_OFFSET_US, start_off as f64)?;
        isi_analysis::io::write_f64_attr(&sync_group, name::END_OFFSET_US, end_off as f64)?;
        isi_analysis::io::write_f64_attr(&sync_group, name::DRIFT_US, (end_off - start_off) as f64)?;
    }

    // ── Write timing characterization ───────────────────────────
    if let Some(tc) = timing {
        let timing_group = acq_group
            .create_group(name::TIMING)
            .map_err(|e| hdf5_err("Failed to create timing group", e))?;
        isi_analysis::io::write_f64_attr(&timing_group, name::F_CAM_HZ, tc.f_cam_hz)?;
        isi_analysis::io::write_f64_attr(&timing_group, name::F_STIM_HZ, tc.f_stim_hz)?;
        isi_analysis::io::write_f64_attr(&timing_group, name::T_CAM_SEC, tc.t_cam_sec)?;
        isi_analysis::io::write_f64_attr(&timing_group, name::T_STIM_SEC, tc.t_stim_sec)?;
        isi_analysis::io::write_f64_attr(&timing_group, name::RATE_RATIO, tc.rate_ratio)?;
        isi_analysis::io::write_f64_attr(&timing_group, name::BEAT_PERIOD_SEC, tc.beat_period_sec)?;
        isi_analysis::io::write_f64_attr(&timing_group, name::PHASE_INCREMENT, tc.phase_increment)?;
        isi_analysis::io::write_str_attr(&timing_group, name::REGIME, &tc.regime.to_string())?;
        isi_analysis::io::write_f64_attr(
            &timing_group,
            name::EXPECTED_PHASE_SAMPLES,
            tc.expected_phase_samples,
        )?;
        isi_analysis::io::write_f64_attr(&timing_group, name::PHASE_COVERAGE, tc.phase_coverage)?;
        isi_analysis::io::write_f64_attr(
            &timing_group,
            name::ONSET_UNCERTAINTY_SEC,
            tc.onset_uncertainty_sec,
        )?;
        isi_analysis::io::write_f64_attr(
            &timing_group,
            name::ONSET_UNCERTAINTY_FRACTION,
            tc.onset_uncertainty_fraction,
        )?;
        isi_analysis::io::write_u32_attr(&timing_group, name::CAM_SAMPLE_COUNT, tc.cam_sample_count)?;
        isi_analysis::io::write_u32_attr(&timing_group, name::STIM_SAMPLE_COUNT, tc.stim_sample_count)?;
        isi_analysis::io::write_f64_attr(&timing_group, name::CAM_JITTER_SEC, tc.cam_jitter_sec)?;
        isi_analysis::io::write_f64_attr(&timing_group, name::STIM_JITTER_SEC, tc.stim_jitter_sec)?;
        if !tc.warnings.is_empty() {
            let warnings_json = serde_json::to_string(&tc.warnings)
                .map_err(|e| fs_err(format!("Failed to serialize timing warnings: {e}")))?;
            isi_analysis::io::write_str_attr(&timing_group, name::WARNINGS, &warnings_json)?;
        }
    }

    // ── Write camera data ────────────────────────────────────────
    let camera_group = acq_group
        .create_group(name::CAMERA)
        .map_err(|e| hdf5_err("Failed to create camera group", e))?;

    let n_frames = camera_data.frames.len();
    let h = camera_data.height as usize;
    let w = camera_data.width as usize;

    if n_frames > 0 {
        // Pack u16 frames into (T, H, W) array — raw, no conversion.
        let mut frame_data = vec![0u16; n_frames * h * w];
        for (t, pixels) in camera_data.frames.iter().enumerate() {
            let src_len = pixels.len().min(h * w);
            let offset = t * h * w;
            frame_data[offset..offset + src_len].copy_from_slice(&pixels[..src_len]);
        }

        camera_group
            .new_dataset_builder()
            .deflate(4)
            .fletcher32()
            .chunk((1, h, w))
            .with_data(
                &ndarray::Array3::from_shape_vec((n_frames, h, w), frame_data)
                    .map_err(|e| AppError::Analysis(isi_analysis::AnalysisError::Compute(format!("Shape error: {e}"))))?,
            )
            .create(name::FRAMES)
            .map_err(|e| hdf5_err("Failed to write camera/frames", e))?;

        // Unified camera timestamps (seconds from t=0).
        isi_analysis::io::write_checked_1d(&camera_group, name::TIMESTAMPS_SEC, camera_sec)?;

        // Raw hardware timestamps (provenance — camera's internal clock).
        isi_analysis::io::write_checked_1d(
            &camera_group,
            name::HARDWARE_TIMESTAMPS_US,
            camera_data.hardware_timestamps_us,
        )?;
        // Raw system timestamps (provenance — QPC at frame read time).
        isi_analysis::io::write_checked_1d(
            &camera_group,
            name::SYSTEM_TIMESTAMPS_US,
            camera_data.system_timestamps_us,
        )?;

        let seq_i64: Vec<i64> = camera_data
            .sequence_numbers
            .iter()
            .map(|&s| s as i64)
            .collect();
        isi_analysis::io::write_checked_1d(&camera_group, name::SEQUENCE_NUMBERS, seq_i64)?;
    }

    // Flush + close the HDF5 file before renaming. HDF5's default weak-
    // close semantics keep the file open as long as any subobject
    // (group/dataset/attribute) is alive, so `drop(file)` alone is a
    // no-op for the OS file handle while these group bindings are still
    // in scope. Drop them explicitly first; on Windows the rename below
    // fails with "file in use" otherwise. Any new long-lived
    // group/dataset binding added in this function needs to be dropped
    // here too.
    drop(camera_group);
    drop(sync_group);
    drop(stim_group);
    drop(acq_group);
    file.flush()
        .map_err(|e| fs_err(format!("Failed to flush .oisi.partial: {e}")))?;
    drop(file);

    // Atomic rename: partial → final. POSIX `rename(2)` is atomic;
    // Windows uses ReplaceFile semantics via std::fs::rename. After
    // this point, the canonical `.oisi` exists at `path`. Failures
    // before this point leave `*.partial` in place for forensics.
    std::fs::rename(&partial_path, path)
        .map_err(|e| fs_err(format!("Failed to rename .oisi.partial to .oisi: {e}")))?;

    let summary = format!(
        "Wrote {} camera frames ({}x{}, u16) to {}",
        n_frames,
        w,
        h,
        path.display()
    );
    tracing::info!("{summary}");
    Ok(summary)
}

/// Write hardware snapshot as `/hardware` group with scalar attributes.
fn write_hardware_group(
    file: &hdf5::File,
    hw: &HardwareSnapshot,
    snapshot: &openisi_params::config::ConfigSnapshot,
) -> AppResult<()> {
    let group = file
        .create_group(name::HARDWARE)
        .map_err(|e| hdf5_err("Failed to create hardware group", e))?;

    isi_analysis::io::write_str_attr(&group, name::MONITOR_NAME, &hw.monitor_name)?;
    isi_analysis::io::write_u32_attr(&group, name::MONITOR_WIDTH_PX, hw.monitor_width_px)?;
    isi_analysis::io::write_u32_attr(&group, name::MONITOR_HEIGHT_PX, hw.monitor_height_px)?;
    isi_analysis::io::write_f64_attr(&group, name::MONITOR_WIDTH_CM, hw.monitor_width_cm)?;
    isi_analysis::io::write_f64_attr(&group, name::MONITOR_HEIGHT_CM, hw.monitor_height_cm)?;
    isi_analysis::io::write_f64_attr(&group, name::MONITOR_REFRESH_HZ, hw.monitor_refresh_hz)?;
    isi_analysis::io::write_f64_attr(&group, name::MEASURED_REFRESH_HZ, hw.measured_refresh_hz)?;
    isi_analysis::io::write_str_attr(&group, name::CAMERA_MODEL, &hw.camera_model)?;
    isi_analysis::io::write_u32_attr(&group, name::CAMERA_WIDTH_PX, hw.camera_width_px)?;
    isi_analysis::io::write_u32_attr(&group, name::CAMERA_HEIGHT_PX, hw.camera_height_px)?;

    // Gamma correction flag.
    let gamma_val: u8 = if hw.gamma_corrected { 1 } else { 0 };
    let attr = group
        .new_attr::<u8>()
        .create(name::GAMMA_CORRECTED)
        .map_err(|e| hdf5_err("creating gamma_corrected attr", e))?;
    attr.write_scalar(&gamma_val)
        .map_err(|e| hdf5_err("writing gamma_corrected attr", e))?;

    // Rig geometry — viewing distance for stimulus geometry reproduction.
    isi_analysis::io::write_f64_attr(
        &group,
        name::VIEWING_DISTANCE_CM,
        snapshot.rig.geometry.viewing_distance_cm,
    )?;

    // Camera acquisition config — exposure and binning at acquisition time.
    isi_analysis::io::write_u32_attr(&group, name::CAMERA_EXPOSURE_US, snapshot.rig.camera.exposure_us)?;
    let binning_val = snapshot.rig.camera.binning;
    let attr = group
        .new_attr::<u16>()
        .create(name::CAMERA_BINNING)
        .map_err(|e| hdf5_err("creating camera_binning attr", e))?;
    attr.write_scalar(&binning_val)
        .map_err(|e| hdf5_err("writing camera_binning attr", e))?;

    // Display settings — rotation and target FPS at acquisition time.
    isi_analysis::io::write_f64_attr(
        &group,
        name::MONITOR_ROTATION_DEG,
        snapshot.rig.display.monitor_rotation_deg,
    )?;
    isi_analysis::io::write_u32_attr(
        &group,
        name::TARGET_STIMULUS_FPS,
        snapshot.rig.display.target_stimulus_fps,
    )?;

    Ok(())
}

/// Write per-frame stimulus arrays under `/acquisition/stimulus/`.
fn write_stimulus_arrays(acq_group: &hdf5::Group, dataset: &StimulusDataset) -> AppResult<()> {
    let stim_group = acq_group
        .create_group(name::STIMULUS)
        .map_err(|e| hdf5_err("Failed to create acquisition/stimulus group", e))?;

    isi_analysis::io::write_checked_1d(&stim_group, name::TIMESTAMPS_US, dataset.timestamps_us.clone())?;
    isi_analysis::io::write_checked_1d(&stim_group, name::STATE_IDS, dataset.state_ids.clone())?;
    isi_analysis::io::write_checked_1d(
        &stim_group,
        name::CONDITION_INDICES,
        dataset.condition_indices.clone(),
    )?;
    isi_analysis::io::write_checked_1d(&stim_group, name::SWEEP_INDICES, dataset.sweep_indices.clone())?;
    isi_analysis::io::write_checked_1d(&stim_group, name::PROGRESS, dataset.progress.clone())?;
    isi_analysis::io::write_checked_1d(
        &stim_group,
        name::FRAME_DELTAS_US,
        dataset.frame_deltas_us.clone(),
    )?;
    isi_analysis::io::write_checked_1d(
        &stim_group,
        name::DROPPED_FRAME_INDICES,
        dataset.dropped_frame_indices.clone(),
    )?;

    Ok(())
}

/// Write `/acquisition/schedule/` group with the realized sweep schedule.
/// Includes both raw microsecond timestamps and unified seconds from t=0.
fn write_sweep_schedule_sec(
    acq_group: &hdf5::Group,
    schedule: &SweepSchedule,
    sweep_start_sec: &[f64],
    sweep_end_sec: &[f64],
) -> AppResult<()> {
    let sched_group = acq_group
        .create_group(name::SCHEDULE)
        .map_err(|e| hdf5_err("Failed to create schedule group", e))?;

    // Sweep sequence as JSON array attribute (HDF5 doesn't have native string arrays easily).
    let seq_json = serde_json::to_string(&schedule.sweep_sequence)
        .map_err(|e| fs_err(format!("Failed to serialize sweep_sequence: {e}")))?;
    let attr = sched_group
        .new_attr::<hdf5::types::VarLenUnicode>()
        .create(name::SWEEP_SEQUENCE)
        .map_err(|e| hdf5_err("creating sweep_sequence attr", e))?;
    let val: hdf5::types::VarLenUnicode = seq_json
        .parse()
        .map_err(|e| AppError::Analysis(isi_analysis::AnalysisError::Validation(format!("Failed to create HDF5 unicode value: {e}"))))?;
    attr.write_scalar(&val)
        .map_err(|e| hdf5_err("writing sweep_sequence attr", e))?;

    // Raw microsecond timestamps (provenance).
    isi_analysis::io::write_checked_1d(
        &sched_group,
        name::SWEEP_START_US,
        schedule.sweep_start_us.clone(),
    )?;
    isi_analysis::io::write_checked_1d(&sched_group, name::SWEEP_END_US, schedule.sweep_end_us.clone())?;

    // Unified seconds from t=0.
    isi_analysis::io::write_checked_1d(&sched_group, name::SWEEP_START_SEC, sweep_start_sec.to_vec())?;
    isi_analysis::io::write_checked_1d(&sched_group, name::SWEEP_END_SEC, sweep_end_sec.to_vec())?;

    Ok(())
}

/// Write `/acquisition/quality/` group with timing quality metrics.
fn write_quality_metrics(
    acq_group: &hdf5::Group,
    camera_data: &AccumulatedData,
    stimulus_dataset: &StimulusDataset,
    acquisition_complete: bool,
    stimulus_timing_validatable: bool,
) -> AppResult<()> {
    let quality = acq_group
        .create_group(name::QUALITY)
        .map_err(|e| hdf5_err("Failed to create quality group", e))?;

    // Camera frame deltas (computed from hardware timestamps).
    let cam_ts = &camera_data.hardware_timestamps_us;
    let cam_deltas: Vec<i64> = cam_ts.windows(2).map(|w| w[1] - w[0]).collect();
    isi_analysis::io::write_checked_1d(&quality, name::CAMERA_FRAME_DELTAS_US, cam_deltas)?;

    // Camera sequence number gaps (indices where sequence is non-consecutive).
    let seq = &camera_data.sequence_numbers;
    let cam_seq_gaps: Vec<u32> = seq
        .windows(2)
        .enumerate()
        .filter(|(_, w)| w[1] != w[0] + 1)
        .map(|(i, _)| (i + 1) as u32)
        .collect();
    isi_analysis::io::write_checked_1d(&quality, name::CAMERA_SEQUENCE_GAPS, cam_seq_gaps.clone())?;

    // Stimulus frame deltas and drops.
    isi_analysis::io::write_checked_1d(
        &quality,
        name::STIMULUS_FRAME_DELTAS_US,
        stimulus_dataset.frame_deltas_us.clone(),
    )?;
    isi_analysis::io::write_checked_1d(
        &quality,
        name::STIMULUS_DROPPED_INDICES,
        stimulus_dataset.dropped_frame_indices.clone(),
    )?;

    // Mean pixel intensity per camera frame (reveals illumination drift).
    let mean_intensities: Vec<f32> = camera_data
        .frames
        .iter()
        .map(|pixels| {
            if pixels.is_empty() {
                return 0.0;
            }
            let sum: u64 = pixels.iter().map(|&p| p as u64).sum();
            sum as f32 / pixels.len() as f32
        })
        .collect();
    isi_analysis::io::write_checked_1d(&quality, name::MEAN_FRAME_INTENSITY, mean_intensities)?;

    // Summary attributes.
    let cam_drops = cam_seq_gaps.len() as u32;
    let stim_drops = stimulus_dataset.dropped_frame_indices.len() as u32;

    isi_analysis::io::write_u32_attr(&quality, name::CAMERA_DROPS_TOTAL, cam_drops)?;
    isi_analysis::io::write_u32_attr(&quality, name::STIMULUS_DROPS_TOTAL, stim_drops)?;

    // Provenance: was the stimulus presented on a real hardware scanout? On a
    // remote (RDP) virtual display there is no hardware vblank, so `stim_drops`
    // above is NOT a physically meaningful measurement. Record this so the count
    // is never later mistaken for a real defect, and so analysis/QA can require
    // a validatable run before trusting stimulus timing.
    let stim_timing_flag: u8 = if stimulus_timing_validatable { 1 } else { 0 };
    isi_analysis::io::write_u32_attr(&quality, name::STIMULUS_TIMING_VALIDATABLE, stim_timing_flag as u32)?;
    isi_analysis::io::write_str_attr(
        &quality,
        name::DISPLAY_SCANOUT,
        if stimulus_timing_validatable {
            "physical"
        } else {
            "remote_virtual"
        },
    )?;

    // Acquisition completeness flag.
    let complete_val: u8 = if acquisition_complete { 1 } else { 0 };
    let attr = quality
        .new_attr::<u8>()
        .create(name::ACQUISITION_COMPLETE)
        .map_err(|e| hdf5_err("creating acquisition_complete attr", e))?;
    attr.write_scalar(&complete_val)
        .map_err(|e| hdf5_err("writing acquisition_complete attr", e))?;

    // Camera drops are always real (the camera has its own hardware clock).
    // Stimulus drops are only a real defect on a hardware scanout; on a remote
    // virtual display they reflect the absence of vsync, not lost frames — so we
    // report them at INFO with the caveat rather than warning.
    if cam_drops > 0 {
        tracing::warn!(camera_gaps = cam_drops, "quality: camera frames dropped");
    }
    if stim_drops > 0 {
        if stimulus_timing_validatable {
            tracing::warn!(stimulus_drops = stim_drops, "quality: stimulus frames dropped");
        } else {
            tracing::info!(
                stimulus_drops = stim_drops,
                "quality: stimulus present-timing not validatable on a remote virtual display \
                 (no hardware vsync) — this count is not a real drop measurement; validate at \
                 the physical console",
            );
        }
    }

    Ok(())
}

/// Public wrapper for encode_16bit_to_png used by commands.rs.
pub fn encode_16bit_to_png_pub(pixels: &[u16], width: u32, height: u32) -> Option<Vec<u8>> {
    encode_16bit_to_png(pixels, width, height)
}

/// Encode 16-bit grayscale pixels to 8-bit PNG for UI preview.
fn encode_16bit_to_png(pixels: &[u16], width: u32, height: u32) -> Option<Vec<u8>> {
    let expected = (width * height) as usize;
    if pixels.len() < expected {
        return None;
    }

    // Find min/max for auto-contrast
    let mut min_val = u16::MAX;
    let mut max_val = 0u16;
    for &p in &pixels[..expected] {
        min_val = min_val.min(p);
        max_val = max_val.max(p);
    }
    let range = (max_val - min_val).max(1) as f64;

    // Convert to 8-bit with auto-contrast
    let bytes: Vec<u8> = pixels[..expected]
        .iter()
        .map(|&p| ((p - min_val) as f64 / range * 255.0) as u8)
        .collect();

    let mut png_data = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png_data, width, height);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(&bytes).ok()?;
    }
    Some(png_data)
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use openisi_stimulus::dataset::{DatasetConfig, EnvelopeType};
    use openisi_stimulus::geometry::{DisplayGeometry, ProjectionType};
    use openisi_stimulus::sequencer::Order;

    fn test_stimulus_dataset() -> StimulusDataset {
        let config = DatasetConfig {
            envelope: EnvelopeType::Bar,
            stimulus_params: HashMap::new(),
            conditions: vec!["LR".into(), "RL".into()],
            repetitions: 1,
            order: Order::Sequential,
            baseline_start_sec: 1.0,
            baseline_end_sec: 1.0,
            inter_stimulus_sec: 0.5,
            inter_direction_sec: 0.5,
            sweep_duration_sec: 3.0,
            geometry: DisplayGeometry::new(
                ProjectionType::Cartesian,
                25.0,
                0.0,
                0.0,
                0.0,
                0.0,
                53.0,
                30.0,
                1920,
                1080,
            ),
            display_physical_source: "test".into(),
            reported_refresh_hz: 60.0,
            measured_refresh_hz: 59.94,
            target_stimulus_fps: 0,
            drop_detection_warmup_frames: 10,
            drop_detection_threshold: 1.5,
            fps_window_frames: 10,
        };
        StimulusDataset::new(config)
    }

    #[test]
    fn new_accumulator_is_inactive() {
        let acc = AcquisitionAccumulator::new();
        assert!(!acc.is_active());
        assert_eq!(acc.width, 0);
        assert_eq!(acc.height, 0);
    }

    #[test]
    fn start_finish_lifecycle() {
        let mut acc = AcquisitionAccumulator::new();
        acc.start(64, 64);
        assert!(acc.is_active());

        let data = acc.finish();
        assert_eq!(data.frame_count(), 0);
    }

    #[test]
    fn all_frames_stored_including_baseline() {
        let mut acc = AcquisitionAccumulator::new();
        acc.start(4, 4);

        // Add frames — no cycle filtering, all stored
        acc.add_frame(vec![100u16; 16], 1000, 2000, 1);
        acc.add_frame(vec![200u16; 16], 1100, 2100, 2);
        acc.add_frame(vec![300u16; 16], 1200, 2200, 3);

        let data = acc.finish();
        assert_eq!(data.frame_count(), 3);
        assert_eq!(data.hardware_timestamps_us, vec![1000, 1100, 1200]);
        assert_eq!(data.system_timestamps_us, vec![2000, 2100, 2200]);
        assert_eq!(data.sequence_numbers, vec![1, 2, 3]);
    }

    #[test]
    fn add_frame_when_inactive_is_discarded() {
        let mut acc = AcquisitionAccumulator::new();
        acc.add_frame(vec![0u16; 16], 1000, 2000, 1);

        acc.start(4, 4);
        let data = acc.finish();
        assert_eq!(data.frame_count(), 0);
    }

    #[test]
    fn stats_reports_correct_count() {
        let mut acc = AcquisitionAccumulator::new();
        acc.start(2, 2);
        acc.add_frame(vec![0u16; 4], 100, 200, 1);
        acc.add_frame(vec![0u16; 4], 200, 300, 2);

        let stats = acc.stats();
        assert!(stats.contains("2 frames"), "stats: {stats}");
    }

    #[test]
    fn write_oisi_creates_valid_file() {
        let tmp = std::env::temp_dir().join("openisi_test_write_oisi.oisi");
        let _ = std::fs::remove_file(&tmp);

        let mut ds = test_stimulus_dataset();
        ds.start_recording();

        // Use small frames to keep test fast. The write_oisi function is the same
        // regardless of frame size.
        let data = AccumulatedData {
            frames: vec![vec![42u16; 8 * 8]; 3],
            hardware_timestamps_us: vec![1000, 2000, 3000],
            system_timestamps_us: vec![1500, 2500, 3500],
            sequence_numbers: vec![1, 2, 3],
            width: 8,
            height: 8,
        };

        // Empty schedule — no sweeps occurred in this minimal test dataset.
        let schedule = SweepSchedule {
            sweep_sequence: Vec::new(),
            sweep_start_us: Vec::new(),
            sweep_end_us: Vec::new(),
        };
        // Create a default typed config snapshot for the test.
        let snapshot = openisi_params::config::ConfigStore::new(
            std::path::Path::new("."),
            std::path::Path::new("."),
        )
        .snapshot();
        let result = write_oisi(
            &tmp,
            OisiBundle {
                stimulus_dataset: &ds,
                camera_data: data,
                snapshot: &snapshot,
                hardware: None,
                schedule: &schedule,
                timing: None,
                session_meta: None,
                anatomical: None,
                acquisition_complete: true,
                stimulus_timing_validatable: true,
            },
        );
        assert!(result.is_ok(), "write_oisi failed: {:?}", result.err());

        let file = hdf5::File::open(&tmp).expect("Should open .oisi file");
        assert!(file.group("acquisition").is_ok());
        assert!(file.group("acquisition/camera").is_ok());
        assert!(file.dataset("acquisition/camera/frames").is_ok());
        assert!(
            file.dataset("acquisition/camera/hardware_timestamps_us")
                .is_ok()
        );
        assert!(
            file.dataset("acquisition/camera/system_timestamps_us")
                .is_ok()
        );
        assert!(file.dataset("acquisition/camera/sequence_numbers").is_ok());

        // Unified timestamps (seconds from t=0).
        assert!(
            file.dataset("acquisition/camera/timestamps_sec").is_ok(),
            "camera/timestamps_sec missing"
        );
        assert!(
            file.dataset("acquisition/stimulus/timestamps_sec").is_ok(),
            "stimulus/timestamps_sec missing"
        );

        // Clock sync group.
        assert!(
            file.group("acquisition/clock_sync").is_ok(),
            "clock_sync group missing"
        );

        // Schedule group.
        assert!(
            file.group("acquisition/schedule").is_ok(),
            "schedule group missing"
        );

        // Stimulus-timing provenance: this fixture passed `true`, so the file
        // must record the scanout as physical / timing validatable. This guards
        // the honesty contract — a remote (RDP) run records "remote_virtual" so
        // the stimulus-drop count is never later mistaken for a real defect.
        let quality = file
            .group("acquisition/quality")
            .expect("quality group missing");
        let timing_flag: u32 = quality
            .attr("stimulus_timing_validatable")
            .expect("stimulus_timing_validatable attr missing")
            .read_scalar()
            .expect("read stimulus_timing_validatable");
        assert_eq!(timing_flag, 1, "fixture is a physical scanout");
        let scanout: hdf5::types::VarLenUnicode = quality
            .attr("display_scanout")
            .expect("display_scanout attr missing")
            .read_scalar()
            .expect("read display_scanout");
        assert_eq!(scanout.as_str(), "physical");

        // Verify unified camera timestamps are correct (seconds from t=0).
        let cam_sec: Vec<f64> = file
            .dataset("acquisition/camera/timestamps_sec")
            .unwrap()
            .read_1d()
            .unwrap()
            .to_vec();
        assert_eq!(cam_sec.len(), 3);
        assert!(
            (cam_sec[0] - 0.0).abs() < 1e-10,
            "first camera timestamp should be 0.0"
        );
        assert!(
            (cam_sec[1] - 0.001).abs() < 1e-10,
            "second camera timestamp should be 0.001s"
        );
        assert!(
            (cam_sec[2] - 0.002).abs() < 1e-10,
            "third camera timestamp should be 0.002s"
        );

        let frames_ds = file.dataset("acquisition/camera/frames").unwrap();
        let shape = frames_ds.shape();
        assert_eq!(shape, vec![3, 8, 8]);

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn write_stimulus_arrays_creates_datasets() {
        let tmp = std::env::temp_dir().join("openisi_test_stim_arrays.oisi");
        let _ = std::fs::remove_file(&tmp);

        let file = hdf5::File::create(&tmp).expect("create test file");
        let acq = file.create_group(name::ACQUISITION).expect("create acq group");

        let ds = test_stimulus_dataset();
        write_stimulus_arrays(&acq, &ds).expect("write stimulus arrays");

        assert!(acq.group("stimulus").is_ok());
        assert!(acq.dataset("stimulus/timestamps_us").is_ok());
        assert!(acq.dataset("stimulus/state_ids").is_ok());
        assert!(acq.dataset("stimulus/condition_indices").is_ok());
        assert!(acq.dataset("stimulus/sweep_indices").is_ok());
        assert!(acq.dataset("stimulus/progress").is_ok());
        assert!(acq.dataset("stimulus/frame_deltas_us").is_ok());
        assert!(acq.dataset("stimulus/dropped_frame_indices").is_ok());

        let _ = std::fs::remove_file(&tmp);
    }

    /// A minimal valid `write_oisi` call — shared by the atomic-write integrity
    /// tests below, which care about the file lifecycle, not the payload.
    fn minimal_write_oisi(path: &std::path::Path) -> AppResult<String> {
        let mut ds = test_stimulus_dataset();
        ds.start_recording();
        let data = AccumulatedData {
            frames: vec![vec![7u16; 4 * 4]; 2],
            hardware_timestamps_us: vec![1000, 2000],
            system_timestamps_us: vec![1500, 2500],
            sequence_numbers: vec![1, 2],
            width: 4,
            height: 4,
        };
        let schedule = SweepSchedule {
            sweep_sequence: Vec::new(),
            sweep_start_us: Vec::new(),
            sweep_end_us: Vec::new(),
        };
        let snapshot = openisi_params::config::ConfigStore::new(
            std::path::Path::new("."),
            std::path::Path::new("."),
        )
        .snapshot();
        write_oisi(
            path,
            OisiBundle {
                stimulus_dataset: &ds,
                camera_data: data,
                snapshot: &snapshot,
                hardware: None,
                schedule: &schedule,
                timing: None,
                session_meta: None,
                anatomical: None,
                acquisition_complete: true,
                stimulus_timing_validatable: true,
            },
        )
    }

    /// Atomic-write contract (see the protocol comment at the top of
    /// `write_oisi`): a SUCCESSFUL write leaves the canonical `.oisi` and the
    /// `.partial` is consumed by the rename — no leftover temp file is ever
    /// observed next to a good file.
    #[test]
    fn write_oisi_success_leaves_no_partial() {
        let tmp = std::env::temp_dir().join("openisi_integrity_success.oisi");
        let partial = std::env::temp_dir().join("openisi_integrity_success.oisi.partial");
        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::remove_file(&partial);

        minimal_write_oisi(&tmp).expect("write should succeed");

        assert!(tmp.exists(), "canonical .oisi must exist after success");
        assert!(
            !partial.exists(),
            "the .partial must be consumed by the atomic rename, not left behind"
        );

        let _ = std::fs::remove_file(&tmp);
    }

    /// Atomic-write contract: a FAILED write must NEVER produce a canonical
    /// `.oisi` at `path`. Here the parent directory does not exist, so file
    /// creation fails. A reader that sees `<name>.oisi` can therefore trust it
    /// is a complete file — an interrupted acquisition surfaces as a *missing*
    /// `.oisi` (plus a forensic `.partial` when one got far enough to exist),
    /// never as a half-written canonical file.
    #[test]
    fn write_oisi_failure_produces_no_canonical_file() {
        let missing_dir = std::env::temp_dir().join("openisi_integrity_no_such_dir");
        let _ = std::fs::remove_dir_all(&missing_dir);
        let target = missing_dir.join("acq.oisi");

        let result = minimal_write_oisi(&target);

        assert!(result.is_err(), "write into a missing dir must fail");
        assert!(
            !target.exists(),
            "no canonical .oisi may exist after a failed write"
        );
    }
}
