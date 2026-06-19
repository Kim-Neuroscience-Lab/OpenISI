//! Analysis-semantic `.oisi` I/O — the readers/writers keyed by *analysis*
//! vocabulary (`/results/*`, the incremental `/cache` + stage fingerprints,
//! `/analysis_params`, file introspection, and the raw→complex stage glue).
//!
//! The format itself — the HDF5 structure, the schema (single source of truth),
//! and the format-pure primitives (open/attr/`create`/`atomic_update`/raw +
//! complex-map + anatomical read/write) — lives in the light [`oisi`] crate
//! ([`oisi::schema`] for the on-disk layout, [`oisi::io`] for the primitives).
//! The functions here **compose** those primitives; an `oisi::OisiError`
//! auto-converts to [`crate::AnalysisError`] through the crate-root `From` impl.

use hdf5::File as H5File;
use ndarray::{Array1, Array2};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use crate::oisi_schema::name;
use crate::{
    AcquisitionProperties, AnalysisError, AnalysisParams, AnalysisResult, ProgressSink,
    RawProcessingResult,
};

// The format-pure primitives — file open, attribute read, group member
// listing — live in the light `oisi` crate. The analysis-semantic
// readers/writers below call them; an `oisi` error auto-converts to
// `AnalysisError` through the crate-root `From` impl.
use oisi::io::{
    list_group_members_from_group, open_read, open_readwrite, read_f64_attr, read_sweep_sequence,
};

// The format-layer I/O the analysis crate composes — re-exported on
// `isi_analysis::io::*` so external callers (src-tauri export, headless,
// tests, examples) keep their existing paths. These are pure-format moves;
// the analysis-semantic functions in THIS module wrap them with vocabulary
// the format layer doesn't own (Direction, AnalysisResult, the cache). The
// attribute/dataset writers (`write_*`) double as the internal primitives the
// kept code uses, so they're brought into scope by these `pub use`s.
pub use oisi::import_snlc_directory;
pub use oisi::io::{
    atomic_update, create, read_acquisition_identity, read_anatomical, read_complex_maps,
    read_cortex_roi, read_experiment_params, read_raw_acquisition, read_rig_params,
    strip_derived_outputs, verify_format_version, write_anatomical, write_checked_1d,
    write_complex_maps, write_f64_attr, write_str_attr, write_u32_attr, FORMAT_VERSION,
};
// `AcquisitionIdentity` / `RawAcquisition` etc. now live at the crate root
// (`pub use oisi::{...}` in lib.rs); the analysis-semantic readers below refer
// to them as `crate::X`.

// Per-dataset rendering metadata (the `/results/*` palette/units contract) is a
// distinct concern, split into a submodule. It reaches the shared low-level
// HDF5 helpers via `oisi::io::*`.
mod meta;
pub use meta::{classify_result_type, meta_for_f64, read_map_meta, MapMeta};
use meta::{attach_meta, map_meta_bool, map_meta_labels};

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

/// Summary of an acquisition's stimulus schedule, derived from the single
/// source of truth — `/acquisition/schedule`'s `sweep_sequence` — the same
/// schedule the DFT groups by direction. `cycles_per_direction` is the
/// repetition count the analysis actually averages over.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ScheduleSummary {
    /// Total sweeps in the schedule (all directions).
    pub total_sweeps: usize,
    /// Number of distinct, recognized stimulus directions.
    pub directions: usize,
    /// Cycles (repetitions) per direction — the standard protocol runs the
    /// same count for each; this is the minimum if they differ.
    pub cycles_per_direction: usize,
}

/// What's present in an .oisi file.
pub struct FileCapabilities {
    pub has_anatomical: bool,
    pub has_acquisition: bool,
    pub has_complex_maps: bool,
    pub has_results: bool,
    /// Map dimensions (H, W) — from whichever data is present
    pub dimensions: Option<(usize, usize)>,
    /// Stimulus schedule summary (sweeps / directions / cycles-per-direction),
    /// derived from `/acquisition/schedule`. `None` if there's no schedule.
    pub acquisition_schedule: Option<ScheduleSummary>,
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

    // Stimulus schedule summary — the real cycle count, read from the same
    // `/acquisition/schedule` `sweep_sequence` the DFT groups by direction
    // (one source of truth, not a separate count).
    let acquisition_schedule = if has_acquisition {
        schedule_summary(&file)
    } else {
        None
    };

