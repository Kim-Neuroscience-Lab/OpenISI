//! .oisi file I/O — single HDF5 file, system introspects what's present.
//!
//! HDF5 layout:
//!
//!   /version                     attr: "1.0"
//!   /created_at                  attr: ISO-8601 string
//!   /source_type                 attr: "raw_acquisition" | "complex_maps_import" | ...
//!   /analysis_params             attr: JSON string (serialized AnalysisParams)
//!
//!   /anatomical                  dataset: u8 (H, W)  — optional
//!
//!   /acquisition/                group — optional, only from raw acquisition
//!     frames/<name>              dataset: f32 (T, H, W) chunked+gzip
//!     timestamps/<name>          dataset: f64 (T,)
//!
//!   /complex_maps/               group — present after DFT or import
//!     azi_fwd                    dataset: f64 (H, W, 2) where [:,:,0]=re, [:,:,1]=im
//!     azi_rev, alt_fwd, alt_rev
//!
//!   /results/                    group — present after retinotopy computation
//!     azi_phase                  dataset: f64 (H, W)
//!     alt_phase, azi_phase_degrees, alt_phase_degrees
//!     azi_amplitude, alt_amplitude, vfs

use hdf5::File as H5File;
use ndarray::{Array2, Array3, s};
use num_complex::Complex64;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::{
    AnalysisError, AnalysisParams, AnalysisResult, ComplexMaps, ProgressSink,
    RawProcessingResult, SnrMaps,
    math,
};

// ---------------------------------------------------------------------------
// Capability detection — what can the system do with this file?
// ---------------------------------------------------------------------------

/// Info about one result dataset in the file.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ResultInfo {
    pub name: String,
    /// "scalar_map" (f64 H,W), "bool_mask" (u8 H,W), "label_map" (i32 H,W), "sign_array" (i8 N,)
    pub result_type: String,
}

/// What's present in an .oisi file.
pub struct FileCapabilities {
    pub has_anatomical: bool,
    pub has_acquisition: bool,
    pub has_complex_maps: bool,
    pub has_results: bool,
    /// Map dimensions (H, W) — from whichever data is present
    pub dimensions: Option<(usize, usize)>,
    /// Names of acquisition cycle groups
    pub acquisition_cycles: Vec<String>,
    /// Typed result entries
    pub results: Vec<ResultInfo>,
}

