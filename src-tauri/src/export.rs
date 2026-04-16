//! .oisi file export — writes acquisition data to HDF5.
//!
//! During acquisition, ALL camera frames are accumulated in order (including
//! baselines and inter-trial periods). Each frame carries both the camera
//! hardware timestamp and the system QPC timestamp for clock synchronization.
//! When the stimulus thread signals completion, the data is written to .oisi.

use std::path::Path;

use openisi_stimulus::dataset::StimulusDataset;
use serde::Serialize;

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

use crate::params::RegistrySnapshot;

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
        format!("{} frames, {}x{}", self.frames.len(), self.width, self.height)
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

pub fn write_oisi(
    path: &Path,
    stimulus_dataset: &StimulusDataset,
    camera_data: AccumulatedData,
    snapshot: &RegistrySnapshot,
    hardware: Option<&HardwareSnapshot>,
    schedule: &SweepSchedule,
    timing: Option<&crate::timing::TimingCharacterization>,
    session_meta: Option<&SessionMetadata>,
    anatomical: Option<&ndarray::Array2<u8>>,
    acquisition_complete: bool,
) -> Result<String, String> {
    use isi_analysis::io;

    // Create the file with metadata.
    io::create(path, "raw_acquisition")
        .map_err(|e| format!("Failed to create .oisi: {e}"))?;

    // Open for writing.
    let file = hdf5::File::open_rw(path)
        .map_err(|e| format!("Failed to open .oisi for writing: {e}"))?;

    // Software version for provenance.
    write_str_attr(&file, "software_version", env!("CARGO_PKG_VERSION"))?;

    // Write stimulus metadata.
    let metadata = stimulus_dataset.export_metadata();
    let meta_json = serde_json::to_string_pretty(&metadata)
        .map_err(|e| format!("Failed to serialize metadata: {e}"))?;
    write_str_attr(&file, "stimulus_metadata", &meta_json)?;

    // Write experiment snapshot as JSON (reconstructed from registry snapshot for provenance).
    {
        let exp_json = serde_json::json!({
            "geometry": {
                "horizontal_offset_deg": snapshot.horizontal_offset_deg(),
                "vertical_offset_deg": snapshot.vertical_offset_deg(),
            },
            "stimulus": {
                "envelope": format!("{:?}", snapshot.stimulus_envelope()).to_lowercase(),
                "carrier": format!("{:?}", snapshot.stimulus_carrier()).to_lowercase(),
                "params": {
                    "contrast": snapshot.contrast(),
                    "mean_luminance": snapshot.mean_luminance(),
                    "background_luminance": snapshot.background_luminance(),
                    "check_size_deg": snapshot.check_size_deg(),
                    "check_size_cm": snapshot.check_size_cm(),
                    "strobe_frequency_hz": snapshot.strobe_frequency_hz(),
                    "stimulus_width_deg": snapshot.stimulus_width_deg(),
                    "sweep_speed_deg_per_sec": snapshot.sweep_speed_deg_per_sec(),
                    "rotation_speed_deg_per_sec": snapshot.rotation_speed_deg_per_sec(),
                    "expansion_speed_deg_per_sec": snapshot.expansion_speed_deg_per_sec(),
                    "rotation_deg": snapshot.rotation_deg(),
                }
            },
            "presentation": {
                "conditions": snapshot.conditions(),
                "repetitions": snapshot.repetitions(),
            },
            "timing": {
                "baseline_start_sec": snapshot.baseline_start_sec(),
                "baseline_end_sec": snapshot.baseline_end_sec(),
                "inter_stimulus_sec": snapshot.inter_stimulus_sec(),
                "inter_direction_sec": snapshot.inter_direction_sec(),
            }
        });
        let exp_str = serde_json::to_string_pretty(&exp_json)
            .map_err(|e| format!("Failed to serialize experiment snapshot: {e}"))?;
        write_str_attr(&file, "experiment", &exp_str)?;
    }

    // Write hardware snapshot.
    if let Some(hw) = hardware {
        write_hardware_group(&file, hw, snapshot)?;
    }

    // Write session metadata (animal ID, notes).
    if let Some(meta) = session_meta {
        if !meta.animal_id.is_empty() {
            write_str_attr(&file, "animal_id", &meta.animal_id)?;
        }
        if !meta.notes.is_empty() {
            write_str_attr(&file, "notes", &meta.notes)?;
        }
    }

    // Write anatomical image if available.
    if let Some(anat) = anatomical {
        file.new_dataset_builder()
            .with_data(anat)
            .create("anatomical")
            .map_err(|e| format!("Failed to write anatomical: {e}"))?;
    }

    // Create acquisition group.
    let acq_group = file.create_group("acquisition")
        .map_err(|e| format!("Failed to create acquisition group: {e}"))?;

    // ── Compute unified timeline ─────────────────────────────────
    // t=0 is the first camera frame's system (QPC) timestamp.
    // All timestamps converted to seconds from t=0 as f64.
    // Camera system timestamps and stimulus QPC timestamps are in the same clock domain.

    let t0_us: i64 = camera_data.system_timestamps_us.first().copied().unwrap_or(0);

    // Camera timestamps in unified seconds.
    let camera_sec: Vec<f64> = camera_data.system_timestamps_us.iter()
        .map(|&ts| (ts - t0_us) as f64 / 1_000_000.0)
        .collect();

    // Stimulus timestamps in unified seconds (same QPC clock as camera system timestamps).
    let stimulus_sec: Vec<f64> = stimulus_dataset.timestamps_us.iter()
        .map(|&ts| (ts - t0_us) as f64 / 1_000_000.0)
        .collect();

    // Sweep schedule in unified seconds.
    let sweep_start_sec: Vec<f64> = schedule.sweep_start_us.iter()
        .map(|&ts| (ts - t0_us) as f64 / 1_000_000.0)
        .collect();
    let sweep_end_sec: Vec<f64> = schedule.sweep_end_us.iter()
        .map(|&ts| (ts - t0_us) as f64 / 1_000_000.0)
        .collect();

    // Clock synchronization: offset between camera hardware clock and system clock.
    // offset = system_us - hardware_us. Computed at first and last frame for drift detection.
    let clock_sync = if camera_data.system_timestamps_us.len() >= 2
        && camera_data.hardware_timestamps_us.len() >= 2
    {
        let start_offset = camera_data.system_timestamps_us[0] - camera_data.hardware_timestamps_us[0];
        let n = camera_data.system_timestamps_us.len();
        let end_offset = camera_data.system_timestamps_us[n - 1] - camera_data.hardware_timestamps_us[n - 1];
        Some((start_offset, end_offset))
    } else {
        None
    };

    // ── Write per-frame stimulus arrays + unified timestamps ─────
    write_stimulus_arrays(&acq_group, stimulus_dataset)?;

    // Write unified stimulus timestamps.
    let stim_group = acq_group.group("stimulus")
        .map_err(|e| format!("Failed to open stimulus group: {e}"))?;
    write_checked_1d(&stim_group, "timestamps_sec", stimulus_sec)?;

    // Write realized sweep schedule (unified seconds).
    write_sweep_schedule_sec(&acq_group, schedule, &sweep_start_sec, &sweep_end_sec)?;

    // Write quality metrics (before camera data writing, which consumes vecs).
    write_quality_metrics(&acq_group, &camera_data, stimulus_dataset, acquisition_complete)?;

    // ── Write clock sync ─────────────────────────────────────────
    let sync_group = acq_group.create_group("clock_sync")
        .map_err(|e| format!("Failed to create clock_sync group: {e}"))?;
    write_group_f64_attr(&sync_group, "t0_system_us", t0_us as f64)?;
    if let Some((start_off, end_off)) = clock_sync {
        write_group_f64_attr(&sync_group, "start_offset_us", start_off as f64)?;
        write_group_f64_attr(&sync_group, "end_offset_us", end_off as f64)?;
        write_group_f64_attr(&sync_group, "drift_us", (end_off - start_off) as f64)?;
    }

    // ── Write timing characterization ───────────────────────────
    if let Some(tc) = timing {
        let timing_group = acq_group.create_group("timing")
            .map_err(|e| format!("Failed to create timing group: {e}"))?;
        write_group_f64_attr(&timing_group, "f_cam_hz", tc.f_cam_hz)?;
        write_group_f64_attr(&timing_group, "f_stim_hz", tc.f_stim_hz)?;
        write_group_f64_attr(&timing_group, "t_cam_sec", tc.t_cam_sec)?;
        write_group_f64_attr(&timing_group, "t_stim_sec", tc.t_stim_sec)?;
        write_group_f64_attr(&timing_group, "rate_ratio", tc.rate_ratio)?;
        write_group_f64_attr(&timing_group, "beat_period_sec", tc.beat_period_sec)?;
        write_group_f64_attr(&timing_group, "phase_increment", tc.phase_increment)?;
        write_group_str_attr(&timing_group, "regime", &tc.regime.to_string())?;
        write_group_f64_attr(&timing_group, "expected_phase_samples", tc.expected_phase_samples)?;
        write_group_f64_attr(&timing_group, "phase_coverage", tc.phase_coverage)?;
        write_group_f64_attr(&timing_group, "onset_uncertainty_sec", tc.onset_uncertainty_sec)?;
        write_group_f64_attr(&timing_group, "onset_uncertainty_fraction", tc.onset_uncertainty_fraction)?;
        write_group_u32_attr(&timing_group, "cam_sample_count", tc.cam_sample_count)?;
        write_group_u32_attr(&timing_group, "stim_sample_count", tc.stim_sample_count)?;
        write_group_f64_attr(&timing_group, "cam_jitter_sec", tc.cam_jitter_sec)?;
        write_group_f64_attr(&timing_group, "stim_jitter_sec", tc.stim_jitter_sec)?;
        if !tc.warnings.is_empty() {
            let warnings_json = serde_json::to_string(&tc.warnings)
                .map_err(|e| format!("Failed to serialize timing warnings: {e}"))?;
            write_group_str_attr(&timing_group, "warnings", &warnings_json)?;
        }
    }

    // ── Write camera data ────────────────────────────────────────
    let camera_group = acq_group.create_group("camera")
        .map_err(|e| format!("Failed to create camera group: {e}"))?;

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

        camera_group.new_dataset_builder()
            .deflate(4)
            .fletcher32()
            .chunk((1, h, w))
            .with_data(
                &ndarray::Array3::from_shape_vec((n_frames, h, w), frame_data)
                    .map_err(|e| format!("Shape error: {e}"))?
            )
            .create("frames")
            .map_err(|e| format!("Failed to write camera/frames: {e}"))?;

        // Unified camera timestamps (seconds from t=0).
        write_checked_1d(&camera_group, "timestamps_sec", camera_sec)?;

        // Raw hardware timestamps (provenance — camera's internal clock).
        write_checked_1d(&camera_group, "hardware_timestamps_us", camera_data.hardware_timestamps_us)?;
        // Raw system timestamps (provenance — QPC at frame read time).
        write_checked_1d(&camera_group, "system_timestamps_us", camera_data.system_timestamps_us)?;

        let seq_i64: Vec<i64> = camera_data.sequence_numbers.iter().map(|&s| s as i64).collect();
        write_checked_1d(&camera_group, "sequence_numbers", seq_i64)?;
    }

    let summary = format!(
        "Wrote {} camera frames ({}x{}, u16) to {}",
        n_frames, w, h, path.display()
    );
    eprintln!("[export] {summary}");
    Ok(summary)
}