    // Classify each result dataset by type.
    let results = if has_results {
        let group = file.group("results").ok();
        if let Some(g) = group {
            let names = list_group_members_from_group(&g)?;
            names
                .into_iter()
                .filter_map(|name| {
                    if let Ok(ds) = g.dataset(&name) {
                        let shape = ds.shape();
                        let dtype = ds.dtype().ok();
                        let result_type = classify_result_type(&name, &shape, dtype.as_ref());
                        Some(ResultInfo { name, result_type })
                    } else {
                        None // skip sub-groups
                    }
                })
                .collect()
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
        acquisition_schedule,
        results,
    })
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

/// Read the cached retinotopy maps from `/results`, for the incremental cache's
/// retinotopy restore point. Returns `Ok(None)` if any required dataset is
/// absent (a cache miss — `magnification_raw` in particular is only present in
/// results written by the current pipeline). The caller restores these only
/// after confirming the stored retinotopy fingerprint matches.
pub fn read_retinotopy_maps(path: &Path) -> Result<Option<crate::RetinotopyMaps>, AnalysisError> {
    let r = |name: &str| read_result_map(path, name);
    match (
        r("azi_phase"),
        r("alt_phase"),
        r("azi_phase_degrees"),
        r("alt_phase_degrees"),
        r("azi_amplitude"),
        r("alt_amplitude"),
        r("vfs"),
        r("magnification_raw"),
        r("magnification_axis"),
        r("magnification_distortion"),
    ) {
        (
            Ok(azi_phase),
            Ok(alt_phase),
            Ok(azi_phase_degrees),
            Ok(alt_phase_degrees),
            Ok(azi_amplitude),
            Ok(alt_amplitude),
            Ok(vfs),
            Ok(magnification_raw),
            Ok(magnification_axis),
            Ok(magnification_distortion),
        ) => Ok(Some(crate::RetinotopyMaps {
            azi_phase,
            alt_phase,
            azi_phase_degrees,
            alt_phase_degrees,
            azi_amplitude,
            alt_amplitude,
            vfs,
            magnification_raw,
            magnification_axis,
            magnification_distortion,
            // Delay maps are method-conditional (absent under unweighted
            // combine, or in files from before the leaf existed). Read as
            // optional — their absence does NOT defeat the retinotopy restore.
            azi_delay: read_result_map(path, "azi_delay").ok(),
            alt_delay: read_result_map(path, "alt_delay").ok(),
        })),
        _ => Ok(None),
    }
}

/// Read a stage's stored fingerprint from `/analysis_state@<stage>`. `Ok(None)`
/// if the group or attribute is absent (no cache to compare against).
pub fn read_stage_fingerprint(path: &Path, stage: &str) -> Result<Option<String>, AnalysisError> {
    let file = open_read(path)?;
    let Ok(group) = file.group("analysis_state") else {
        return Ok(None);
    };
    // Distinguish "no fingerprint for this stage yet" (benign → None, recompute)
    // from "the fingerprint attribute is present but won't open" (corruption →
    // loud error, not silently swallowed as a cache miss).
    let present = group
        .attr_names()
        .map(|names| names.iter().any(|n| n == stage))
        .unwrap_or(false);
    match group.attr(stage) {
        Ok(a) => {
            let s = a
                .read_scalar::<hdf5::types::VarLenUnicode>()
                .map_err(|e| AnalysisError::hdf5(format!("reading fingerprint {stage}"), e))?;
            Ok(Some(s.as_str().to_string()))
        }
        Err(e) if present => Err(AnalysisError::hdf5(
            format!("fingerprint {stage} present but unreadable (corrupt /analysis_state)"),
            e,
        )),
        Err(_) => Ok(None),
    }
}

/// Record a stage's fingerprint at `/analysis_state@<stage>` — the identity of
/// the inputs that produced the cached output, so the next run can decide
/// whether to restore or recompute.
pub fn write_stage_fingerprint(path: &Path, stage: &str, fp: &str) -> Result<(), AnalysisError> {
    let file = open_readwrite(path)?;
    let group = match file.group(name::ANALYSIS_STATE) {
        Ok(g) => g,
        Err(_) => file
            .create_group(name::ANALYSIS_STATE)
            .map_err(|e| AnalysisError::hdf5("creating analysis_state group", e))?,
    };
    write_str_attr(&group, stage, fp)?;
    Ok(())
}

/// Read every stored stage fingerprint at once (the `/analysis_state` group's
/// attributes, `stage → key`). One file open instead of one per stage. An empty
/// map means no cache has been written (or the group is absent).
pub fn read_all_stage_fingerprints(path: &Path) -> Result<HashMap<String, String>, AnalysisError> {
    let file = open_read(path)?;
    let Ok(group) = file.group("analysis_state") else {
        return Ok(HashMap::new());
    };
    let names = group
        .attr_names()
        .map_err(|e| AnalysisError::hdf5("listing analysis_state attrs", e))?;
    let mut out = HashMap::with_capacity(names.len());
    for name in names {
        match group
            .attr(&name)
            .and_then(|a| a.read_scalar::<hdf5::types::VarLenUnicode>())
        {
            Ok(s) => {
                out.insert(name, s.as_str().to_string());
            }
            // A fingerprint we can't read is a corrupt cache entry: treat it as
            // absent (the stage recomputes — safe, never stale) but say so, so a
            // damaged file doesn't silently masquerade as a cold cache.
            Err(e) => {
                tracing::warn!(attr = %name, error = %e, "unreadable stage fingerprint — recomputing")
            }
        }
    }
    Ok(out)
}

/// Which cached stage artifacts are physically present on disk, by stage. The
/// incremental cut consults this so a fingerprint match on a stage whose data
/// was stripped (or never written, e.g. a pre-`/cache` file) still recomputes
/// rather than trying to restore absent data. Each field is `true` iff **all**
/// datasets that stage's restore reads exist.
#[derive(Debug, Clone, Copy, Default)]
pub struct StageArtifacts {
    pub retinotopy: bool,
    pub sign_smoothing: bool,
    pub cortex_source: bool,
    pub patch_threshold: bool,
    pub labels: bool,
    pub eccentricity: bool,
    pub derived_maps: bool,
}

/// Probe the file once for every cacheable tail stage's restore artifacts.
pub fn stage_artifacts_present(path: &Path) -> Result<StageArtifacts, AnalysisError> {
    let file = open_read(path)?;
    let has = |p: &str| file.link_exists(p);
    let all = |ps: &[&str]| ps.iter().all(|p| has(p));

    // `/cache`: the patch-threshold intermediates — `imseg` dataset + the
    // `threshold_applied` scalar carried as an attribute on the group. Resolve
    // the group once and check both from it.
    let patch_threshold = file
        .group("cache")
        .map(|g| g.link_exists("imseg") && g.attr("threshold_applied").is_ok())
        .unwrap_or(false);

    Ok(StageArtifacts {
        retinotopy: all(&[
            "results/azi_phase",
            "results/alt_phase",
            "results/azi_phase_degrees",
            "results/alt_phase_degrees",
            "results/azi_amplitude",
            "results/alt_amplitude",
            "results/vfs",
            "results/magnification_raw",
            "results/magnification_axis",
            "results/magnification_distortion",
        ]),
        sign_smoothing: has("results/vfs_smoothed"),
        cortex_source: has("results/cortex_mask"),
        patch_threshold,
        labels: all(&[
            "results/area_labels",
            "results/area_signs",
            "results/area_borders",
        ]),
        // Both outputs must be present to restore the stage; an old file with
        // `eccentricity` but no `polar_angle` recomputes (cheap) rather than
        // restoring a partial Eccentricity stage.
        eccentricity: all(&["results/eccentricity", "results/polar_angle"]),
        derived_maps: all(&[
            "results/magnification",
            "results/contours_azi",
            "results/contours_alt",
            "results/vfs_smoothed_thresholded",
        ]),
    })
}

/// Persist the two non-result `PatchThreshold` intermediates to `/cache` so a
/// later run can restore that stage instead of recomputing the threshold. The
/// binary candidate mask (`imseg`) is the dataset; the applied scalar threshold
/// is a group attribute. Rewritten wholesale each time the stage executes;
/// `read_stage_fingerprint(path, "patch_threshold")` is what gates reuse.
pub fn write_stage_cache(
    path: &Path,
    imseg: &Array2<bool>,
    threshold_applied: f64,
) -> Result<(), AnalysisError> {
    let file = open_readwrite(path)?;
    let _ = file.unlink(name::CACHE);
    let group = file
        .create_group(name::CACHE)
        .map_err(|e| AnalysisError::hdf5("creating cache group", e))?;
    let u8data = imseg.mapv(|b| b as u8);
    group
        .new_dataset_builder()
        .with_data(&u8data)
        .create(name::IMSEG)
        .map_err(|e| AnalysisError::hdf5("writing cache/imseg", e))?;
    write_f64_attr(&group, name::THRESHOLD_APPLIED, threshold_applied)?;
    Ok(())
}

/// Read the cached binary candidate-patch mask (`/cache/imseg`).
pub fn read_cache_imseg(path: &Path) -> Result<Array2<bool>, AnalysisError> {
    let file = open_read(path)?;
    let ds = file
        .dataset("cache/imseg")
        .map_err(|e| AnalysisError::MissingData(format!("cache/imseg: {e}")))?;
    let data: Array2<u8> = ds
        .read()
        .map_err(|e| AnalysisError::hdf5("reading cache/imseg", e))?;
    Ok(data.mapv(|v| v != 0))
}

/// Read the cached applied `|VFS|` threshold (`/cache@threshold_applied`).
pub fn read_cache_threshold(path: &Path) -> Result<f64, AnalysisError> {
    let file = open_read(path)?;
    let group = file
        .group("cache")
        .map_err(|e| AnalysisError::MissingData(format!("cache: {e}")))?;
    read_f64_attr(&group, "threshold_applied")
        .ok_or_else(|| AnalysisError::MissingData("cache@threshold_applied".into()))
}

/// Read cached spectral responsiveness maps if all four datasets
/// (`/results/{spectral_snr,allen_power_snr}_{azi,alt}`) exist; returns
/// `Ok(None)` if any is missing (cache miss, not an error — e.g. a file
/// analyzed under the old `snr_*` schema). Used by the boundary to seed the
/// responsiveness maps when the cached complex maps are reused; both metrics are
/// parameterless on the raw acquisition, so cached values are correct as long as
/// the raw frames + baseline haven't changed (gated by the projection fingerprint).
pub fn read_responsiveness_maps(
    path: &Path,
) -> Result<Option<crate::ResponsivenessMaps>, AnalysisError> {
    match (
        read_result_map(path, "spectral_snr_azi"),
        read_result_map(path, "spectral_snr_alt"),
        read_result_map(path, "allen_power_snr_azi"),
        read_result_map(path, "allen_power_snr_alt"),
    ) {
        (Ok(spectral_snr_azi), Ok(spectral_snr_alt), Ok(allen_power_snr_azi), Ok(allen_power_snr_alt)) => {
            Ok(Some(crate::ResponsivenessMaps {
                spectral_snr_azi,
                spectral_snr_alt,
                allen_power_snr_azi,
                allen_power_snr_alt,
            }))
        }
        _ => Ok(None),
    }
}

/// Read cached per-direction reliability maps; returns `Ok(None)` if
/// any of the four datasets is missing (e.g. K=1 capture where
/// reliability is undefined). Companion to `read_responsiveness_maps`.
pub fn read_reliability_maps(path: &Path) -> Result<Option<crate::ReliabilityMaps>, AnalysisError> {
    match (
        read_result_map(path, "reliability_azi_fwd"),
        read_result_map(path, "reliability_azi_rev"),
        read_result_map(path, "reliability_alt_fwd"),
        read_result_map(path, "reliability_alt_rev"),
    ) {
        (Ok(rel_azi_fwd), Ok(rel_azi_rev), Ok(rel_alt_fwd), Ok(rel_alt_rev)) => {
            Ok(Some(crate::ReliabilityMaps {
                rel_azi_fwd,
                rel_azi_rev,
                rel_alt_fwd,
                rel_alt_rev,
            }))
        }
        _ => Ok(None),
    }
}

/// Read a single result map by name (e.g. "azi_phase", "vfs").
pub fn read_result_map(path: &Path, name: &str) -> Result<Array2<f64>, AnalysisError> {
    let file = open_read(path)?;
    let ds_path = format!("results/{name}");
    let ds = file
        .dataset(&ds_path)
        .map_err(|e| AnalysisError::MissingData(format!("{ds_path}: {e}")))?;
    let data: Array2<f64> = ds
        .read()
        .map_err(|e| AnalysisError::hdf5(format!("reading {ds_path}"), e))?;
    Ok(data)
}

/// Read a `/results` boolean mask (stored `u8` 0/1) by name — the read half of
/// the masks `write_results` writes (`cortex_mask`, `area_borders`,
/// `contours_*`). Used by the incremental cache to restore a skipped stage's
/// mask output.
pub fn read_result_mask(path: &Path, name: &str) -> Result<Array2<bool>, AnalysisError> {
    let file = open_read(path)?;
    let ds_path = format!("results/{name}");
    let ds = file
        .dataset(&ds_path)
        .map_err(|e| AnalysisError::MissingData(format!("{ds_path}: {e}")))?;
    let data: Array2<u8> = ds
        .read()
        .map_err(|e| AnalysisError::hdf5(format!("reading {ds_path}"), e))?;
    Ok(data.mapv(|v| v != 0))
}

/// Read the `/results/area_labels` integer label map (the `Labels` stage output)
/// for the incremental cache.
pub fn read_result_labels(path: &Path) -> Result<Array2<i32>, AnalysisError> {
    let file = open_read(path)?;
    let ds = file
        .dataset("results/area_labels")
        .map_err(|e| AnalysisError::MissingData(format!("results/area_labels: {e}")))?;
    ds.read()
        .map_err(|e| AnalysisError::hdf5("reading results/area_labels", e))
}

/// Read the `/results/area_signs` per-area sign array (the `Labels` stage
/// output) for the incremental cache.
pub fn read_result_signs(path: &Path) -> Result<Vec<i8>, AnalysisError> {
    let file = open_read(path)?;
    let ds = file
        .dataset("results/area_signs")
        .map_err(|e| AnalysisError::MissingData(format!("results/area_signs: {e}")))?;
    let arr: Array1<i8> = ds
        .read_1d()
        .map_err(|e| AnalysisError::hdf5("reading results/area_signs", e))?;
    Ok(arr.to_vec())
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

/// Write the `.oisi /analysis_params` attribute from a tagged-`AnalysisConfig`
/// JSON value (the shape produced by `serde_json::to_value(&AnalysisConfig)`).
///
/// This function owns only persistence of the params tree to the `.oisi` file;
/// it is agnostic to how the tree was produced. The analysis crate has no notion
/// of a config store — callers serialize the typed `AnalysisConfig`.
///
/// **Atomicity note:** HDF5 attribute rewrite is in-place on an
/// existing file. Crash-during-write leaves the file with the old
/// attribute; parallel writers to the same file are unsafe regardless.
pub fn write_analysis_params_attr(
    path: &Path,
    params_tree: &serde_json::Value,
) -> Result<(), AnalysisError> {
    let file = open_readwrite(path)?;
    let json = serde_json::to_string(params_tree)
        .map_err(|e| AnalysisError::Io(std::io::Error::other(e)))?;
    write_str_attr(&file, name::ANALYSIS_PARAMS, &json)?;
    Ok(())
}

/// Return true iff the file's `/analysis_params` JSON attribute uses a
/// pre-2026 schema. The current schema is the tagged `AnalysisConfig`
/// (`"<stage>": {"method": "...", <active tunables>}`); a file is pre-2026 iff
/// its tree does **not** deserialize into `AnalysisConfig`. That single check
/// subsumes every legacy form: the registry/nested-subtree shape (tunables
/// nested under a variant key fail the flat-field deserialize), root-level
/// moved fields (`azi_angular_range`, … → unknown-field on the outer struct),
/// and renamed/legacy method strings (invalid enum tag). Used by the
/// orchestrator to refuse old-schema files with a "run `oisi migrate`" message.
///
/// Returns `Ok(false)` when the attribute is absent or already current;
/// returns `Err` only on HDF5 / I/O failure.
pub fn is_pre_2026_analysis_params(path: &Path) -> Result<bool, AnalysisError> {
    let Some(value) = read_analysis_params_attr(path)? else {
        return Ok(false);
    };
    Ok(serde_json::from_value::<openisi_params::config::AnalysisConfig>(value).is_err())
}

/// Read the `/analysis_params` HDF5 attribute as a raw
/// `serde_json::Value` (the tagged-`AnalysisConfig` shape). Returns `None` if
/// the attribute is absent (file never analyzed); returns `Err` only
/// on HDF5 / parse failure.
///
/// Callers that need an `AnalysisParams` pass the value through
/// [`crate::bridge::analysis_params_from_oisi_tree`], which deserializes the
/// tagged `AnalysisConfig` and adapts it (fail-loud on a legacy schema).
pub fn read_analysis_params_attr(path: &Path) -> Result<Option<serde_json::Value>, AnalysisError> {
    let file = open_read(path)?;
    let attr_names = file
        .attr_names()
        .map_err(|e| AnalysisError::hdf5("listing root attrs", e))?;
    if !attr_names.iter().any(|n| n == "analysis_params") {
        return Ok(None);
    }
    let attr = file
        .attr("analysis_params")
        .map_err(|e| AnalysisError::hdf5("opening analysis_params attr", e))?;
    let json_vlu: hdf5::types::VarLenUnicode = attr
        .read_scalar()
        .map_err(|e| AnalysisError::hdf5("reading analysis_params attr", e))?;
    let value: serde_json::Value = serde_json::from_str(json_vlu.as_str())
        .map_err(|e| AnalysisError::InvalidPackage(format!("parsing analysis_params: {e}")))?;
    Ok(Some(value))
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

/// Write all analysis results as flat datasets in `/results/`. No sub-groups.
///
/// **Does NOT write `/analysis_params`** — that's the orchestrator's
/// responsibility via `write_analysis_params_attr` with the tagged
/// `AnalysisConfig` JSON. Keeping them separate avoids `AnalysisParams`
/// needing to carry a serde representation; the tagged `AnalysisConfig` is the
/// canonical on-disk form, and only the orchestrator (which owns the config)
/// can produce it.
pub fn write_results(
    path: &Path,
    result: &AnalysisResult,
    acquisition: &AcquisitionProperties,
    _params: &AnalysisParams,
) -> Result<(), AnalysisError> {
    let file = open_readwrite(path)?;

    // Remove and recreate results group (flat).
    let _ = file.unlink(name::RESULTS);
    let group = file
        .create_group(name::RESULTS)
        .map_err(|e| AnalysisError::hdf5("creating results group", e))?;

    // Writes a f64 (H,W) dataset and attaches the meta attrs that
    // describe how to render it. The renderer reads these attrs and
    // does ZERO inference — palette, units, display range, NaN/zero
    // semantics are all decided here, once, at write time.
    let write_f64 = |name: &str, data: &Array2<f64>| -> Result<(), AnalysisError> {
        let ds = group
            .new_dataset_builder()
            .with_data(data)
            .create(name)
            .map_err(|e| AnalysisError::hdf5(format!("writing results/{name}"), e))?;
        let meta = meta_for_f64(name, data, acquisition);
        attach_meta(&ds, &meta)?;
        Ok(())
    };
    let write_mask = |name: &str, data: &Array2<bool>| -> Result<(), AnalysisError> {
        let u8data = data.mapv(|b| b as u8);
        let ds = group
            .new_dataset_builder()
            .with_data(&u8data)
            .create(name)
            .map_err(|e| AnalysisError::hdf5(format!("writing results/{name}"), e))?;
        attach_meta(&ds, &map_meta_bool())?;
        Ok(())
    };

    // Phases, amplitudes, and the three VFS algorithm stages.
    // `vfs` is the raw mathematical VFS; `vfs_smoothed` is the
    // smoothed array segmentation operated on; `vfs_smoothed_thresholded`
    // is the literal threshold mask. All full frame — no cortex
    // masking pre-baked.
    write_f64(name::AZI_PHASE, &result.azi_phase)?;
    write_f64(name::ALT_PHASE, &result.alt_phase)?;
    write_f64(name::AZI_PHASE_DEGREES, &result.azi_phase_degrees)?;
    write_f64(name::ALT_PHASE_DEGREES, &result.alt_phase_degrees)?;
    write_f64(name::AZI_AMPLITUDE, &result.azi_amplitude)?;
    write_f64(name::ALT_AMPLITUDE, &result.alt_amplitude)?;
    write_f64(name::VFS, &result.vfs)?;
    write_f64(name::VFS_SMOOTHED, &result.vfs_smoothed)?;
    write_f64(name::VFS_SMOOTHED_THRESHOLDED, &result.vfs_smoothed_thresholded)?;

    // Segmentation outputs.
    write_mask(name::CORTEX_MASK, &result.cortex_mask)?;
    let labels_ds = group
        .new_dataset_builder()
        .with_data(&result.area_labels)
        .create(name::AREA_LABELS)
        .map_err(|e| AnalysisError::hdf5("writing results/area_labels", e))?;
    attach_meta(&labels_ds, &map_meta_labels())?;
    let signs_arr = ndarray::Array1::from(result.area_signs.clone());
    group
        .new_dataset_builder()
        .with_data(&signs_arr)
        .create(name::AREA_SIGNS)
        .map_err(|e| AnalysisError::hdf5("writing results/area_signs", e))?;
    write_mask(name::AREA_BORDERS, &result.area_borders)?;

    // Derived maps.
    write_f64(name::ECCENTRICITY, &result.eccentricity)?;
    write_f64(name::POLAR_ANGLE, &result.polar_angle)?;
    write_f64(name::MAGNIFICATION, &result.magnification)?;
    // Unmasked Jacobian magnitude — persisted as a retinotopy restore input
    // (and a legitimate raw output). Read back by `read_retinotopy_maps`.
    write_f64(name::MAGNIFICATION_RAW, &result.magnification_raw)?;
    // Magnification anisotropy (SNLC getMagFactors) — restored with retinotopy.
    write_f64(name::MAGNIFICATION_AXIS, &result.magnification_axis)?;
    write_f64(name::MAGNIFICATION_DISTORTION, &result.magnification_distortion)?;
    write_mask(name::CONTOURS_AZI, &result.contours_azi)?;
    write_mask(name::CONTOURS_ALT, &result.contours_alt)?;

    // Hemodynamic delay maps (SNLC Gprocesskret delay_hor/_vert) — present only
    // under delay-subtraction cycle-combine, so written conditionally.
    if let Some(ref d) = result.azi_delay {
        write_f64(name::AZI_DELAY, d)?;
    }
    if let Some(ref d) = result.alt_delay {
        write_f64(name::ALT_DELAY, d)?;
    }

    if let Some(ref r) = result.responsiveness {
        write_f64(name::SPECTRAL_SNR_AZI, &r.spectral_snr_azi)?;
        write_f64(name::SPECTRAL_SNR_ALT, &r.spectral_snr_alt)?;
        write_f64(name::ALLEN_POWER_SNR_AZI, &r.allen_power_snr_azi)?;
        write_f64(name::ALLEN_POWER_SNR_ALT, &r.allen_power_snr_alt)?;
    }

    // Per-direction cross-cycle reliability (Allen / Engel). Source of
    // truth for the cortex mask above; persisted so the user (or a
    // future reanalysis) can re-derive cortex with a different threshold
    // without rerunning the raw pipeline.
    if let Some(ref rel) = result.reliability {
        write_f64(name::RELIABILITY_AZI_FWD, &rel.rel_azi_fwd)?;
        write_f64(name::RELIABILITY_AZI_REV, &rel.rel_azi_rev)?;
        write_f64(name::RELIABILITY_ALT_FWD, &rel.rel_alt_fwd)?;
        write_f64(name::RELIABILITY_ALT_REV, &rel.rel_alt_rev)?;
    }

    let area_count = result.area_signs.len();
    if area_count > 0 {
        tracing::info!(areas = area_count, "areas segmented");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Streaming raw frame processing
// ---------------------------------------------------------------------------

/// Complex maps from a path — thin shim over [`read_raw_acquisition`] + the
/// `Baseline` (F0) + `Projection` (DFT) stages. Retained for callers that want
/// the complex maps directly from a file (regression tests, the headless
/// phase-spread diagnostic); the production pipeline runs the same two stages
/// from the I/O boundary (`analyze`).
pub fn compute_complex_maps_from_raw(
    path: &Path,
    params: &AnalysisParams,
    progress: &dyn ProgressSink,
    cancel: &AtomicBool,
) -> Result<RawProcessingResult, AnalysisError> {
    progress.set_stage("Loading camera frames");
    progress.set_progress(0.0);
    let raw = read_raw_acquisition(path)?;
    use crate::methods::BaselineExt;
    let base = params.baseline.apply(&raw);
    crate::compute::projection::run(
        &raw,
        &base.f0,
        base.floor,
        &params.response_normalization,
        &params.rectification,
        &params.cycle_average,
        cancel,
        progress,
    )
}

/// Binary search for the index of the element in `sorted` closest to `target`.
/// Assumes `sorted` is non-empty and non-decreasing.
pub(crate) fn nearest_index_sorted(sorted: &[f64], target: f64) -> usize {
    match sorted.binary_search_by(|v| v.partial_cmp(&target).unwrap_or(std::cmp::Ordering::Equal)) {
        Ok(i) => i,
        Err(insert_at) => {
            if insert_at == 0 {
                0
            } else if insert_at >= sorted.len() {
                sorted.len() - 1
            } else {
                let lo = insert_at - 1;
                let hi = insert_at;
                if (target - sorted[lo]).abs() <= (sorted[hi] - target).abs() {
                    lo
                } else {
                    hi
                }
            }
        }
    }
}

/// Summarize the stimulus schedule — total sweeps, distinct directions, and
/// cycles (repetitions) per direction — from the `sweep_sequence` SSoT,
/// counting directions the same way the DFT does (`classify_cycle_name`).
/// Returns `None` when there's no readable schedule (e.g. a complex-maps-only
/// import) or no recognized directions.
fn schedule_summary(file: &H5File) -> Option<ScheduleSummary> {
    let sweep_sequence = read_sweep_sequence(file).ok()?;
    let total_sweeps = sweep_sequence.len();
    if total_sweeps == 0 {
        return None;
    }
    let mut per_dir: std::collections::BTreeMap<crate::compute::Direction, usize> =
        std::collections::BTreeMap::new();
    for name in &sweep_sequence {
        if let Some(direction) = classify_cycle_name(name) {
            *per_dir.entry(direction).or_default() += 1;
        }
    }
    if per_dir.is_empty() {
        return None;
    }
    Some(ScheduleSummary {
        total_sweeps,
        directions: per_dir.len(),
        cycles_per_direction: per_dir.values().copied().min().unwrap_or(0),
    })
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
pub(crate) fn classify_cycle_name(name: &str) -> Option<crate::compute::Direction> {
    use crate::compute::Direction;
    let lower = name.to_lowercase();
    if lower.starts_with("lr") {
        Some(Direction::AziFwd)
    } else if lower.starts_with("rl") {
        Some(Direction::AziRev)
    } else if lower.starts_with("tb") {
        Some(Direction::AltFwd)
    }
    // absorbs camera vertical flip
    else if lower.starts_with("bt") {
        Some(Direction::AltRev)
    }
    // absorbs camera vertical flip
    else if lower.starts_with("ccw") {
        Some(Direction::AziRev)
    }
    // wedge counter-clockwise → azimuth rev (check ccw before cw)
    else if lower.starts_with("cw") {
        Some(Direction::AziFwd)
    }
    // wedge clockwise → azimuth fwd
    else if lower.starts_with("expand") {
        Some(Direction::AltFwd)
    }
    // ring expand → altitude fwd
    else if lower.starts_with("contract") {
        Some(Direction::AltRev)
    }
    // ring contract → altitude rev
    else {
        None
    }
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
    /// writing results. Construction flows from the typed `AnalysisConfig`
    /// defaults via the `From` adapter, the same path production uses.
    fn test_params() -> crate::AnalysisParams {
        crate::AnalysisParams::from(&openisi_params::config::AnalysisConfig::default())
    }

    /// Zeroed complex maps for `AnalysisResult` round-trip fixtures (the field
    /// is persisted to `/complex_maps`; these tests only exercise `/results`).
    fn zeros_complex_maps(h: usize, w: usize) -> crate::ComplexMaps {
        let z = || Array2::<num_complex::Complex64>::zeros((h, w));
        crate::ComplexMaps {
            azi_fwd: z(),
            azi_rev: z(),
            alt_fwd: z(),
            alt_rev: z(),
        }
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
    fn make_complex_maps(h: usize, w: usize) -> crate::ComplexMaps {
        let make = |scale: f64| -> Array2<Complex64> {
            Array2::from_shape_fn((h, w), |(r, c)| {
                Complex64::new((r as f64 + 1.0) * scale, (c as f64 + 1.0) * scale * 0.5)
            })
        };
        crate::ComplexMaps {
            azi_fwd: make(1.0),
            azi_rev: make(2.0),
            alt_fwd: make(3.0),
            alt_rev: make(4.0),
        }
    }

    // 2. Results write + read round-trip
    // -------------------------------------------------------------------------

    #[test]
    fn results_round_trip() {
        let tmp = TempFile::new("results_rt");
        let (h, w) = (8, 8);
        let params = test_params();

        let result = crate::AnalysisResult {
            complex_maps: zeros_complex_maps(h, w),
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
            polar_angle: Array2::zeros((h, w)),
            magnification: Array2::zeros((h, w)),
            magnification_raw: Array2::zeros((h, w)),
            magnification_axis: Array2::zeros((h, w)),
            magnification_distortion: Array2::zeros((h, w)),
            contours_azi: Array2::from_elem((h, w), false),
            contours_alt: Array2::from_elem((h, w), false),
            responsiveness: None,
            reliability: None,
            azi_delay: None,
            alt_delay: None,
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
            complex_maps: zeros_complex_maps(h, w),
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
            polar_angle: Array2::zeros((h, w)),
            magnification: Array2::zeros((h, w)),
            magnification_raw: Array2::zeros((h, w)),
            magnification_axis: Array2::zeros((h, w)),
            magnification_distortion: Array2::zeros((h, w)),
            contours_azi: Array2::from_elem((h, w), false),
            contours_alt: Array2::from_elem((h, w), false),
            responsiveness: None,
            reliability: None,
            azi_delay: None,
            alt_delay: None,
        };

        create(tmp.path(), "test").unwrap();
        write_results(tmp.path(), &result, &test_acquisition(), &params).unwrap();

        let caps = inspect(tmp.path()).unwrap();
        assert!(caps.has_results, "should detect results");
        assert!(!caps.has_complex_maps, "no complex_maps written");
        assert_eq!(caps.dimensions, Some((8, 8)));

        // Verify result classification.
        let names: Vec<&str> = caps.results.iter().map(|r| r.name.as_str()).collect();
        assert!(
            names.contains(&"azi_phase"),
            "results should list azi_phase"
        );
        assert!(names.contains(&"vfs"), "results should list vfs");
        assert!(
            names.contains(&"area_labels"),
            "results should list area_labels"
        );
    }

    // -------------------------------------------------------------------------
    // 5. import_snlc_directory() with missing files
    // -------------------------------------------------------------------------

    #[test]
    fn import_snlc_missing_mat_files() {
        let dir =
            std::env::temp_dir().join(format!("openisi_test_empty_dir_{}", std::process::id()));
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
    // 8. read_params round-trip
    // -------------------------------------------------------------------------

    #[test]
    fn params_round_trip() {
        let tmp = TempFile::new("params_rt");
        let params = test_params();
        let (h, w) = (4, 4);

        // write_results stores params as an attribute.
        let result = crate::AnalysisResult {
            complex_maps: zeros_complex_maps(h, w),
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
            polar_angle: Array2::zeros((h, w)),
            magnification: Array2::zeros((h, w)),
            magnification_raw: Array2::zeros((h, w)),
            magnification_axis: Array2::zeros((h, w)),
            magnification_distortion: Array2::zeros((h, w)),
            contours_azi: Array2::from_elem((h, w), false),
            contours_alt: Array2::from_elem((h, w), false),
            responsiveness: None,
            reliability: None,
            azi_delay: None,
            alt_delay: None,
        };

        create(tmp.path(), "test").unwrap();
        write_results(tmp.path(), &result, &test_acquisition(), &params).unwrap();

        // write_results no longer writes /analysis_params (the
        // orchestrator owns that via write_analysis_params_attr with
        // the config tree). Confirm the attribute is absent here.
        assert!(read_analysis_params_attr(tmp.path()).unwrap().is_none());

        // Then stamp a config tree and verify it round-trips.
        let tree = serde_json::json!({"cycle_combine": {"method": "kalatsky_stryker2003_delay_subtraction"}});
        write_analysis_params_attr(tmp.path(), &tree).unwrap();
        let loaded = read_analysis_params_attr(tmp.path()).unwrap().unwrap();
        assert_eq!(loaded, tree);
    }

    // ─────────────────────────────────────────────────────────────────
    // write_analysis_params_attr round-trip
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn write_analysis_params_attr_round_trips_via_read() {
        // Write a config-tree JSON value, read it back, verify equality.
        let tmp = TempFile::new("write_analysis_params");
        create(tmp.path(), "test").unwrap();

        let tree = serde_json::json!({
            "sign_map_smoothing": { "method": "gaussian", "gaussian": { "sigma_um": 77.0 } },
            "cortex_source":      { "method": "no_restriction" },
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
        assert!(!is_pre_2026_analysis_params(tmp.path()).unwrap());
    }

    #[test]
    fn is_pre_2026_analysis_params_current_schema_returns_false() {
        let tmp = TempFile::new("pre2026_current");
        create(tmp.path(), "test").unwrap();
        // Current schema: tagged `AnalysisConfig` — method + active tunable flat.
        let tree = serde_json::json!({
            "sign_map_smoothing": { "method": "gaussian", "sigma_um": 60.0 }
        });
        write_analysis_params_attr(tmp.path(), &tree).unwrap();
        assert!(!is_pre_2026_analysis_params(tmp.path()).unwrap());
    }

    #[test]
    fn is_pre_2026_analysis_params_old_schema_moved_fields_returns_true() {
        let tmp = TempFile::new("pre2026_old_moved");
        create(tmp.path(), "test").unwrap();
        let file = hdf5::File::open_rw(tmp.path()).unwrap();
        let stale_json = r#"{"azi_angular_range":120.0,"cycle_combine":{"method":"marshel_garrett2011_delay_subtraction"}}"#;
        write_str_attr(&file, "analysis_params", stale_json).unwrap();
        drop(file);
        assert!(is_pre_2026_analysis_params(tmp.path()).unwrap());
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
        assert!(is_pre_2026_analysis_params(tmp.path()).unwrap());
    }

    #[test]
    fn current_schema_tunable_less_method_only_stage_is_not_pre_2026() {
        // Regression for a detector false-positive: tunable-less methods
        // (cycle_combine, vfs_computation, eccentricity)
        // serialize as method-only in the CURRENT schema. The detector must
        // NOT flag these as legacy, or re-analysis of valid files would be
        // wrongly refused with "run migrate first".
        let tmp = TempFile::new("pre2026_method_only");
        create(tmp.path(), "test").unwrap();
        let tree = serde_json::json!({
            "cycle_combine": { "method": "kalatsky_stryker2003_delay_subtraction" },
            "vfs_computation": { "method": "open_isi_chain_rule_phasor_gradient" }
        });
        write_analysis_params_attr(tmp.path(), &tree).unwrap();
        assert!(!is_pre_2026_analysis_params(tmp.path()).unwrap());
    }

    /// End-to-end: a pre-2026 `.oisi` → detect → migrate → write back →
    /// reload → reconstruct. The migrated tree MUST deserialize into an
    /// `AnalysisParams` (via the tagged-`AnalysisConfig` reader) without error.
    #[test]
    fn pre_2026_file_migrates_then_reconstructs_for_analysis() {
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

        // Reconstruct through the tagged-config reader. If migration produced an
        // incomplete or unknown-keyed tree, this would error.
        let tree = read_analysis_params_attr(tmp.path()).unwrap().unwrap();
        let _params = crate::bridge::analysis_params_from_oisi_tree(&tree)
            .expect("migrated tree must reconstruct into AnalysisParams");

        // The migrated override survived (renamed to the SNLC method, tunable
        // flat at the stage level); the moved field was dropped.
        assert_eq!(
            tree["phase_smoothing"]["method"],
            serde_json::json!("snlc_amp_weighted_phasor")
        );
        assert_eq!(tree["phase_smoothing"]["sigma_px"], serde_json::json!(2.5));
        assert!(tree.get("azi_angular_range").is_none());
    }

}
