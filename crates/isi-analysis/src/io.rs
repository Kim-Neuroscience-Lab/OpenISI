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
use ndarray::{Array2, Array3};
use num_complex::Complex64;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::{
    AcquisitionProperties, AnalysisError, AnalysisParams, AnalysisResult, ComplexMaps, ProgressSink,
    RawProcessingResult,
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
        // acquisition/camera/frames is (T, H, W).
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

    // Camera frames live at acquisition/camera/frames (single contiguous
    // dataset). The reader supports nothing else; reporting any other
    // shape here would advertise a capability the read path can't honor.
    let acquisition_cycles = if has_acquisition && file.group("acquisition/camera").is_ok() {
        vec!["all".into()]
    } else {
        vec![]
    };

    // Classify each result dataset by type.
    let results = if has_results {
        let group = file.group("results").ok();
        if let Some(g) = group {
            let names = list_group_members_from_group(&g)?;
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

/// Read the self-describing render metadata (palette, display range,
/// units, NaN/zero semantics) attached to a `/results/<name>` dataset.
/// Returns `None` when the dataset is absent or the attrs haven't been
/// written (legacy files written before `attach_meta`).
pub fn read_result_meta(path: &Path, name: &str) -> Option<MapMeta> {
    let file = open_read(path).ok()?;
    let ds_path = format!("results/{name}");
    let ds = file.dataset(&ds_path).ok()?;
    read_map_meta(&ds)
}

/// Read the anatomical image as u8 grayscale.
/// Portable identifiers for an acquisition, read from the `.oisi` root
/// attributes. Used by dev tooling (e.g. `dev_figures/meta.json`) to identify
/// the source recording without baking absolute paths into shareable artifacts.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AcquisitionIdentity {
    /// `/animal_id` root attribute (e.g. `"5/14/2026_test5"`).
    pub animal_id: String,
    /// `/created_at` root attribute (unix timestamp string — globally unique
    /// to this acquisition, survives renames and copies).
    pub created_at: String,
}

/// Read the acquisition identity attributes from a `.oisi` file. Returns `None`
/// for either field if its attribute is missing.
pub fn read_acquisition_identity(path: &Path) -> Result<AcquisitionIdentity, AnalysisError> {
    let file = open_read(path)?;
    let read = |name: &str| -> String {
        file.attr(name)
            .and_then(|a| a.read_scalar::<hdf5::types::VarLenUnicode>())
            .map(|s| s.as_str().to_string())
            .unwrap_or_default()
    };
    Ok(AcquisitionIdentity {
        animal_id: read("animal_id"),
        created_at: read("created_at"),
    })
}

/// Write the `.oisi /analysis_params` attribute from a registry-tree
/// JSON value (the shape produced by
/// `RegistrySnapshot::to_json_for_target(PersistTarget::Analysis)`).
///
/// The bridge owns conversion from a `RegistrySnapshot` to an
/// `AnalysisParams`; this function owns persistence of the snapshot
/// tree to the `.oisi` file. The two are decoupled because the
/// analysis crate has no notion of a Registry.
///
/// **Atomicity note:** HDF5 attribute rewrite is in-place on an
/// existing file. Crash-during-write leaves the file with the old
/// attribute; parallel writers to the same file are unsafe regardless.
pub fn write_analysis_params_attr(
    path: &Path,
    registry_tree: &serde_json::Value,
) -> Result<(), AnalysisError> {
    let file = open_readwrite(path)?;
    let json = serde_json::to_string(registry_tree)
        .map_err(|e| AnalysisError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
    write_str_attr(&file, "analysis_params", &json)?;
    Ok(())
}

/// Return true iff the file's `/analysis_params` JSON attribute uses
/// the pre-2026 schema (legacy serde-derived `AnalysisParams` shape:
/// tagged-enum object with `"method"` keys; OR a flat object with
/// stimulus-geometry fields that were moved to `/experiment_params`).
/// Used by the orchestrator to detect old-schema files and refuse
/// analysis with a clear "run `oisi migrate <file>`" message.
///
/// Returns `Ok(false)` when the attribute is absent or matches the
/// current registry-tree schema; returns `Err` only on HDF5 / I/O
/// failure.
pub fn is_pre_2026_analysis_params(path: &Path) -> Result<bool, AnalysisError> {
    // Pre-2026 markers: either the moved-field names at root, OR the
    // tagged-enum shape (`"<stage>": {"method": "..."}` with the
    // stage's tunables also at that level). The current schema is
    // `"<stage>": {"method": "..."}` PLUS sibling tunable subtrees,
    // so the presence of `"method"` alone doesn't distinguish — we rely
    // on the moved-field names OR a flat (non-subtree) tunable sibling.
    const MOVED_FIELDS: &[&str] = &[
        "azi_angular_range",
        "alt_angular_range",
        "offset_azi",
        "offset_alt",
        "rotation_k",
        "um_per_pixel",
    ];

    let Some(value) = read_analysis_params_attr(path)? else {
        return Ok(false);
    };
    let Some(obj) = value.as_object() else { return Ok(false); };
    if MOVED_FIELDS.iter().any(|f| obj.contains_key(*f)) {
        return Ok(true);
    }
    // Detect the legacy tagged-enum shape by its *distinguishing* marker:
    // a FLAT tunable sibling of `method` — a non-object (scalar) value
    // carried directly at the stage level. The legacy serde-derived
    // AnalysisParams wrote tunables flat
    // (`{"method": "x", "sigma_px": 2.5}`); the current schema nests every
    // variant's tunables under that variant's object subtree
    // (`{"method": "x", "x": {"sigma_px": 2.5}, "y": {…}}` — a stage can
    // carry MULTIPLE variant subtrees, only one of which is active). So a
    // stage is legacy iff it has `method` and any non-`method` sibling
    // whose value is NOT an object (a scalar tunable).
    //
    // Method-only stages (`{"method": "x"}`) are NOT a marker: tunable-less
    // methods (cycle_combine, vfs_computation, quality_gate, eccentricity)
    // serialize to exactly that shape in the *current* schema, so flagging
    // them as legacy would falsely reject valid files (and break
    // re-analysis of already-analyzed files).
    for (_stage, stage_val) in obj.iter() {
        if let Some(stage_obj) = stage_val.as_object() {
            if stage_obj.contains_key("method") {
                let has_flat_tunable = stage_obj
                    .iter()
                    .any(|(k, v)| k != "method" && !v.is_object());
                if has_flat_tunable {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

/// Read the `/analysis_params` HDF5 attribute as a raw
/// `serde_json::Value` (the registry-tree shape). Returns `None` if
/// the attribute is absent (file never analyzed); returns `Err` only
/// on HDF5 / parse failure.
///
/// Callers that need an `AnalysisParams` must rebuild a
/// `RegistrySnapshot` via `RegistrySnapshot::from_json_tree` and then
/// invoke `crate::bridge::analysis_params_from_snapshot`. The bridge
/// is the only construction path.
pub fn read_analysis_params_attr(path: &Path) -> Result<Option<serde_json::Value>, AnalysisError> {
    let file = open_read(path)?;
    let attr_names = file.attr_names()
        .map_err(|e| AnalysisError::Hdf5(format!("listing root attrs: {e}")))?;
    if !attr_names.iter().any(|n| n == "analysis_params") {
        return Ok(None);
    }
    let attr = file.attr("analysis_params")
        .map_err(|e| AnalysisError::Hdf5(format!("opening analysis_params attr: {e}")))?;
    let json_vlu: hdf5::types::VarLenUnicode = attr.read_scalar()
        .map_err(|e| AnalysisError::Hdf5(format!("reading analysis_params attr: {e}")))?;
    let value: serde_json::Value = serde_json::from_str(json_vlu.as_str())
        .map_err(|e| AnalysisError::InvalidPackage(format!("parsing analysis_params: {e}")))?;
    Ok(Some(value))
}

/// Read the `rig_params` JSON attribute from a `.oisi` file, if present.
/// Captured at acquisition time (`src-tauri/src/export.rs::write_oisi`).
/// Returns an opaque `serde_json::Value` because the analysis crate
/// doesn't have a typed `RigParams` struct — the rig config is
/// provenance, not analysis input. Returns `None` for files captured
/// before `/rig_params` was written.
pub fn read_rig_params(path: &Path) -> Result<Option<serde_json::Value>, AnalysisError> {
    read_root_json_attr(path, "rig_params")
}

/// Read the `experiment_params` JSON attribute from a `.oisi` file, if
/// present. Same provenance role as `read_rig_params`. Returns `None`
/// for files captured before `/experiment_params` was written.
pub fn read_experiment_params(path: &Path) -> Result<Option<serde_json::Value>, AnalysisError> {
    read_root_json_attr(path, "experiment_params")
}

/// Helper for reading a JSON-encoded root HDF5 attribute that may be
/// absent on older files. Used by `read_rig_params` and
/// `read_experiment_params`.
fn read_root_json_attr(path: &Path, name: &str) -> Result<Option<serde_json::Value>, AnalysisError> {
    let file = open_read(path)?;
    let attr_names = file.attr_names()
        .map_err(|e| AnalysisError::Hdf5(format!("listing root attrs: {e}")))?;
    if !attr_names.iter().any(|n| n == name) {
        return Ok(None);
    }
    let attr = file.attr(name)
        .map_err(|e| AnalysisError::Hdf5(format!("opening {name} attr: {e}")))?;
    let json_vlu: hdf5::types::VarLenUnicode = attr.read_scalar()
        .map_err(|e| AnalysisError::Hdf5(format!("reading {name} attr: {e}")))?;
    let value: serde_json::Value = serde_json::from_str(json_vlu.as_str())
        .map_err(|e| AnalysisError::InvalidPackage(format!("parsing {name}: {e}")))?;
    Ok(Some(value))
}

/// Read the user-drawn cortex ROI from `/anatomical/cortex_roi`, if
/// present. Returns `Ok(None)` when the dataset is absent (no user
/// override for this file). Returns `Err` only on I/O / parse failure.
///
/// The dataset is stored as `u8` (0/1) for HDF5 compatibility; this
/// helper converts to `Array2<bool>`. Source-of-truth path is
/// `/anatomical/cortex_roi`; consumers (analyze orchestrator, future
/// UI) write to that path when the user provides an explicit ROI.
pub fn read_cortex_roi(path: &Path) -> Result<Option<Array2<bool>>, AnalysisError> {
    let file = open_read(path)?;
    if !file.link_exists("anatomical/cortex_roi") {
        return Ok(None);
    }
    let ds = file.dataset("anatomical/cortex_roi")
        .map_err(|e| AnalysisError::Hdf5(format!("opening anatomical/cortex_roi: {e}")))?;
    let data: Array2<u8> = ds.read()
        .map_err(|e| AnalysisError::Hdf5(format!("reading anatomical/cortex_roi: {e}")))?;
    Ok(Some(data.mapv(|v| v != 0)))
}

pub fn read_anatomical(path: &Path) -> Result<Array2<u8>, AnalysisError> {
    let file = open_read(path)?;
    let ds = file.dataset("anatomical")
        .map_err(|e| AnalysisError::MissingData(format!("anatomical: {e}")))?;
    let data: Array2<u8> = ds.read()
        .map_err(|e| AnalysisError::Hdf5(format!("reading anatomical: {e}")))?;
    Ok(data)
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

/// Write all analysis results as flat datasets in `/results/`. No sub-groups.
///
/// **Does NOT write `/analysis_params`** — that's the orchestrator's
/// responsibility via `write_analysis_params_attr` with the registry-
/// tree JSON. Keeping them separate avoids `AnalysisParams` needing
/// to carry a serde representation; the Registry tree is the canonical
/// on-disk form, and only the orchestrator (which owns the snapshot)
/// can produce it.
pub fn write_results(
    path: &Path,
    result: &AnalysisResult,
    acquisition: &AcquisitionProperties,
    _params: &AnalysisParams,
) -> Result<(), AnalysisError> {
    let file = open_readwrite(path)?;

    // Remove and recreate results group (flat).
    let _ = file.unlink("results");
    let group = file.create_group("results")
        .map_err(|e| AnalysisError::Hdf5(format!("creating results group: {e}")))?;

    // Writes a f64 (H,W) dataset and attaches the meta attrs that
    // describe how to render it. The renderer reads these attrs and
    // does ZERO inference — palette, units, display range, NaN/zero
    // semantics are all decided here, once, at write time.
    let write_f64 = |name: &str, data: &Array2<f64>| -> Result<(), AnalysisError> {
        let ds = group.new_dataset_builder().with_data(data).create(name)
            .map_err(|e| AnalysisError::Hdf5(format!("writing results/{name}: {e}")))?;
        let meta = meta_for_f64(name, data, acquisition);
        attach_meta(&ds, &meta)?;
        Ok(())
    };
    let write_mask = |name: &str, data: &Array2<bool>| -> Result<(), AnalysisError> {
        let u8data = data.mapv(|b| b as u8);
        let ds = group.new_dataset_builder().with_data(&u8data).create(name)
            .map_err(|e| AnalysisError::Hdf5(format!("writing results/{name}: {e}")))?;
        attach_meta(&ds, &map_meta_bool())?;
        Ok(())
    };

    // Phases, amplitudes, and the three VFS algorithm stages.
    // `vfs` is the raw mathematical VFS; `vfs_smoothed` is the
    // smoothed array segmentation operated on; `vfs_smoothed_thresholded`
    // is the literal threshold mask. All full frame — no cortex
    // masking pre-baked.
    write_f64("azi_phase", &result.azi_phase)?;
    write_f64("alt_phase", &result.alt_phase)?;
    write_f64("azi_phase_degrees", &result.azi_phase_degrees)?;
    write_f64("alt_phase_degrees", &result.alt_phase_degrees)?;
    write_f64("azi_amplitude", &result.azi_amplitude)?;
    write_f64("alt_amplitude", &result.alt_amplitude)?;
    write_f64("vfs", &result.vfs)?;
    write_f64("vfs_smoothed", &result.vfs_smoothed)?;
    write_f64("vfs_smoothed_thresholded", &result.vfs_smoothed_thresholded)?;

    // Segmentation outputs.
    write_mask("cortex_mask", &result.cortex_mask)?;
    let labels_ds = group.new_dataset_builder().with_data(&result.area_labels).create("area_labels")
        .map_err(|e| AnalysisError::Hdf5(format!("writing results/area_labels: {e}")))?;
    attach_meta(&labels_ds, &map_meta_labels())?;
    let signs_arr = ndarray::Array1::from(result.area_signs.clone());
    group.new_dataset_builder().with_data(&signs_arr).create("area_signs")
        .map_err(|e| AnalysisError::Hdf5(format!("writing results/area_signs: {e}")))?;
    write_mask("area_borders", &result.area_borders)?;

    // Derived maps.
    write_f64("eccentricity", &result.eccentricity)?;
    write_f64("magnification", &result.magnification)?;
    write_mask("contours_azi", &result.contours_azi)?;
    write_mask("contours_alt", &result.contours_alt)?;

    if let Some(ref snr) = result.snr {
        write_f64("snr_azi", &snr.snr_azi)?;
        write_f64("snr_alt", &snr.snr_alt)?;
    }

    // Per-direction cross-cycle reliability (Allen / Engel). Source of
    // truth for the cortex mask above; persisted so the user (or a
    // future reanalysis) can re-derive cortex with a different threshold
    // without rerunning the raw pipeline.
    if let Some(ref rel) = result.reliability {
        write_f64("reliability_azi_fwd", &rel.rel_azi_fwd)?;
        write_f64("reliability_azi_rev", &rel.rel_azi_rev)?;
        write_f64("reliability_alt_fwd", &rel.rel_alt_fwd)?;
        write_f64("reliability_alt_rev", &rel.rel_alt_rev)?;
    }

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
/// Compute four complex maps + (optionally) per-orientation SNR from the
/// raw camera frames in an .oisi file.
///
/// Allen-aligned per-direction cycle averaging — matches
/// `corticalmapping/HighLevel.py::getMappingMovies` and
/// `corticalmapping/core/ImageAnalysis.py::get_average_movie`:
///
///   1. Read all camera frames + camera timestamps, sweep schedule
///      (`sweep_start_sec`, `sweep_end_sec`, `sweep_sequence`).
///   2. `meanFrameDur = mean(diff(cam_ts))` — uniform-regime camera period.
///   3. For each direction d ∈ {LR, RL, TB, BT}:
///      a. Gather sweep indices `k` where `sweep_sequence[k]` is this
///         direction (10 cycles in the standard 10-repetition protocol).
///      b. Per-direction chunk duration = mean(`sweep_end - sweep_start`)
///         across this direction's cycles.
///      c. `chunkFrameDur = ceil(chunk_dur / meanFrameDur)`.
///      d. For each cycle: onset_frame_idx =
///         `argmin(|cam_ts - sweep_start[k]|)`. The contiguous slice
///         `mov[onset:onset+chunkFrameDur]` is this cycle's frames.
///         (Allen `ImageAnalysis.py:1207-1213`.)
///      e. Per-cycle FFT bin 1: `freq = 1 / (chunkFrameDur · meanFrameDur)`.
///      f. Push (cycle complex map, global phase, cycle frames) into the
///         `CycleAccumulator`. The accumulator handles phase-locked
///         averaging and SNR bundling in `finalize()`.
///      g. SNR computed on the cycle-averaged movie for the first fwd sweep
///         per orientation.
///   4. `accumulator.finalize()` produces the per-direction complex maps
///      (phase-locked across cycles) and an `Option<SnrMaps>`. No
///      baseline subtraction — `isRectify=False` default.
///
/// `condition_indices`, `state_ids`, and `sweep_indices` from
/// `acquisition/stimulus/*` are not used for cycle assignment. The schedule
/// — `sweep_start_sec` and `sweep_sequence` — is the ground truth for
/// onset times. This matches Allen's use of `displayOnsets` from the
/// display log.
pub fn compute_complex_maps_from_raw(
    path: &Path,
    _params: &AnalysisParams,
    progress: &dyn ProgressSink,
    cancel: &AtomicBool,
) -> Result<RawProcessingResult, AnalysisError> {
    let file = open_read(path)?;

    if file.group("acquisition/camera").is_err() {
        return Err(AnalysisError::MissingData(
            "Expected acquisition/camera/ group".into(),
        ));
    }

    progress.set_stage("Loading camera frames");
    progress.set_progress(0.0);

    let frames_ds = file.dataset("acquisition/camera/frames")
        .map_err(|e| AnalysisError::Hdf5(format!("opening camera/frames: {e}")))?;
    let all_frames: Array3<u16> = frames_ds.read()
        .map_err(|e| AnalysisError::Hdf5(format!("reading camera/frames: {e}")))?;
    let (t_cam, _h, _w) = all_frames.dim();
    if t_cam < 2 {
        return Err(AnalysisError::MissingData("fewer than 2 camera frames".into()));
    }

    let cam_ts_sec: Vec<f64> = file.dataset("acquisition/camera/timestamps_sec")
        .map_err(|e| AnalysisError::Hdf5(format!("opening camera timestamps_sec: {e}")))?
        .read_1d()
        .map_err(|e| AnalysisError::Hdf5(format!("reading camera timestamps_sec: {e}")))?
        .to_vec();

    // Sweep schedule — onset times + per-sweep duration + direction.
    let sweep_start_sec: Vec<f64> = file.dataset("acquisition/schedule/sweep_start_sec")
        .map_err(|e| AnalysisError::Hdf5(format!("opening sweep_start_sec: {e}")))?
        .read_1d()
        .map_err(|e| AnalysisError::Hdf5(format!("reading sweep_start_sec: {e}")))?
        .to_vec();
    let sweep_end_sec: Vec<f64> = file.dataset("acquisition/schedule/sweep_end_sec")
        .map_err(|e| AnalysisError::Hdf5(format!("opening sweep_end_sec: {e}")))?
        .read_1d()
        .map_err(|e| AnalysisError::Hdf5(format!("reading sweep_end_sec: {e}")))?
        .to_vec();
    let schedule_group = file.group("acquisition/schedule")
        .map_err(|_| AnalysisError::MissingData("acquisition/schedule".into()))?;
    let seq_json: hdf5::types::VarLenUnicode = schedule_group.attr("sweep_sequence")
        .map_err(|e| AnalysisError::Hdf5(format!("reading sweep_sequence: {e}")))?
        .read_scalar()
        .map_err(|e| AnalysisError::Hdf5(format!("reading sweep_sequence value: {e}")))?;
    let sweep_sequence: Vec<String> = serde_json::from_str(seq_json.as_str())
        .map_err(|e| AnalysisError::InvalidPackage(format!("parsing sweep_sequence: {e}")))?;

    let n_sweeps = sweep_sequence.len()
        .min(sweep_start_sec.len())
        .min(sweep_end_sec.len());
    if n_sweeps == 0 {
        return Err(AnalysisError::MissingData("no sweeps in schedule".into()));
    }

    if cancel.load(Ordering::Relaxed) {
        return Err(AnalysisError::Cancelled);
    }

    // `meanFrameDur` — Allen `ImageAnalysis.py:1184`.
    let mean_frame_dur = (cam_ts_sec[t_cam - 1] - cam_ts_sec[0]) / (t_cam - 1) as f64;

    // Group sweep indices by direction.
    use std::collections::BTreeMap;
    let mut dir_groups: BTreeMap<crate::compute::Direction, Vec<usize>> = BTreeMap::new();
    for k in 0..n_sweeps {
        if let Some(direction) = classify_cycle_name(&sweep_sequence[k]) {
            dir_groups.entry(direction).or_default().push(k);
        }
    }
    if dir_groups.is_empty() {
        return Err(AnalysisError::InvalidPackage(
            "no sweeps with recognized direction names".into(),
        ));
    }

    let mut accumulator = crate::compute::CycleAccumulator::new();
    let n_dirs = dir_groups.len() as f64;
    for (dir_idx, (direction, sweep_ks)) in dir_groups.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(AnalysisError::Cancelled);
        }

        // Allen-style chunk duration: per-direction `sweepDur` is the mean
        // of `sweep_end - sweep_start` over this direction's cycles.
        let chunk_dur: f64 = sweep_ks.iter()
            .map(|&k| sweep_end_sec[k] - sweep_start_sec[k])
            .sum::<f64>() / sweep_ks.len() as f64;
        // `chunkFrameDur = ceil(chunkDur / meanFrameDur)` — Allen
        // `ImageAnalysis.py:1187`.
        let chunk_frame_dur = (chunk_dur / mean_frame_dur).ceil() as usize;

        progress.set_stage(&format!(
            "Direction {}: {} cycles × {chunk_frame_dur} frames",
            direction.label(), sweep_ks.len(),
        ));
        progress.set_progress(0.1 + 0.2 * dir_idx as f64 / n_dirs);

        let period_sec = chunk_frame_dur as f64 * mean_frame_dur;
        let freq_bin1 = 1.0 / period_sec;

        // For each cycle: upload frames, compute bin-1 complex map +
        // global phase, push into the accumulator. The accumulator does
        // the phase-locked averaging at finalize time.
        for &k in sweep_ks {
            let onset = sweep_start_sec[k];
            if onset < cam_ts_sec[0] || onset + chunk_dur > cam_ts_sec[t_cam - 1] {
                continue;
            }
            let onset_idx = nearest_index_sorted(&cam_ts_sec, onset);
            if onset_idx + chunk_frame_dur > t_cam { continue; }

            let frame_indices: Vec<usize> = (onset_idx..onset_idx + chunk_frame_dur).collect();
            let cycle_t = crate::compute::frames_u16_subset_to_tensor_f32(
                &all_frames, &frame_indices,
            );

            let cm_k = crate::compute::dft_projection_at_freq(
                &cycle_t, mean_frame_dur, freq_bin1,
            );

            // Global per-cycle phase: arg(Σ_pixels cm_k).
            let (re_sum, im_sum) = crate::compute::complex_tensor_real_imag_sum(&cm_k);
            let phi_k = im_sum.atan2(re_sum);

            accumulator.add_cycle(*direction, cm_k, phi_k, cycle_t)?;
        }

        // SNR on Allen's frame-domain cycle-averaged movie. Once per
        // orientation on the first fwd direction.
        let is_first_fwd_for_orientation = direction.is_fwd();
        if is_first_fwd_for_orientation {
            if let Some(averaged) = accumulator.averaged_movie(*direction) {
                let uniform_ts: Vec<f64> = (0..chunk_frame_dur as i64)
                    .map(|k| k as f64 * mean_frame_dur)
                    .collect();
                let snr = crate::compute::compute_snr(&averaged, &uniform_ts);
                accumulator.record_snr(*direction, snr)?;
            }
        }
    }

    progress.set_progress(0.95);
    accumulator.finalize()
}

/// Binary search for the index of the element in `sorted` closest to `target`.
/// Assumes `sorted` is non-empty and non-decreasing.
fn nearest_index_sorted(sorted: &[f64], target: f64) -> usize {
    match sorted.binary_search_by(|v| v.partial_cmp(&target).unwrap_or(std::cmp::Ordering::Equal)) {
        Ok(i) => i,
        Err(insert_at) => {
            if insert_at == 0 { 0 }
            else if insert_at >= sorted.len() { sorted.len() - 1 }
            else {
                let lo = insert_at - 1;
                let hi = insert_at;
                if (target - sorted[lo]).abs() <= (sorted[hi] - target).abs() { lo } else { hi }
            }
        }
    }
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
                let Some(file_name) = path.file_name() else { continue };
                let name = file_name.to_string_lossy().to_lowercase();
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

    // The `< 2` length checks above guarantee these `next()` calls
    // succeed, but unwrap would panic on a stale .mat file with an
    // unexpected layout. Use explicit ok_or_else so any mismatch
    // surfaces as a clean `InvalidPackage` instead of a backtrace.
    let mut azi_iter = azi_cells.into_iter();
    let azi_fwd = azi_iter.next().ok_or_else(|| AnalysisError::InvalidPackage(
        format!("{}: f1m missing azi_fwd cell after length check", data_mats[0].display())
    ))?.data;
    let azi_rev = azi_iter.next().ok_or_else(|| AnalysisError::InvalidPackage(
        format!("{}: f1m missing azi_rev cell after length check", data_mats[0].display())
    ))?.data;

    let mut alt_iter = alt_cells.into_iter();
    let alt_fwd = alt_iter.next().ok_or_else(|| AnalysisError::InvalidPackage(
        format!("{}: f1m missing alt_fwd cell after length check", data_mats[1].display())
    ))?.data;
    let alt_rev = alt_iter.next().ok_or_else(|| AnalysisError::InvalidPackage(
        format!("{}: f1m missing alt_rev cell after length check", data_mats[1].display())
    ))?.data;

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

fn write_str_attr(
    location: &hdf5::Location,
    name: &str,
    value: &str,
) -> Result<(), AnalysisError> {
    // Remove existing attribute if present.
    let _ = location.delete_attr(name);
    let attr = location
        .new_attr::<hdf5::types::VarLenUnicode>()
        .create(name)
        .map_err(|e| AnalysisError::Hdf5(format!("creating attr {name}: {e}")))?;
    let val: hdf5::types::VarLenUnicode = value.parse().map_err(|e| {
        AnalysisError::Hdf5(format!("invalid UTF-8 attr {name}: {e}"))
    })?;
    attr.write_scalar(&val)
        .map_err(|e| AnalysisError::Hdf5(format!("writing attr {name}: {e}")))?;
    Ok(())
}

fn write_f64_attr(
    location: &hdf5::Location,
    name: &str,
    value: f64,
) -> Result<(), AnalysisError> {
    let _ = location.delete_attr(name);
    let attr = location
        .new_attr::<f64>()
        .create(name)
        .map_err(|e| AnalysisError::Hdf5(format!("creating attr {name}: {e}")))?;
    attr.write_scalar(&value)
        .map_err(|e| AnalysisError::Hdf5(format!("writing attr {name}: {e}")))?;
    Ok(())
}

fn list_group_members_from_group(group: &hdf5::Group) -> crate::Result<Vec<String>> {
    group.member_names()
        .map_err(|e| AnalysisError::Hdf5(format!("listing HDF5 group members: {e}")))
}

// ─────────────────────────────────────────────────────────────────────────
// Self-describing /results datasets — rendering metadata as HDF5 attrs.
//
// Every f64 map decides palette / units / display range / NaN-semantics
// HERE, at write time, using the params it actually used. Downstream
// renderers (figure exporter, Tauri UI) read these attrs and do zero
// inference — no name-matching, no auto-percentile, no unit guessing.
// New maps added via `meta_for_f64` render automatically.
// ─────────────────────────────────────────────────────────────────────────

/// Per-dataset rendering metadata, attached as HDF5 attrs on the dataset.
///
/// All renderers (`headless::render_map`, Tauri `export_map_png`) read
/// these attrs and require nothing else. The attribute schema is the
/// data-layer ↔ renderer contract.
///
/// Strings are `Cow` so the pipeline can build with static literals
/// (zero alloc), and read-back from HDF5 produces owned `String`s
/// (no leak).
#[derive(Clone, Debug)]
pub struct MapMeta {
    /// Colormap name. Renderers map this to a palette function.
    /// One of: `"hsv_circular"`, `"jet"`, `"hot"`, `"binary"`,
    /// `"categorical"`.
    pub palette: std::borrow::Cow<'static, str>,
    /// Physical units of the data values: `"rad"`, `"deg"`,
    /// `"unitless"`, `"bool"`, `"label"`.
    pub units: std::borrow::Cow<'static, str>,
    /// Value mapped to the palette start.
    pub display_min: f64,
    /// Value mapped to the palette end.
    pub display_max: f64,
    /// Period for circular palettes (`2π` for radian phases,
    /// `angular_range` for degree phases). `0.0` means non-circular.
    pub wrap_period: f64,
    /// Semantic meaning of `NaN` values (e.g. `"outside_cortex"`).
    /// Empty when NaN is not expected.
    pub nan_means: std::borrow::Cow<'static, str>,
    /// Semantic meaning of literal `0.0` values, when a sentinel is
    /// used (e.g. `"outside_patch"` for eccentricity/magnification).
    /// Empty when `0.0` is just a regular value.
    pub zero_means: std::borrow::Cow<'static, str>,
}

/// Bool masks (cortex_mask, area_borders, contours_*): stored as u8.
fn map_meta_bool() -> MapMeta {
    use std::borrow::Cow;
    MapMeta {
        palette: Cow::Borrowed("binary"),
        units: Cow::Borrowed("bool"),
        display_min: 0.0,
        display_max: 1.0,
        wrap_period: 0.0,
        nan_means: Cow::Borrowed(""),
        zero_means: Cow::Borrowed(""),
    }
}

/// Categorical label map (area_labels): each integer is an area ID;
/// renderers pick a categorical palette indexed by label value.
fn map_meta_labels() -> MapMeta {
    use std::borrow::Cow;
    MapMeta {
        palette: Cow::Borrowed("categorical"),
        units: Cow::Borrowed("label"),
        display_min: 0.0,
        display_max: 0.0,
        wrap_period: 0.0,
        nan_means: Cow::Borrowed(""),
        zero_means: Cow::Borrowed("background"),
    }
}

/// Decide the rendering metadata for a `Array2<f64>` `/results/<name>`
/// dataset. Single source of truth — name → meta — replacing the
/// renderer-side `render_kind_for` switch and all its inferred ranges.
fn meta_for_f64(name: &str, data: &Array2<f64>, acquisition: &AcquisitionProperties) -> MapMeta {
    use std::borrow::Cow;
    let lit = Cow::Borrowed;
    let half_azi = acquisition.azi_angular_range / 2.0;
    let half_alt = acquisition.alt_angular_range / 2.0;
    match name {
        // Radian phases: HSV over [-π, π], period 2π. Full frame.
        "azi_phase" | "alt_phase" => MapMeta {
            palette: lit("hsv_circular"),
            units: lit("rad"),
            display_min: -std::f64::consts::PI,
            display_max:  std::f64::consts::PI,
            wrap_period:  std::f64::consts::TAU,
            nan_means: lit(""),
            zero_means: lit(""),
        },
        // Degree phases: HSV over [offset - range/2, offset + range/2].
        "azi_phase_degrees" => MapMeta {
            palette: lit("hsv_circular"),
            units: lit("deg"),
            display_min: acquisition.offset_azi - half_azi,
            display_max: acquisition.offset_azi + half_azi,
            wrap_period: acquisition.azi_angular_range,
            nan_means: lit(""),
            zero_means: lit(""),
        },
        "alt_phase_degrees" => MapMeta {
            palette: lit("hsv_circular"),
            units: lit("deg"),
            display_min: acquisition.offset_alt - half_alt,
            display_max: acquisition.offset_alt + half_alt,
            wrap_period: acquisition.alt_angular_range,
            nan_means: lit(""),
            zero_means: lit(""),
        },
        // The three VFS algorithm stages. Same palette/range (jet ±1) so
        // they're visually comparable. Threshold-masked variant uses
        // 0 as the sentinel for "below threshold".
        "vfs" | "vfs_smoothed" => MapMeta {
            palette: lit("jet"),
            units: lit("unitless"),
            display_min: -1.0,
            display_max:  1.0,
            wrap_period: 0.0,
            nan_means: lit(""),
            zero_means: lit(""),
        },
        "vfs_smoothed_thresholded" => MapMeta {
            palette: lit("jet"),
            units: lit("unitless"),
            display_min: -1.0,
            display_max:  1.0,
            wrap_period: 0.0,
            nan_means: lit(""),
            zero_means: lit("below_threshold"),
        },
        // Amplitudes are finite everywhere (they define cortex). Hot
        // palette over the data's actual finite range — frozen here so
        // the renderer needs no auto-fit.
        n if n.ends_with("_amplitude") => {
            let (lo, hi) = finite_range(data);
            MapMeta {
                palette: lit("hot"),
                units: lit("unitless"),
                display_min: lo,
                display_max: hi,
                wrap_period: 0.0,
                nan_means: lit(""),
                zero_means: lit(""),
            }
        }
        // Eccentricity: jet over the 2-98 percentile of valid pixels.
        // `0.0` is the native compute_eccentricity sentinel for
        // pixels outside any segmented patch (`area_labels == 0`).
        "eccentricity" => {
            let (lo, hi) = sentinel_percentile(data, 0.02, 0.98);
            MapMeta {
                palette: lit("jet"),
                units: lit("deg"),
                display_min: lo,
                display_max: hi,
                wrap_period: 0.0,
                nan_means: lit(""),
                zero_means: lit("outside_patch"),
            }
        }
        // Magnification: same convention as eccentricity. The Jacobian
        // ratio is unitless once the input phases are in degrees.
        "magnification" => {
            let (lo, hi) = sentinel_percentile(data, 0.02, 0.98);
            MapMeta {
                palette: lit("jet"),
                units: lit("unitless"),
                display_min: lo,
                display_max: hi,
                wrap_period: 0.0,
                nan_means: lit(""),
                zero_means: lit("outside_patch"),
            }
        }
        // Reliability maps (Allen / Engel cross-cycle vector coherence):
        // bounded [0, 1] by construction. Hot palette over the full
        // range makes the cortex region pop visually.
        "reliability_azi_fwd" | "reliability_azi_rev"
        | "reliability_alt_fwd" | "reliability_alt_rev" => MapMeta {
            palette: lit("hot"),
            units: lit("unitless"),
            display_min: 0.0,
            display_max: 1.0,
            wrap_period: 0.0,
            nan_means: lit(""),
            zero_means: lit(""),
        },
        // SNR maps: per-condition. No canonical fixed range — jet over
        // 2-98 percentile of finite values.
        "snr_azi" | "snr_alt" => {
            let (lo, hi) = sentinel_percentile(data, 0.02, 0.98);
            MapMeta {
                palette: lit("jet"),
                units: lit("unitless"),
                display_min: lo,
                display_max: hi,
                wrap_period: 0.0,
                nan_means: lit(""),
                zero_means: lit(""),
            }
        }
        // Unknown map: jet over percentile, leave NaN/zero semantics
        // empty. Adding a new map name with bespoke conventions means
        // adding an arm above — no renderer change needed.
        _ => {
            let (lo, hi) = sentinel_percentile(data, 0.02, 0.98);
            MapMeta {
                palette: lit("jet"),
                units: lit("unitless"),
                display_min: lo,
                display_max: hi,
                wrap_period: 0.0,
                nan_means: lit(""),
                zero_means: lit(""),
            }
        }
    }
}

/// Min/max over finite values. Returns `(0, 1)` if there are none
/// (avoids the renderer dividing by zero on an empty range).
fn finite_range(data: &Array2<f64>) -> (f64, f64) {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for &v in data.iter() {
        if v.is_finite() {
            if v < lo { lo = v; }
            if v > hi { hi = v; }
        }
    }
    if !lo.is_finite() { return (0.0, 1.0); }
    if (hi - lo).abs() < 1e-12 { (lo, lo + 1.0) } else { (lo, hi) }
}

/// Two-sided percentile of finite, non-zero values — the right range
/// for sentinel-zero maps (eccentricity, magnification) where `0.0`
/// means "no data" and shouldn't influence the colorbar.
fn sentinel_percentile(data: &Array2<f64>, p_lo: f64, p_hi: f64) -> (f64, f64) {
    let mut vals: Vec<f64> = data.iter()
        .copied()
        .filter(|v| v.is_finite() && *v != 0.0)
        .collect();
    if vals.is_empty() { return (0.0, 1.0); }
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = vals.len();
    let idx = |p: f64| -> usize {
        ((p * (n - 1) as f64).round() as usize).min(n - 1)
    };
    let lo = vals[idx(p_lo)];
    let hi = vals[idx(p_hi)];
    if (hi - lo).abs() < 1e-12 { (lo, lo + 1.0) } else { (lo, hi) }
}

/// Attach the `MapMeta` fields to a dataset as HDF5 attributes.
fn attach_meta(dataset: &hdf5::Dataset, m: &MapMeta) -> Result<(), AnalysisError> {
    write_str_attr(dataset, "palette", &m.palette)?;
    write_str_attr(dataset, "units", &m.units)?;
    write_f64_attr(dataset, "display_min", m.display_min)?;
    write_f64_attr(dataset, "display_max", m.display_max)?;
    write_f64_attr(dataset, "wrap_period", m.wrap_period)?;
    write_str_attr(dataset, "nan_means", &m.nan_means)?;
    write_str_attr(dataset, "zero_means", &m.zero_means)?;
    Ok(())
}

/// Read the rendering metadata back from a dataset. Returns `None`
/// when any required attr is missing (legacy files written before
/// the self-describing-attrs pass, ~2026-05-23). Renderers callers
/// must handle `None` explicitly — there is no inference fallback.
///
/// `nan_means` and `zero_means` are intentionally optional (returned
/// as empty string when missing): empty-string is the correct
/// "no sentinel semantics" value, indistinguishable from "attr
/// genuinely absent for a non-sentinel map." All other fields are
/// required and `None` propagates if any are missing.
pub fn read_map_meta(dataset: &hdf5::Dataset) -> Option<MapMeta> {
    use std::borrow::Cow;
    Some(MapMeta {
        palette: Cow::Owned(read_str_attr(dataset, "palette")?),
        units: Cow::Owned(read_str_attr(dataset, "units")?),
        display_min: read_f64_attr(dataset, "display_min")?,
        display_max: read_f64_attr(dataset, "display_max")?,
        wrap_period: read_f64_attr(dataset, "wrap_period")?,
        nan_means: Cow::Owned(read_str_attr(dataset, "nan_means").unwrap_or_default()),
        zero_means: Cow::Owned(read_str_attr(dataset, "zero_means").unwrap_or_default()),
    })
}

fn read_str_attr(location: &hdf5::Location, name: &str) -> Option<String> {
    let attr = location.attr(name).ok()?;
    let v: hdf5::types::VarLenUnicode = attr.read_scalar().ok()?;
    Some(v.to_string())
}

fn read_f64_attr(location: &hdf5::Location, name: &str) -> Option<f64> {
    let attr = location.attr(name).ok()?;
    attr.read_scalar::<f64>().ok()
}

/// Classify a result dataset by its name and HDF5 shape. Single source of
/// truth for the type tag used by `inspect()` (which reports it for the UI
/// to discover what's available) and by the Tauri `read_result` command
/// (which dispatches reads based on this tag).
pub fn classify_result_type(name: &str, shape: &[usize], _dtype: Option<&hdf5::Datatype>) -> String {
    // Known bool masks (stored as u8).
    if name == "area_borders"
        || name == "contours_azi"
        || name == "contours_alt"
        || name == "cortex_mask"
    {
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
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", duration.as_secs())
}

/// Classify a sweep label from the acquisition schedule into a `Direction`.
///
/// **Empirically calibrated to produce Allen/Marshel-convention VFS
/// polarity** (V1 negative sign, RL/PM positive sign) for this imaging
/// rig. The pure label semantics would suggest `TB → AltRev` and
/// `BT → AltFwd` (since TB = altitude *decreasing* in mouse-perceived
/// coordinates after the monitor's 180° rotation correction). However the
/// camera image is *vertically flipped* relative to cortex coordinates by
/// the imaging relay optics — this flips `∂φ/∂y` in image-space relative
/// to cortex-space, which would invert VFS sign. The asymmetric label
/// assignment below absorbs that camera flip so VFS comes out Allen-
/// canonical without an explicit `camera_y_flip` knob in the pipeline.
///
/// Verified against `5_14_2026_test5_1778801597.oisi`: V1 edge renders
/// blue/negative, RL/PM render orange/positive, matching Allen/Marshel
/// figures.
fn classify_cycle_name(name: &str) -> Option<crate::compute::Direction> {
    use crate::compute::Direction;
    let lower = name.to_lowercase();
    if lower.starts_with("lr")           { Some(Direction::AziFwd) }
    else if lower.starts_with("rl")      { Some(Direction::AziRev) }
    else if lower.starts_with("tb")      { Some(Direction::AltFwd) } // absorbs camera vertical flip
    else if lower.starts_with("bt")      { Some(Direction::AltRev) } // absorbs camera vertical flip
    else if lower.starts_with("ccw")     { Some(Direction::AziRev) } // wedge counter-clockwise → azimuth rev (check ccw before cw)
    else if lower.starts_with("cw")      { Some(Direction::AziFwd) } // wedge clockwise → azimuth fwd
    else if lower.starts_with("expand")  { Some(Direction::AltFwd) } // ring expand → altitude fwd
    else if lower.starts_with("contract") { Some(Direction::AltRev) } // ring contract → altitude rev
    else                                 { None }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::Array2;
    use num_complex::Complex64;
    use std::path::PathBuf;

    /// Helper: build minimal AnalysisParams + AcquisitionProperties for
    /// writing results. Construction flows through the SSoT param
    /// registry → bridge, the same path production uses.
    fn test_params() -> crate::AnalysisParams {
        let dir = std::path::Path::new("/tmp/test");
        let reg = openisi_params::Registry::new(dir, dir);
        crate::bridge::analysis_params_from_snapshot(&reg.snapshot())
    }

    fn test_acquisition() -> crate::AcquisitionProperties {
        crate::AcquisitionProperties {
            azi_angular_range: 60.0,
            alt_angular_range: 40.0,
            offset_azi: 0.0,
            offset_alt: 0.0,
            rotation_k: 0,
            um_per_pixel: 20.0,
            // Hand-constructed test fixture; treat as Full since the
            // test provides every field explicitly.
            provenance: crate::ProvenanceLevel::Full,
        }
    }

    /// Helper: create a unique temp file path and ensure cleanup on drop.
    struct TempFile(PathBuf);

    impl TempFile {
        fn new(name: &str) -> Self {
            let mut path = std::env::temp_dir();
            path.push(format!("openisi_test_{}_{}", name, std::process::id()));
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    /// Build synthetic ComplexMaps of the given size.
    fn make_complex_maps(h: usize, w: usize) -> ComplexMaps {
        let make = |scale: f64| -> Array2<Complex64> {
            Array2::from_shape_fn((h, w), |(r, c)| {
                Complex64::new(
                    (r as f64 + 1.0) * scale,
                    (c as f64 + 1.0) * scale * 0.5,
                )
            })
        };
        ComplexMaps {
            azi_fwd: make(1.0),
            azi_rev: make(2.0),
            alt_fwd: make(3.0),
            alt_rev: make(4.0),
        }
    }

    // -------------------------------------------------------------------------
    // 1. Complex maps round-trip
    // -------------------------------------------------------------------------

    #[test]
    fn complex_maps_round_trip() {
        let tmp = TempFile::new("complex_rt");
        let maps = make_complex_maps(8, 8);

        create(tmp.path(), "test").unwrap();
        write_complex_maps(tmp.path(), &maps).unwrap();

        let loaded = read_complex_maps(tmp.path()).unwrap();

        assert_eq!(loaded.azi_fwd.dim(), (8, 8));
        assert_eq!(loaded.azi_fwd, maps.azi_fwd);
        assert_eq!(loaded.azi_rev, maps.azi_rev);
        assert_eq!(loaded.alt_fwd, maps.alt_fwd);
        assert_eq!(loaded.alt_rev, maps.alt_rev);
    }

    // -------------------------------------------------------------------------
    // 2. Results write + read round-trip
    // -------------------------------------------------------------------------

    #[test]
    fn results_round_trip() {
        let tmp = TempFile::new("results_rt");
        let (h, w) = (8, 8);
        let params = test_params();

        let result = crate::AnalysisResult {
            azi_phase: Array2::from_shape_fn((h, w), |(r, c)| r as f64 + c as f64),
            alt_phase: Array2::from_shape_fn((h, w), |(r, c)| r as f64 * 0.1 + c as f64 * 0.2),
            azi_phase_degrees: Array2::from_shape_fn((h, w), |(r, c)| (r + c) as f64 * 10.0),
            alt_phase_degrees: Array2::from_shape_fn((h, w), |(r, c)| (r + c) as f64 * 5.0),
            azi_amplitude: Array2::from_shape_fn((h, w), |(r, c)| (r * w + c) as f64 * 0.01),
            alt_amplitude: Array2::from_shape_fn((h, w), |(r, c)| (r * w + c) as f64 * 0.02),
            vfs: Array2::from_shape_fn((h, w), |(r, c)| if (r + c) % 2 == 0 { 1.0 } else { -1.0 }),
            vfs_smoothed: Array2::zeros((h, w)),
            vfs_smoothed_thresholded: Array2::zeros((h, w)),
            cortex_mask: Array2::from_elem((h, w), true),
            area_labels: Array2::zeros((h, w)),
            area_signs: vec![],
            area_borders: Array2::from_elem((h, w), false),
            eccentricity: Array2::zeros((h, w)),
            magnification: Array2::zeros((h, w)),
            contours_azi: Array2::from_elem((h, w), false),
            contours_alt: Array2::from_elem((h, w), false),
            snr: None,
            reliability: None,
        };

        create(tmp.path(), "test").unwrap();
        write_results(tmp.path(), &result, &test_acquisition(), &params).unwrap();

        // Read back individual result maps and verify.
        let azi_phase = read_result_map(tmp.path(), "azi_phase").unwrap();
        assert_eq!(azi_phase, result.azi_phase);

        let vfs = read_result_map(tmp.path(), "vfs").unwrap();
        assert_eq!(vfs, result.vfs);

        let alt_amplitude = read_result_map(tmp.path(), "alt_amplitude").unwrap();
        assert_eq!(alt_amplitude, result.alt_amplitude);
    }

    // -------------------------------------------------------------------------
    // 3. Anatomical round-trip
    // -------------------------------------------------------------------------

    #[test]
    fn anatomical_round_trip() {
        let tmp = TempFile::new("anat_rt");
        let (h, w) = (16, 16);
        let image = Array2::from_shape_fn((h, w), |(r, c)| ((r * w + c) % 256) as u8);

        create(tmp.path(), "test").unwrap();
        write_anatomical(tmp.path(), &image).unwrap();

        let loaded = read_anatomical(tmp.path()).unwrap();
        assert_eq!(loaded, image);
    }

    // -------------------------------------------------------------------------
    // 4. inspect() returns correct capabilities
    // -------------------------------------------------------------------------

    #[test]
    fn inspect_capabilities() {
        let tmp = TempFile::new("inspect_caps");
        let maps = make_complex_maps(8, 8);
        let image = Array2::<u8>::zeros((8, 8));

        create(tmp.path(), "test").unwrap();
        write_complex_maps(tmp.path(), &maps).unwrap();
        write_anatomical(tmp.path(), &image).unwrap();

        let caps = inspect(tmp.path()).unwrap();

        assert!(caps.has_complex_maps, "should detect complex_maps");
        assert!(caps.has_anatomical, "should detect anatomical");
        assert!(!caps.has_results, "should not detect results");
        assert!(!caps.has_acquisition, "should not detect acquisition");
        assert_eq!(caps.dimensions, Some((8, 8)));
    }

    #[test]
    fn inspect_with_results() {
        let tmp = TempFile::new("inspect_results");
        let params = test_params();
        let (h, w) = (8, 8);

        let result = crate::AnalysisResult {
            azi_phase: Array2::zeros((h, w)),
            alt_phase: Array2::zeros((h, w)),
            azi_phase_degrees: Array2::zeros((h, w)),
            alt_phase_degrees: Array2::zeros((h, w)),
            azi_amplitude: Array2::zeros((h, w)),
            alt_amplitude: Array2::zeros((h, w)),
            vfs: Array2::zeros((h, w)),
            vfs_smoothed: Array2::zeros((h, w)),
            vfs_smoothed_thresholded: Array2::zeros((h, w)),
            cortex_mask: Array2::from_elem((h, w), true),
            area_labels: Array2::zeros((h, w)),
            area_signs: vec![1, -1],
            area_borders: Array2::from_elem((h, w), false),
            eccentricity: Array2::zeros((h, w)),
            magnification: Array2::zeros((h, w)),
            contours_azi: Array2::from_elem((h, w), false),
            contours_alt: Array2::from_elem((h, w), false),
            snr: None,
            reliability: None,
        };

        create(tmp.path(), "test").unwrap();
        write_results(tmp.path(), &result, &test_acquisition(), &params).unwrap();

        let caps = inspect(tmp.path()).unwrap();
        assert!(caps.has_results, "should detect results");
        assert!(!caps.has_complex_maps, "no complex_maps written");
        assert_eq!(caps.dimensions, Some((8, 8)));

        // Verify result classification.
        let names: Vec<&str> = caps.results.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"azi_phase"), "results should list azi_phase");
        assert!(names.contains(&"vfs"), "results should list vfs");
        assert!(names.contains(&"area_labels"), "results should list area_labels");
    }

    // -------------------------------------------------------------------------
    // 5. import_snlc_directory() with missing files
    // -------------------------------------------------------------------------

    #[test]
    fn import_snlc_missing_mat_files() {
        let dir = std::env::temp_dir().join(format!("openisi_test_empty_dir_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let out = TempFile::new("import_snlc_out");

        let result = import_snlc_directory(&dir, out.path());
        assert!(result.is_err(), "should fail with no .mat files");

        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("need at least 2 .mat data files"),
            "error should mention missing .mat files, got: {err_msg}"
        );

        let _ = std::fs::remove_dir(&dir);
    }

    // -------------------------------------------------------------------------
    // 6. Empty file has no capabilities
    // -------------------------------------------------------------------------

    #[test]
    fn inspect_empty_file() {
        let tmp = TempFile::new("inspect_empty");
        create(tmp.path(), "test").unwrap();

        let caps = inspect(tmp.path()).unwrap();
        assert!(!caps.has_complex_maps);
        assert!(!caps.has_anatomical);
        assert!(!caps.has_results);
        assert!(!caps.has_acquisition);
        assert_eq!(caps.dimensions, None);
    }

    // -------------------------------------------------------------------------
    // 7. Overwriting complex maps replaces old data
    // -------------------------------------------------------------------------

    #[test]
    fn complex_maps_overwrite() {
        let tmp = TempFile::new("complex_overwrite");
        create(tmp.path(), "test").unwrap();

        let maps_v1 = make_complex_maps(8, 8);
        write_complex_maps(tmp.path(), &maps_v1).unwrap();

        // Write different maps.
        let maps_v2 = ComplexMaps {
            azi_fwd: Array2::from_elem((8, 8), Complex64::new(99.0, 99.0)),
            azi_rev: Array2::from_elem((8, 8), Complex64::new(88.0, 88.0)),
            alt_fwd: Array2::from_elem((8, 8), Complex64::new(77.0, 77.0)),
            alt_rev: Array2::from_elem((8, 8), Complex64::new(66.0, 66.0)),
        };
        write_complex_maps(tmp.path(), &maps_v2).unwrap();

        let loaded = read_complex_maps(tmp.path()).unwrap();
        assert_eq!(loaded.azi_fwd[[0, 0]], Complex64::new(99.0, 99.0));
        assert_eq!(loaded.alt_rev[[0, 0]], Complex64::new(66.0, 66.0));
    }

    // -------------------------------------------------------------------------
    // 8. read_params round-trip
    // -------------------------------------------------------------------------

    #[test]
    fn params_round_trip() {
        let tmp = TempFile::new("params_rt");
        let params = test_params();
        let (h, w) = (4, 4);

        // write_results stores params as an attribute.
        let result = crate::AnalysisResult {
            azi_phase: Array2::zeros((h, w)),
            alt_phase: Array2::zeros((h, w)),
            azi_phase_degrees: Array2::zeros((h, w)),
            alt_phase_degrees: Array2::zeros((h, w)),
            azi_amplitude: Array2::zeros((h, w)),
            alt_amplitude: Array2::zeros((h, w)),
            vfs: Array2::zeros((h, w)),
            vfs_smoothed: Array2::zeros((h, w)),
            vfs_smoothed_thresholded: Array2::zeros((h, w)),
            cortex_mask: Array2::from_elem((h, w), true),
            area_labels: Array2::zeros((h, w)),
            area_signs: vec![],
            area_borders: Array2::from_elem((h, w), false),
            eccentricity: Array2::zeros((h, w)),
            magnification: Array2::zeros((h, w)),
            contours_azi: Array2::from_elem((h, w), false),
            contours_alt: Array2::from_elem((h, w), false),
            snr: None,
            reliability: None,
        };

        create(tmp.path(), "test").unwrap();
        write_results(tmp.path(), &result, &test_acquisition(), &params).unwrap();

        // write_results no longer writes /analysis_params (the
        // orchestrator owns that via write_analysis_params_attr with
        // the Registry tree). Confirm the attribute is absent here.
        assert!(read_analysis_params_attr(tmp.path()).unwrap().is_none());

        // Then stamp a registry tree and verify it round-trips.
        let tree = serde_json::json!({"cycle_combine": {"method": "marshel_garrett2011_delay_subtraction"}});
        write_analysis_params_attr(tmp.path(), &tree).unwrap();
        let loaded = read_analysis_params_attr(tmp.path()).unwrap().unwrap();
        assert_eq!(loaded, tree);
    }

    // ─────────────────────────────────────────────────────────────────
    // write_analysis_params_attr round-trip
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn write_analysis_params_attr_round_trips_via_read() {
        // Write a registry-tree JSON value, read it back, verify equality.
        let tmp = TempFile::new("write_analysis_params");
        create(tmp.path(), "test").unwrap();

        let tree = serde_json::json!({
            "sign_map_smoothing": { "method": "gaussian", "gaussian": { "sigma_um": 77.0 } },
            "cortex_source":      { "method": "allen_zhuang2017_full_frame" },
        });
        write_analysis_params_attr(tmp.path(), &tree).unwrap();
        let loaded = read_analysis_params_attr(tmp.path()).unwrap().unwrap();
        assert_eq!(loaded, tree);
    }

    #[test]
    fn write_analysis_params_attr_overwrites_existing() {
        let tmp = TempFile::new("write_analysis_params_overwrite");
        create(tmp.path(), "test").unwrap();
        let a = serde_json::json!({"sign_map_smoothing": {"method": "gaussian"}});
        let b = serde_json::json!({"sign_map_smoothing": {"method": "gaussian", "gaussian": {"sigma_um": 90.0}}});
        write_analysis_params_attr(tmp.path(), &a).unwrap();
        write_analysis_params_attr(tmp.path(), &b).unwrap();
        let loaded = read_analysis_params_attr(tmp.path()).unwrap().unwrap();
        assert_eq!(loaded, b);
    }

    // ─────────────────────────────────────────────────────────────────
    // is_pre_2026_analysis_params detection
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn is_pre_2026_analysis_params_absent_attr() {
        let tmp = TempFile::new("pre2026_absent");
        create(tmp.path(), "test").unwrap();
        assert_eq!(is_pre_2026_analysis_params(tmp.path()).unwrap(), false);
    }

    #[test]
    fn is_pre_2026_analysis_params_current_schema_returns_false() {
        let tmp = TempFile::new("pre2026_current");
        create(tmp.path(), "test").unwrap();
        // Current schema: stage with method + sibling variant subtree.
        let tree = serde_json::json!({
            "sign_map_smoothing": {
                "method": "gaussian",
                "gaussian": { "sigma_um": 60.0 }
            }
        });
        write_analysis_params_attr(tmp.path(), &tree).unwrap();
        assert_eq!(is_pre_2026_analysis_params(tmp.path()).unwrap(), false);
    }

    #[test]
    fn is_pre_2026_analysis_params_old_schema_moved_fields_returns_true() {
        let tmp = TempFile::new("pre2026_old_moved");
        create(tmp.path(), "test").unwrap();
        let file = hdf5::File::open_rw(tmp.path()).unwrap();
        let stale_json = r#"{"azi_angular_range":120.0,"cycle_combine":{"method":"marshel_garrett2011_delay_subtraction"}}"#;
        write_str_attr(&file, "analysis_params", stale_json).unwrap();
        drop(file);
        assert_eq!(is_pre_2026_analysis_params(tmp.path()).unwrap(), true);
    }

    #[test]
    fn is_pre_2026_analysis_params_old_schema_flat_tunable_returns_true() {
        // Legacy serde-derived AnalysisParams: a tunable carried FLAT at the
        // stage level (sibling of `method`) rather than nested under a
        // variant subtree. The flat sibling is the distinguishing marker.
        let tmp = TempFile::new("pre2026_old_flat");
        create(tmp.path(), "test").unwrap();
        let stale = serde_json::json!({
            "phase_smoothing": {
                "method": "open_isi_amp_weighted_phasor",
                "sigma_px": 2.5
            }
        });
        write_analysis_params_attr(tmp.path(), &stale).unwrap();
        assert_eq!(is_pre_2026_analysis_params(tmp.path()).unwrap(), true);
    }

    #[test]
    fn current_schema_tunable_less_method_only_stage_is_not_pre_2026() {
        // Regression for a detector false-positive: tunable-less methods
        // (cycle_combine, vfs_computation, quality_gate, eccentricity)
        // serialize as method-only in the CURRENT schema. The detector must
        // NOT flag these as legacy, or re-analysis of valid files would be
        // wrongly refused with "run migrate first".
        let tmp = TempFile::new("pre2026_method_only");
        create(tmp.path(), "test").unwrap();
        let tree = serde_json::json!({
            "cycle_combine": { "method": "marshel_garrett2011_delay_subtraction" },
            "vfs_computation": { "method": "open_isi_chain_rule_phasor_gradient" }
        });
        write_analysis_params_attr(tmp.path(), &tree).unwrap();
        assert_eq!(is_pre_2026_analysis_params(tmp.path()).unwrap(), false);
    }

    /// End-to-end: a pre-2026 `.oisi` → detect → migrate → write back →
    /// strict reload → bridge. This is the only test exercising the full
    /// chain, and it guards the interaction with the now-fail-loud
    /// `from_json_tree` reader: a migrated tree MUST reconstruct into an
    /// `AnalysisParams` without missing/unknown-key errors.
    #[test]
    fn pre_2026_file_migrates_then_reconstructs_for_analysis() {
        use openisi_params::{PersistTarget, RegistrySnapshot};

        let tmp = TempFile::new("migrate_e2e");
        create(tmp.path(), "raw_acquisition").unwrap();

        // Old-schema /analysis_params: a moved root field (dropped on
        // migrate) + a tagged-enum stage carrying a stage-level tunable.
        let old = serde_json::json!({
            "azi_angular_range": 120.0,
            "phase_smoothing": {
                "method": "open_isi_amp_weighted_phasor",
                "sigma_px": 2.5
            }
        });
        write_analysis_params_attr(tmp.path(), &old).unwrap();
        assert!(
            is_pre_2026_analysis_params(tmp.path()).unwrap(),
            "old-schema file should be detected as pre-2026"
        );

        // Migrate: translate + write back.
        let new_tree = crate::migrate::translate_pre_2026_analysis_params(&old).unwrap();
        write_analysis_params_attr(tmp.path(), &new_tree).unwrap();
        assert!(
            !is_pre_2026_analysis_params(tmp.path()).unwrap(),
            "migrated file should be current-schema"
        );

        // Reconstruct through the FAIL-LOUD reader → bridge. If migration
        // produced an incomplete or unknown-keyed tree, from_json_tree
        // would error here.
        let tree = read_analysis_params_attr(tmp.path()).unwrap().unwrap();
        let snap = RegistrySnapshot::from_json_tree(PersistTarget::Analysis, &tree)
            .expect("migrated tree must pass the strict reader");
        // Constructing AnalysisParams via the bridge == the file is analyzable.
        let _params = crate::bridge::analysis_params_from_snapshot(&snap);

        // The migrated override survived the whole round-trip; the moved
        // field was dropped.
        assert_eq!(
            tree["phase_smoothing"]["open_isi_amp_weighted_phasor"]["sigma_px"],
            serde_json::json!(2.5)
        );
        assert!(tree.get("azi_angular_range").is_none());
    }
}