/// Write hardware snapshot as `/hardware` group with scalar attributes.
fn write_hardware_group(
    file: &hdf5::File,
    hw: &HardwareSnapshot,
    snapshot: &RegistrySnapshot,
) -> Result<(), String> {
    let group = file.create_group("hardware")
        .map_err(|e| format!("Failed to create hardware group: {e}"))?;

    write_group_str_attr(&group, "monitor_name", &hw.monitor_name)?;
    write_group_u32_attr(&group, "monitor_width_px", hw.monitor_width_px)?;
    write_group_u32_attr(&group, "monitor_height_px", hw.monitor_height_px)?;
    write_group_f64_attr(&group, "monitor_width_cm", hw.monitor_width_cm)?;
    write_group_f64_attr(&group, "monitor_height_cm", hw.monitor_height_cm)?;
    write_group_f64_attr(&group, "monitor_refresh_hz", hw.monitor_refresh_hz)?;
    write_group_f64_attr(&group, "measured_refresh_hz", hw.measured_refresh_hz)?;
    write_group_str_attr(&group, "camera_model", &hw.camera_model)?;
    write_group_u32_attr(&group, "camera_width_px", hw.camera_width_px)?;
    write_group_u32_attr(&group, "camera_height_px", hw.camera_height_px)?;

    // Gamma correction flag.
    let gamma_val: u8 = if hw.gamma_corrected { 1 } else { 0 };
    let attr = group.new_attr::<u8>()
        .create("gamma_corrected")
        .map_err(|e| format!("creating gamma_corrected attr: {e}"))?;
    attr.write_scalar(&gamma_val)
        .map_err(|e| format!("writing gamma_corrected attr: {e}"))?;

    // Rig geometry — viewing distance for stimulus geometry reproduction.
    write_group_f64_attr(&group, "viewing_distance_cm", snapshot.viewing_distance_cm())?;

    // Camera acquisition config — exposure and binning at acquisition time.
    write_group_u32_attr(&group, "camera_exposure_us", snapshot.camera_exposure_us())?;
    let binning_val = snapshot.camera_binning();
    let attr = group.new_attr::<u16>()
        .create("camera_binning")
        .map_err(|e| format!("creating camera_binning attr: {e}"))?;
    attr.write_scalar(&binning_val)
        .map_err(|e| format!("writing camera_binning attr: {e}"))?;

    // Display settings — rotation and target FPS at acquisition time.
    write_group_f64_attr(&group, "monitor_rotation_deg", snapshot.monitor_rotation_deg())?;
    write_group_u32_attr(&group, "target_stimulus_fps", snapshot.target_stimulus_fps())?;

    Ok(())
}