/// Inspect an .oisi file and report what's present.
pub fn inspect(path: &Path) -> Result<FileCapabilities, AnalysisError> {
    let file = open_read(path)?;

    let has_anatomical = file.dataset("anatomical").is_ok();
    let has_acquisition = file.group("acquisition").is_ok();
    let has_complex_maps = file.group("complex_maps").is_ok();
    let has_results = file.group("results").is_ok();

    let mut dimensions = None;

    // Try to determine dimensions from whatever is present
    if has_results {
        if let Ok(ds) = file.dataset("results/azi_phase") {
            let shape = ds.shape();
            if shape.len() == 2 {
                dimensions = Some((shape[0], shape[1]));
            }
        }
    }
    if dimensions.is_none() && has_complex_maps {
        if let Ok(ds) = file.dataset("complex_maps/azi_fwd") {
            let shape = ds.shape();
            if shape.len() == 3 && shape[2] == 2 {
                dimensions = Some((shape[0], shape[1]));
            }
        }
    }
    if dimensions.is_none() && has_acquisition {
        // Try new format: acquisition/camera/frames is (T, H, W)
        if let Ok(ds) = file.dataset("acquisition/camera/frames") {
            let shape = ds.shape();
            if shape.len() == 3 {
                dimensions = Some((shape[1], shape[2]));
            }
        }
    }
    if dimensions.is_none() && has_anatomical {
        if let Ok(ds) = file.dataset("anatomical") {
            let shape = ds.shape();
            if shape.len() == 2 {
                dimensions = Some((shape[0], shape[1]));
            }
        }
    }

    // Check for camera frames — new format uses acquisition/camera/frames (single dataset),
    // old format used acquisition/frames/<cycle_name> (group of datasets).
    let acquisition_cycles = if has_acquisition {
        if file.group("acquisition/camera").is_ok() {
            // New format: single contiguous frame array. Report as one "all" cycle.
            vec!["all".into()]
        } else if file.group("acquisition/frames").is_ok() {
            // Old format: frames grouped by cycle.
            list_group_members(&file, "acquisition/frames")
        } else {
            vec![]
        }
    } else {
        vec![]
    };

    // Classify each result dataset by type.
    let results = if has_results {
        let group = file.group("results").ok();
        if let Some(g) = group {
            let names = list_group_members_from_group(&g);
            names.into_iter().filter_map(|name| {
                if let Ok(ds) = g.dataset(&name) {
                    let shape = ds.shape();
                    let dtype = ds.dtype().ok();
                    let result_type = classify_result_type(&name, &shape, dtype.as_ref());
                    Some(ResultInfo { name, result_type })
                } else {
                    None // skip sub-groups
                }
            }).collect()
        } else {
            vec![]
        }
    } else {
        vec![]
    };

    Ok(FileCapabilities {
        has_anatomical,
        has_acquisition,
        has_complex_maps,
        has_results,
        dimensions,
        acquisition_cycles,
        results,
    })
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

/// Read analysis params stored in the file (if any).
pub fn read_params(path: &Path) -> Result<Option<AnalysisParams>, AnalysisError> {
    let file = open_read(path)?;
    match file.attr("analysis_params") {
        Ok(attr) => {
            let json_vlu: hdf5::types::VarLenUnicode = attr.read_scalar()
                .map_err(|e| AnalysisError::Hdf5(format!("reading analysis_params attr: {e}")))?;
            let json: String = json_vlu.as_str().to_string();
            let params: AnalysisParams = serde_json::from_str(&json)
                .map_err(|e| AnalysisError::InvalidPackage(format!("parsing analysis_params: {e}")))?;
            Ok(Some(params))
        }
        Err(_) => Ok(None),
    }
}

/// Read the four complex maps.
pub fn read_complex_maps(path: &Path) -> Result<ComplexMaps, AnalysisError> {
    let file = open_read(path)?;

    let read_complex = |name: &str| -> Result<Array2<Complex64>, AnalysisError> {
        let ds_path = format!("complex_maps/{name}");
        let ds = file.dataset(&ds_path)
            .map_err(|e| AnalysisError::MissingData(format!("{ds_path}: {e}")))?;
        let raw: Array3<f64> = ds.read()
            .map_err(|e| AnalysisError::Hdf5(format!("reading {ds_path}: {e}")))?;
        let (h, w, c) = raw.dim();
        if c != 2 {
            return Err(AnalysisError::InvalidPackage(
                format!("{ds_path}: expected shape (H,W,2), got dim 2 = {c}")
            ));
        }
        let mut result = Array2::<Complex64>::zeros((h, w));
        for r in 0..h {
            for col in 0..w {
                result[[r, col]] = Complex64::new(raw[[r, col, 0]], raw[[r, col, 1]]);
            }
        }
        Ok(result)
    };

    Ok(ComplexMaps {
        azi_fwd: read_complex("azi_fwd")?,
        azi_rev: read_complex("azi_rev")?,
        alt_fwd: read_complex("alt_fwd")?,
        alt_rev: read_complex("alt_rev")?,
    })
}

/// Read a single result map by name (e.g. "azi_phase", "vfs").
pub fn read_result_map(path: &Path, name: &str) -> Result<Array2<f64>, AnalysisError> {
    let file = open_read(path)?;
    let ds_path = format!("results/{name}");
    let ds = file.dataset(&ds_path)
        .map_err(|e| AnalysisError::MissingData(format!("{ds_path}: {e}")))?;
    let data: Array2<f64> = ds.read()
        .map_err(|e| AnalysisError::Hdf5(format!("reading {ds_path}: {e}")))?;
    Ok(data)
}

/// Read the anatomical image as u8 grayscale.
pub fn read_anatomical(path: &Path) -> Result<Array2<u8>, AnalysisError> {
    let file = open_read(path)?;
    let ds = file.dataset("anatomical")
        .map_err(|e| AnalysisError::MissingData(format!("anatomical: {e}")))?;
    let data: Array2<u8> = ds.read()
        .map_err(|e| AnalysisError::Hdf5(format!("reading anatomical: {e}")))?;
    Ok(data)
}

/// Read a single raw acquisition frame by cycle name and frame index.
pub fn read_raw_frame(
    path: &Path,
    cycle_name: &str,
    frame_index: usize,
) -> Result<Array2<f32>, AnalysisError> {
    let file = open_read(path)?;
    let ds_path = format!("acquisition/frames/{cycle_name}");
    let ds = file.dataset(&ds_path)
        .map_err(|e| AnalysisError::MissingData(format!("{ds_path}: {e}")))?;
    let shape = ds.shape();
    if shape.len() != 3 || frame_index >= shape[0] {
        return Err(AnalysisError::InvalidPackage(
            format!("{ds_path}: frame index {frame_index} out of range (T={})", shape[0])
        ));
    }
    let frame: Array2<f32> = ds.read_slice(s![frame_index, .., ..])
        .map_err(|e| AnalysisError::Hdf5(format!("reading frame {frame_index} from {ds_path}: {e}")))?;
    Ok(frame)
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

/// Create a new .oisi file with just metadata.
pub fn create(path: &Path, source_type: &str) -> Result<(), AnalysisError> {
    let file = H5File::create(path)
        .map_err(|e| AnalysisError::Hdf5(format!("creating {}: {e}", path.display())))?;

    write_str_attr(&file, "version", "1.0")?;
    write_str_attr(&file, "source_type", source_type)?;
    write_str_attr(&file, "created_at", &chrono_now())?;

    Ok(())
}

/// Write complex maps to the file.
pub fn write_complex_maps(path: &Path, maps: &ComplexMaps) -> Result<(), AnalysisError> {
    let file = open_readwrite(path)?;

    // Remove existing group if present, then recreate
    let _ = file.unlink("complex_maps");
    let group = file.create_group("complex_maps")
        .map_err(|e| AnalysisError::Hdf5(format!("creating complex_maps group: {e}")))?;

    let write_complex = |name: &str, data: &Array2<Complex64>| -> Result<(), AnalysisError> {
        let (h, w) = data.dim();
        let mut raw = Array3::<f64>::zeros((h, w, 2));
        for r in 0..h {
            for c in 0..w {
                raw[[r, c, 0]] = data[[r, c]].re;
                raw[[r, c, 1]] = data[[r, c]].im;
            }
        }
        group.new_dataset_builder()
            .with_data(&raw)
            .create(name)
            .map_err(|e| AnalysisError::Hdf5(format!("writing complex_maps/{name}: {e}")))?;
        Ok(())
    };

    write_complex("azi_fwd", &maps.azi_fwd)?;
    write_complex("azi_rev", &maps.azi_rev)?;
    write_complex("alt_fwd", &maps.alt_fwd)?;
    write_complex("alt_rev", &maps.alt_rev)?;

    Ok(())
}

/// Write all analysis results atomically: retinotopy maps, VFS borders, and optional SNR.
/// Write ALL analysis results as flat datasets in `/results/`. No sub-groups.
pub fn write_results(
    path: &Path,
    result: &AnalysisResult,
    params: &AnalysisParams,
) -> Result<(), AnalysisError> {
    let file = open_readwrite(path)?;

    // Store params as JSON attribute on the root.
    let params_json = serde_json::to_string(params)
        .map_err(|e| AnalysisError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
    write_str_attr(&file, "analysis_params", &params_json)?;

    // Remove and recreate results group (flat).
    let _ = file.unlink("results");
    let group = file.create_group("results")
        .map_err(|e| AnalysisError::Hdf5(format!("creating results group: {e}")))?;

    // Helper: write f64 (H,W) dataset.
    let write_f64 = |name: &str, data: &Array2<f64>| -> Result<(), AnalysisError> {
        group.new_dataset_builder().with_data(data).create(name)
            .map_err(|e| AnalysisError::Hdf5(format!("writing results/{name}: {e}")))?;
        Ok(())
    };
    // Helper: write u8 mask (H,W) dataset.
    let write_mask = |name: &str, data: &Array2<bool>| -> Result<(), AnalysisError> {
        let u8data = data.mapv(|b| b as u8);
        group.new_dataset_builder().with_data(&u8data).create(name)
            .map_err(|e| AnalysisError::Hdf5(format!("writing results/{name}: {e}")))?;
        Ok(())
    };

    // Core retinotopy (7 maps).
    write_f64("azi_phase", &result.azi_phase)?;
    write_f64("alt_phase", &result.alt_phase)?;
    write_f64("azi_phase_degrees", &result.azi_phase_degrees)?;
    write_f64("alt_phase_degrees", &result.alt_phase_degrees)?;
    write_f64("azi_amplitude", &result.azi_amplitude)?;
    write_f64("alt_amplitude", &result.alt_amplitude)?;
    write_f64("vfs", &result.vfs)?;

    // Segmentation outputs.
    write_f64("vfs_thresholded", &result.vfs_thresholded)?;
    group.new_dataset_builder().with_data(&result.area_labels).create("area_labels")
        .map_err(|e| AnalysisError::Hdf5(format!("writing results/area_labels: {e}")))?;
    let signs_arr = ndarray::Array1::from(result.area_signs.clone());
    group.new_dataset_builder().with_data(&signs_arr).create("area_signs")
        .map_err(|e| AnalysisError::Hdf5(format!("writing results/area_signs: {e}")))?;
    write_mask("area_borders", &result.area_borders)?;

    // Derived maps.
    write_f64("eccentricity", &result.eccentricity)?;
    write_f64("magnification", &result.magnification)?;
    write_mask("contours_azi", &result.contours_azi)?;
    write_mask("contours_alt", &result.contours_alt)?;

    // SNR (only from raw acquisition).
    if let Some(ref snr) = result.snr_azi { write_f64("snr_azi", snr)?; }
    if let Some(ref snr) = result.snr_alt { write_f64("snr_alt", snr)?; }

    let area_count = result.area_signs.len();
    if area_count > 0 {
        eprintln!("[analysis] {} areas segmented", area_count);
    }

    Ok(())
}

/// Write an anatomical image.
pub fn write_anatomical(path: &Path, image: &Array2<u8>) -> Result<(), AnalysisError> {
    let file = open_readwrite(path)?;
    let _ = file.unlink("anatomical");
    file.new_dataset_builder()
        .with_data(image)
        .create("anatomical")
        .map_err(|e| AnalysisError::Hdf5(format!("writing anatomical: {e}")))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Streaming raw frame processing
// ---------------------------------------------------------------------------

/// Process raw acquisition frames into complex maps.
/// Supports both the new format (acquisition/camera/frames + stimulus arrays)
/// and handles grouping by condition from the stimulus state.
pub fn compute_complex_maps_from_raw(
    path: &Path,
    params: &AnalysisParams,
    progress: &dyn ProgressSink,
    cancel: &AtomicBool,
) -> Result<RawProcessingResult, AnalysisError> {
    let file = open_read(path)?;

    if file.group("acquisition/camera").is_ok() {
        return compute_complex_maps_new_format(&file, params, progress, cancel);
    }

    Err(AnalysisError::MissingData(
        "No supported acquisition format found. Expected acquisition/camera/ group.".into()
    ))
}

/// New format: all frames in acquisition/camera/frames (u16 T,H,W),
/// stimulus state in acquisition/stimulus/ arrays.
/// Groups frames by condition using stimulus state, computes dF/F + DFT per group.
fn compute_complex_maps_new_format(
    file: &hdf5::File,
    params: &AnalysisParams,
    progress: &dyn ProgressSink,
    cancel: &AtomicBool,
) -> Result<RawProcessingResult, AnalysisError> {
    progress.set_stage("Loading camera frames");
    progress.set_progress(0.0);

    // Read all camera frames (u16).
    let frames_ds = file.dataset("acquisition/camera/frames")
        .map_err(|e| AnalysisError::Hdf5(format!("opening camera/frames: {e}")))?;
    let all_frames: Array3<u16> = frames_ds.read()
        .map_err(|e| AnalysisError::Hdf5(format!("reading camera/frames: {e}")))?;
    let (t_cam, h, w) = all_frames.dim();

    // Read unified camera timestamps (seconds from t=0).
    let cam_ts_sec: Vec<f64> = file.dataset("acquisition/camera/timestamps_sec")
        .map_err(|e| AnalysisError::Hdf5(format!("opening camera timestamps_sec: {e}")))?
        .read_1d()
        .map_err(|e| AnalysisError::Hdf5(format!("reading camera timestamps_sec: {e}")))?
        .to_vec();

    // Read sweep schedule to get condition names.
    let schedule_group = file.group("acquisition/schedule")
        .map_err(|_| AnalysisError::MissingData("acquisition/schedule".into()))?;
    let seq_attr = schedule_group.attr("sweep_sequence")
        .map_err(|e| AnalysisError::Hdf5(format!("reading sweep_sequence: {e}")))?;
    let seq_json: hdf5::types::VarLenUnicode = seq_attr.read_scalar()
        .map_err(|e| AnalysisError::Hdf5(format!("reading sweep_sequence value: {e}")))?;
    let sweep_sequence: Vec<String> = serde_json::from_str(seq_json.as_str())
        .map_err(|e| AnalysisError::InvalidPackage(format!("parsing sweep_sequence: {e}")))?;

    // Read experiment to get condition list.
    let exp_json: hdf5::types::VarLenUnicode = file.attr("experiment")
        .map_err(|e| AnalysisError::Hdf5(format!("reading experiment attr: {e}")))?
        .read_scalar()
        .map_err(|e| AnalysisError::Hdf5(format!("reading experiment value: {e}")))?;
    let experiment: serde_json::Value = serde_json::from_str(exp_json.as_str())
        .map_err(|e| AnalysisError::InvalidPackage(format!("parsing experiment: {e}")))?;
    let conditions: Vec<String> = experiment["presentation"]["conditions"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    if conditions.is_empty() {
        return Err(AnalysisError::InvalidPackage("No conditions in experiment".into()));
    }

    if cancel.load(Ordering::Relaxed) {
        return Err(AnalysisError::Cancelled);
    }

    progress.set_stage("Computing baseline");
    progress.set_progress(0.2);

    // Compute global mean (baseline) from all frames.
    let n_pixels = h * w;
    let mut baseline_flat = vec![0.0f64; n_pixels];
    for t in 0..t_cam {
        let frame = all_frames.slice(ndarray::s![t, .., ..]);
        if let Some(data) = frame.as_slice() {
            for px in 0..n_pixels {
                baseline_flat[px] += data[px] as f64;
            }
        }
    }
    let inv_t = 1.0 / t_cam as f64;
    for px in 0..n_pixels {
        baseline_flat[px] *= inv_t;
    }
    let baseline_sum = ndarray::Array2::from_shape_vec((h, w), baseline_flat)
        .expect("baseline shape mismatch");

    // Read sweep schedule for per-repetition processing.
    let sweep_start_sec: Vec<f64> = file.dataset("acquisition/schedule/sweep_start_sec")
        .and_then(|ds| ds.read_1d().map(|a| a.to_vec()))
        .unwrap_or_default();
    let sweep_end_sec: Vec<f64> = file.dataset("acquisition/schedule/sweep_end_sec")
        .and_then(|ds| ds.read_1d().map(|a| a.to_vec()))
        .unwrap_or_default();

    progress.set_stage("Processing sweeps");
    progress.set_progress(0.3);

    let mut accumulator = CycleAccumulator::new();
    let n_sweeps = sweep_start_sec.len().min(sweep_end_sec.len()).min(sweep_sequence.len());
    let baseline_slice = baseline_sum.as_slice().unwrap();
    let eps = params.epsilon;

    // Process each individual sweep (repetition) separately, then average in CycleAccumulator.
    for sweep_i in 0..n_sweeps {
        if cancel.load(Ordering::Relaxed) {
            return Err(AnalysisError::Cancelled);
        }

        let start = sweep_start_sec[sweep_i];
        let end = sweep_end_sec[sweep_i];
        let cond = &sweep_sequence[sweep_i];

        // Classify direction.
        let (is_azi, is_fwd) = match classify_cycle_name(cond) {
            Some(v) => v,
            None => continue,
        };

        // Find camera frames within this sweep's time window.
        let frame_indices: Vec<usize> = (0..t_cam)
            .filter(|&i| cam_ts_sec[i] >= start && cam_ts_sec[i] <= end)
            .collect();

        if frame_indices.is_empty() { continue; }

        progress.set_stage(&format!("Sweep {}/{} {} ({} frames)",
            sweep_i + 1, n_sweeps, cond, frame_indices.len()));
        progress.set_progress(0.3 + 0.6 * sweep_i as f64 / n_sweeps as f64);

        let n = frame_indices.len();
        let mut frames_f32 = Array3::<f32>::zeros((n, h, w));

        // Extract and compute dF/F.
        for (fi, &cam_i) in frame_indices.iter().enumerate() {
            let src = all_frames.slice(ndarray::s![cam_i, .., ..]);
            let mut dst = frames_f32.slice_mut(ndarray::s![fi, .., ..]);
            if let (Some(src_data), Some(dst_data)) = (src.as_slice(), dst.as_slice_mut()) {
                for px in 0..n_pixels {
                    let raw = src_data[px] as f64;
                    let base = baseline_slice[px];
                    dst_data[px] = ((raw - base) / (base + eps)) as f32;
                }
            }
        }

        // Timestamps for this sweep's frames.
        let timestamps: Vec<f64> = frame_indices.iter()
            .map(|&i| cam_ts_sec[i])
            .collect();

        // DFT projection for this single sweep.
        let complex_map = math::dft_projection(&frames_f32, &timestamps, is_fwd);
        accumulator.add(complex_map, is_azi, is_fwd);

        // SNR: compute once per axis from the first forward sweep only.
        if is_fwd && ((is_azi && accumulator.snr_azi.is_none()) || (!is_azi && accumulator.snr_alt.is_none())) {
            let snr = math::compute_snr_map(&frames_f32, &timestamps);
            if is_azi {
                accumulator.snr_azi = Some(snr);
            } else {
                accumulator.snr_alt = Some(snr);
            }
        }
    }

    accumulator.finalize()
}

// ---------------------------------------------------------------------------
// SNLC .mat import
// ---------------------------------------------------------------------------

/// Import a directory of SNLC .mat files into a new .oisi file.
///
/// Expected directory contents:
/// - Two paired .mat files with `f1m` cell arrays (horizontal=azimuth, vertical=altitude)
///   Convention: lower-numbered file = horizontal (004), higher = vertical (005)
/// - Optional `grab_*.mat` anatomical reference image
///
/// Returns the output .oisi path.
pub fn import_snlc_directory(
    dir_path: &Path,
    output_path: &Path,
) -> Result<(), AnalysisError> {
    use crate::mat5;

    // Find .mat files in the directory
    let entries = std::fs::read_dir(dir_path)
        .map_err(|e| AnalysisError::Io(e))?;

    let mut data_mats: Vec<std::path::PathBuf> = Vec::new();
    let mut grab_mat: Option<std::path::PathBuf> = None;

    for entry in entries {
        let entry = entry.map_err(|e| AnalysisError::Io(e))?;
        let path = entry.path();
        if let Some(ext) = path.extension() {
            if ext.to_ascii_lowercase() == "mat" {
                let name = path.file_name().expect("path has extension so must have filename").to_string_lossy().to_lowercase();
                if name.starts_with("grab") || name.starts_with("grab_") {
                    grab_mat = Some(path);
                } else if !name.contains("analyzer") {
                    data_mats.push(path);
                }
            }
        }
    }

    // Sort data .mat files by name so lower number = horizontal (azimuth)
    data_mats.sort();

    if data_mats.len() < 2 {
        return Err(AnalysisError::MissingData(format!(
            "need at least 2 .mat data files in {}, found {}",
            dir_path.display(),
            data_mats.len()
        )));
    }

    // Read complex maps from the paired .mat files
    // Convention: first file (lower number) = horizontal = azimuth
    //             second file (higher number) = vertical = altitude
    let azi_cells = mat5::read_snlc_f1m(&data_mats[0])?;
    if azi_cells.len() < 2 {
        return Err(AnalysisError::InvalidPackage(format!(
            "{}: f1m has {} cells, expected 2",
            data_mats[0].display(),
            azi_cells.len()
        )));
    }

    let alt_cells = mat5::read_snlc_f1m(&data_mats[1])?;
    if alt_cells.len() < 2 {
        return Err(AnalysisError::InvalidPackage(format!(
            "{}: f1m has {} cells, expected 2",
            data_mats[1].display(),
            alt_cells.len()
        )));
    }

    let mut azi_iter = azi_cells.into_iter();
    let azi_fwd = azi_iter.next().unwrap().data;
    let azi_rev = azi_iter.next().unwrap().data;

    let mut alt_iter = alt_cells.into_iter();
    let alt_fwd = alt_iter.next().unwrap().data;
    let alt_rev = alt_iter.next().unwrap().data;

    let complex_maps = ComplexMaps {
        azi_fwd,
        azi_rev,
        alt_fwd,
        alt_rev,
    };

    // Create the .oisi file
    create(output_path, "complex_maps_import")?;
    write_complex_maps(output_path, &complex_maps)?;

    // Import anatomical if present
    if let Some(grab_path) = &grab_mat {
        eprintln!("[import] found anatomical: {}", grab_path.display());
        match mat5::read_snlc_anatomical(grab_path) {
            Ok(anat) => {
                let (h, w) = anat.dim();
                eprintln!("[import] anatomical: {}x{}", w, h);
                write_anatomical(output_path, &anat)?;
            }
            Err(e) => {
                eprintln!("[import] WARNING: could not read anatomical from {}: {e}", grab_path.display());
            }
        }
    } else {
        eprintln!("[import] no grab_*.mat found in directory");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn open_read(path: &Path) -> Result<H5File, AnalysisError> {
    H5File::open(path)
        .map_err(|e| AnalysisError::Hdf5(format!("opening {}: {e}", path.display())))
}

fn open_readwrite(path: &Path) -> Result<H5File, AnalysisError> {
    H5File::open_rw(path)
        .map_err(|e| AnalysisError::Hdf5(format!("opening {}: {e}", path.display())))
}

fn write_str_attr(location: &H5File, name: &str, value: &str) -> Result<(), AnalysisError> {
    // Remove existing attribute if present.
    let _ = location.delete_attr(name);
    let attr = location
        .new_attr::<hdf5::types::VarLenUnicode>()
        .create(name)
        .map_err(|e| AnalysisError::Hdf5(format!("creating attr {name}: {e}")))?;
    let val: hdf5::types::VarLenUnicode = value.parse().unwrap();
    attr.write_scalar(&val)
        .map_err(|e| AnalysisError::Hdf5(format!("writing attr {name}: {e}")))?;
    Ok(())
}

fn list_group_members(file: &H5File, group_path: &str) -> Vec<String> {
    let group = file.group(group_path)
        .unwrap_or_else(|e| panic!("Failed to open HDF5 group '{}': {}", group_path, e));
    list_group_members_from_group(&group)
}

fn list_group_members_from_group(group: &hdf5::Group) -> Vec<String> {
    group.member_names()
        .expect("Failed to list HDF5 group members")
}

/// Classify a result dataset by its name and HDF5 type.
fn classify_result_type(name: &str, shape: &[usize], _dtype: Option<&hdf5::Datatype>) -> String {
    // Known bool masks (stored as u8).
    if name == "area_borders" || name == "contours_azi" || name == "contours_alt" {
        return "bool_mask".into();
    }
    // Known label maps (stored as i32).
    if name == "area_labels" {
        return "label_map".into();
    }
    // 1D arrays = metadata.
    if shape.len() == 1 {
        return "sign_array".into();
    }
    // Default: scalar map (f64 H,W).
    "scalar_map".into()
}

fn chrono_now() -> String {
    // Simple ISO-8601 without pulling in chrono crate
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("System clock is before Unix epoch");
    format!("{}", duration.as_secs())
}

/// Classify a cycle name by direction. Returns (is_azi, is_fwd).
fn classify_cycle_name(name: &str) -> Option<(bool, bool)> {
    let lower = name.to_lowercase();
    if lower.starts_with("lr") {
        Some((true, true))     // azimuth forward
    } else if lower.starts_with("rl") {
        Some((true, false))    // azimuth reverse
    } else if lower.starts_with("tb") {
        Some((false, true))    // altitude forward
    } else if lower.starts_with("bt") {
        Some((false, false))   // altitude reverse
    } else if lower.starts_with("cw") {
        Some((true, true))     // wedge clockwise → azimuth forward
    } else if lower.starts_with("ccw") {
        Some((true, false))    // wedge counter-clockwise → azimuth reverse
    } else if lower.starts_with("expand") {
        Some((false, true))    // ring expand → altitude forward
    } else if lower.starts_with("contract") {
        Some((false, false))   // ring contract → altitude reverse
    } else {
        None
    }
}

/// Accumulates complex maps per direction for averaging.
struct CycleAccumulator {
    azi_fwd: Option<(Array2<Complex64>, usize)>,
    azi_rev: Option<(Array2<Complex64>, usize)>,
    alt_fwd: Option<(Array2<Complex64>, usize)>,
    alt_rev: Option<(Array2<Complex64>, usize)>,
    pub snr_azi: Option<ndarray::Array2<f64>>,
    pub snr_alt: Option<ndarray::Array2<f64>>,
}

impl CycleAccumulator {
    fn new() -> Self {
        Self {
            azi_fwd: None,
            azi_rev: None,
            alt_fwd: None,
            alt_rev: None,
            snr_azi: None,
            snr_alt: None,
        }
    }

    fn add(&mut self, map: Array2<Complex64>, is_azi: bool, is_fwd: bool) {
        let slot = match (is_azi, is_fwd) {
            (true, true) => &mut self.azi_fwd,
            (true, false) => &mut self.azi_rev,
            (false, true) => &mut self.alt_fwd,
            (false, false) => &mut self.alt_rev,
        };
        match slot {
            Some((ref mut sum, ref mut count)) => {
                *sum += &map;
                *count += 1;
            }
            None => {
                *slot = Some((map, 1));
            }
        }
    }

    fn finalize(self) -> Result<RawProcessingResult, AnalysisError> {
        let avg = |slot: Option<(Array2<Complex64>, usize)>, label: &str| -> Result<Array2<Complex64>, AnalysisError> {
            match slot {
                Some((sum, count)) => Ok(sum.mapv(|v| v / count as f64)),
                None => Err(AnalysisError::MissingData(format!("no cycles found for {label}"))),
            }
        };

        let complex_maps = ComplexMaps {
            azi_fwd: avg(self.azi_fwd, "azi_fwd (LR)")?,
            azi_rev: avg(self.azi_rev, "azi_rev (RL)")?,
            alt_fwd: avg(self.alt_fwd, "alt_fwd (TB)")?,
            alt_rev: avg(self.alt_rev, "alt_rev (BT)")?,
        };

        let dims = complex_maps.azi_fwd.raw_dim();
        let snr = SnrMaps {
            snr_azi: self.snr_azi.unwrap_or_else(|| ndarray::Array2::zeros(dims)),
            snr_alt: self.snr_alt.unwrap_or_else(|| ndarray::Array2::zeros(dims)),
        };

        Ok(RawProcessingResult { complex_maps, snr })
    }
}