/// Write per-frame stimulus arrays under `/acquisition/stimulus/`.
fn write_stimulus_arrays(
    acq_group: &hdf5::Group,
    dataset: &StimulusDataset,
) -> Result<(), String> {
    let stim_group = acq_group.create_group("stimulus")
        .map_err(|e| format!("Failed to create acquisition/stimulus group: {e}"))?;

    write_checked_1d(&stim_group, "timestamps_us", dataset.timestamps_us.clone())?;
    write_checked_1d(&stim_group, "state_ids", dataset.state_ids.clone())?;
    write_checked_1d(&stim_group, "condition_indices", dataset.condition_indices.clone())?;
    write_checked_1d(&stim_group, "sweep_indices", dataset.sweep_indices.clone())?;
    write_checked_1d(&stim_group, "progress", dataset.progress.clone())?;
    write_checked_1d(&stim_group, "frame_deltas_us", dataset.frame_deltas_us.clone())?;
    write_checked_1d(&stim_group, "dropped_frame_indices", dataset.dropped_frame_indices.clone())?;

    Ok(())
}

/// Write `/acquisition/schedule/` group with the realized sweep schedule.
/// Includes both raw microsecond timestamps and unified seconds from t=0.
fn write_sweep_schedule_sec(
    acq_group: &hdf5::Group,
    schedule: &SweepSchedule,
    sweep_start_sec: &[f64],
    sweep_end_sec: &[f64],
) -> Result<(), String> {
    let sched_group = acq_group.create_group("schedule")
        .map_err(|e| format!("Failed to create schedule group: {e}"))?;

    // Sweep sequence as JSON array attribute (HDF5 doesn't have native string arrays easily).
    let seq_json = serde_json::to_string(&schedule.sweep_sequence)
        .map_err(|e| format!("Failed to serialize sweep_sequence: {e}"))?;
    let attr = sched_group.new_attr::<hdf5::types::VarLenUnicode>()
        .create("sweep_sequence")
        .map_err(|e| format!("creating sweep_sequence attr: {e}"))?;
    let val: hdf5::types::VarLenUnicode = seq_json.parse()
        .map_err(|e| format!("Failed to create HDF5 unicode value: {e}"))?;
    attr.write_scalar(&val)
        .map_err(|e| format!("writing sweep_sequence attr: {e}"))?;

    // Raw microsecond timestamps (provenance).
    write_checked_1d(&sched_group, "sweep_start_us", schedule.sweep_start_us.clone())?;
    write_checked_1d(&sched_group, "sweep_end_us", schedule.sweep_end_us.clone())?;

    // Unified seconds from t=0.
    write_checked_1d(&sched_group, "sweep_start_sec", sweep_start_sec.to_vec())?;
    write_checked_1d(&sched_group, "sweep_end_sec", sweep_end_sec.to_vec())?;

    Ok(())
}

/// Write `/acquisition/quality/` group with timing quality metrics.
fn write_quality_metrics(
    acq_group: &hdf5::Group,
    camera_data: &AccumulatedData,
    stimulus_dataset: &StimulusDataset,
    acquisition_complete: bool,
) -> Result<(), String> {
    let quality = acq_group.create_group("quality")
        .map_err(|e| format!("Failed to create quality group: {e}"))?;

    // Camera frame deltas (computed from hardware timestamps).
    let cam_ts = &camera_data.hardware_timestamps_us;
    let cam_deltas: Vec<i64> = cam_ts.windows(2).map(|w| w[1] - w[0]).collect();
    write_checked_1d(&quality, "camera_frame_deltas_us", cam_deltas)?;

    // Camera sequence number gaps (indices where sequence is non-consecutive).
    let seq = &camera_data.sequence_numbers;
    let cam_seq_gaps: Vec<u32> = seq.windows(2)
        .enumerate()
        .filter(|(_, w)| w[1] != w[0] + 1)
        .map(|(i, _)| (i + 1) as u32)
        .collect();
    write_checked_1d(&quality, "camera_sequence_gaps", cam_seq_gaps.clone())?;

    // Stimulus frame deltas and drops.
    write_checked_1d(&quality, "stimulus_frame_deltas_us", stimulus_dataset.frame_deltas_us.clone())?;
    write_checked_1d(&quality, "stimulus_dropped_indices", stimulus_dataset.dropped_frame_indices.clone())?;

    // Mean pixel intensity per camera frame (reveals illumination drift).
    let mean_intensities: Vec<f32> = camera_data.frames.iter().map(|pixels| {
        if pixels.is_empty() {
            return 0.0;
        }
        let sum: u64 = pixels.iter().map(|&p| p as u64).sum();
        sum as f32 / pixels.len() as f32
    }).collect();
    write_checked_1d(&quality, "mean_frame_intensity", mean_intensities)?;

    // Summary attributes.
    let cam_drops = cam_seq_gaps.len() as u32;
    let stim_drops = stimulus_dataset.dropped_frame_indices.len() as u32;

    write_group_u32_attr(&quality, "camera_drops_total", cam_drops)?;
    write_group_u32_attr(&quality, "stimulus_drops_total", stim_drops)?;

    // Acquisition completeness flag.
    let complete_val: u8 = if acquisition_complete { 1 } else { 0 };
    let attr = quality.new_attr::<u8>()
        .create("acquisition_complete")
        .map_err(|e| format!("creating acquisition_complete attr: {e}"))?;
    attr.write_scalar(&complete_val)
        .map_err(|e| format!("writing acquisition_complete attr: {e}"))?;

    if cam_drops > 0 || stim_drops > 0 {
        eprintln!(
            "[export] quality: {} camera sequence gaps, {} stimulus drops",
            cam_drops, stim_drops
        );
    }

    Ok(())
}

fn write_str_attr(file: &hdf5::File, name: &str, value: &str) -> Result<(), String> {
    let attr = file
        .new_attr::<hdf5::types::VarLenUnicode>()
        .create(name)
        .map_err(|e| format!("creating attr {name}: {e}"))?;
    let val: hdf5::types::VarLenUnicode = value.parse()
        .map_err(|e| format!("Failed to create HDF5 unicode value: {e}"))?;
    attr.write_scalar(&val)
        .map_err(|e| format!("writing attr {name}: {e}"))?;
    Ok(())
}

/// Write a 1D array with Fletcher32 checksum. Requires chunking.
fn write_checked_1d<T: hdf5::H5Type + Clone>(
    group: &hdf5::Group,
    name: &str,
    data: Vec<T>,
) -> Result<(), String> {
    if data.is_empty() {
        // Write empty dataset — no chunking needed for empty
        group.new_dataset_builder()
            .with_data(&ndarray::Array1::<T>::from(data))
            .create(name)
            .map_err(|e| format!("Failed to write {name}: {e}"))?;
    } else {
        let len = data.len();
        group.new_dataset_builder()
            .fletcher32()
            .chunk((len,))
            .with_data(&ndarray::Array1::from(data))
            .create(name)
            .map_err(|e| format!("Failed to write {name}: {e}"))?;
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

fn write_group_str_attr(group: &hdf5::Group, name: &str, value: &str) -> Result<(), String> {
    let attr = group
        .new_attr::<hdf5::types::VarLenUnicode>()
        .create(name)
        .map_err(|e| format!("creating group attr {name}: {e}"))?;
    let val: hdf5::types::VarLenUnicode = value.parse()
        .map_err(|e| format!("Failed to create HDF5 unicode value: {e}"))?;
    attr.write_scalar(&val)
        .map_err(|e| format!("writing group attr {name}: {e}"))?;
    Ok(())
}

fn write_group_u32_attr(group: &hdf5::Group, name: &str, value: u32) -> Result<(), String> {
    let attr = group
        .new_attr::<u32>()
        .create(name)
        .map_err(|e| format!("creating group attr {name}: {e}"))?;
    attr.write_scalar(&value)
        .map_err(|e| format!("writing group attr {name}: {e}"))?;
    Ok(())
}

fn write_group_f64_attr(group: &hdf5::Group, name: &str, value: f64) -> Result<(), String> {
    let attr = group
        .new_attr::<f64>()
        .create(name)
        .map_err(|e| format!("creating group attr {name}: {e}"))?;
    attr.write_scalar(&value)
        .map_err(|e| format!("writing group attr {name}: {e}"))?;
    Ok(())
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
                25.0, 0.0, 0.0,
                53.0, 30.0,
                1920, 1080,
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
        // Create a default registry snapshot for the test.
        let snapshot = crate::params::Registry::new(std::path::Path::new(".")).snapshot();
        let result = write_oisi(&tmp, &ds, data, &snapshot, None, &schedule, None, None, None, true);
        assert!(result.is_ok(), "write_oisi failed: {:?}", result.err());

        let file = hdf5::File::open(&tmp).expect("Should open .oisi file");
        assert!(file.group("acquisition").is_ok());
        assert!(file.group("acquisition/camera").is_ok());
        assert!(file.dataset("acquisition/camera/frames").is_ok());
        assert!(file.dataset("acquisition/camera/hardware_timestamps_us").is_ok());
        assert!(file.dataset("acquisition/camera/system_timestamps_us").is_ok());
        assert!(file.dataset("acquisition/camera/sequence_numbers").is_ok());

        // Unified timestamps (seconds from t=0).
        assert!(file.dataset("acquisition/camera/timestamps_sec").is_ok(),
            "camera/timestamps_sec missing");
        assert!(file.dataset("acquisition/stimulus/timestamps_sec").is_ok(),
            "stimulus/timestamps_sec missing");

        // Clock sync group.
        assert!(file.group("acquisition/clock_sync").is_ok(),
            "clock_sync group missing");

        // Schedule group.
        assert!(file.group("acquisition/schedule").is_ok(),
            "schedule group missing");

        // Verify unified camera timestamps are correct (seconds from t=0).
        let cam_sec: Vec<f64> = file.dataset("acquisition/camera/timestamps_sec").unwrap()
            .read_1d().unwrap().to_vec();
        assert_eq!(cam_sec.len(), 3);
        assert!((cam_sec[0] - 0.0).abs() < 1e-10, "first camera timestamp should be 0.0");
        assert!((cam_sec[1] - 0.001).abs() < 1e-10, "second camera timestamp should be 0.001s");
        assert!((cam_sec[2] - 0.002).abs() < 1e-10, "third camera timestamp should be 0.002s");

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
        let acq = file.create_group("acquisition").expect("create acq group");

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
}
