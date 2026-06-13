//! Analysis pipeline commands: run analysis, read results, export.

use tauri::State;

use crate::error::{AppError, AppResult};

use super::SharedState;

/// Return a human-readable label for the device the analysis pipeline
/// is using — e.g. `"CPU (Burn ndarray)"`. Read-only; the backend is
/// fixed at compile time (the single `Backend` alias) and there's no
/// override toggle. Used by the analysis sidebar to surface the compute
/// backend without the user having to read stderr.
#[tauri::command]
pub fn get_analysis_backend() -> AppResult<String> {
    Ok(isi_analysis::compute::backend_info())
}

/// Inspect a .oisi file — what data is present.
///
/// The `provenance` field reports the acquisition-properties provenance
/// (Full / Partial / Defaulted) so the UI can show a badge when the
/// file lacks complete `/rig_params` + `/experiment_params` capture
/// metadata (pre-2026-05-23 files, hand-built test files, etc.). See
/// `isi_analysis::ProvenanceLevel`.
#[tauri::command]
pub fn inspect_oisi(path: String) -> AppResult<serde_json::Value> {
    let p = std::path::Path::new(&path);
    let caps = isi_analysis::io::inspect(p)?;
    let rig = isi_analysis::io::read_rig_params(p).ok().flatten();
    let exp = isi_analysis::io::read_experiment_params(p).ok().flatten();
    let acq = isi_analysis::AcquisitionProperties::from_oisi_attrs(rig.as_ref(), exp.as_ref());
    Ok(serde_json::json!({
        "has_anatomical": caps.has_anatomical,
        "has_acquisition": caps.has_acquisition,
        "has_complex_maps": caps.has_complex_maps,
        "has_results": caps.has_results,
        "dimensions": caps.dimensions,
        "acquisition_schedule": caps.acquisition_schedule,
        "results": caps.results,
        "provenance": acq.provenance,
        "provenance_warning": acq.provenance.warning_summary(),
    }))
}

/// Run analysis on a .oisi file.
///
/// **Source of truth flow.** A fresh `RegistrySnapshot` is taken from
/// the current Registry, the bridge converts it to `AnalysisParams`,
/// the analysis runs, and the Registry tree is stamped into the
/// `.oisi /analysis_params` HDF5 attribute. Every value used in the
/// pipeline provably came from the SSoT param registry — no
/// `AnalysisParams::bootstrap()`, no inline defaults.
///
/// **Concurrency invariant:** `path` is taken as an explicit argument
/// (not read from the `active_oisi` lock mid-run).
#[tauri::command]
pub fn run_analysis(state: State<'_, SharedState>, path: String) -> AppResult<String> {
    let snapshot = state.registry.lock().snapshot();
    let analysis_tx = state.threads.analysis_tx.clone();

    let path_buf = std::path::PathBuf::from(&path);

    // Refuse pre-2026 schema files — the user must migrate explicitly.
    // Done up front (before send) so the caller sees the error
    // synchronously rather than as a delayed `analysis:failed` event.
    if isi_analysis::io::is_pre_2026_analysis_params(&path_buf)? {
        return Err(AppError::Validation(format!(
            "{} has pre-2026 /analysis_params schema. Run `oisi migrate {}` first.",
            path_buf.display(),
            path_buf.display(),
        )));
    }

    // The heavy work runs on the analysis worker thread; this command
    // returns immediately so the IPC thread stays responsive and rapid
    // param edits don't queue up synchronously. Listen to the
    // `analysis:started` / `analysis:complete` / `analysis:failed` /
    // `analysis:cancelled` Tauri events for status.
    let params_tree = snapshot.to_json_for_target(openisi_params::PersistTarget::Analysis);
    analysis_tx
        .send(crate::messages::AnalysisCmd::Run(Box::new(
            crate::messages::AnalysisRequest {
                path: path_buf,
                snapshot,
                params_tree,
            },
        )))
        .map_err(|e| AppError::Validation(format!("send to analysis worker: {e}")))?;
    Ok("queued".into())
}

/// Set the active `.oisi` file — the one the UI currently has open.
/// `get_analysis_params` / `set_analysis_params` target this path.
/// Pass an empty string to clear the active file.
#[tauri::command]
pub fn set_active_oisi(state: State<'_, SharedState>, path: String) -> AppResult<()> {
    set_active_oisi_impl(&state, path)
}

/// Migrate a pre-2026 `.oisi`'s `/analysis_params` to the current
/// registry-tree schema, in place. Safe to call on any file: a no-op
/// message is returned when there's nothing to migrate. This is the
/// in-app counterpart to the `oisi migrate` CLI, so a GUI-only user can
/// bring an old file forward (e.g. when `run_analysis` refuses it).
#[tauri::command]
pub fn migrate_oisi(path: String) -> AppResult<String> {
    let p = std::path::PathBuf::from(&path);
    if !p.exists() {
        return Err(AppError::NotAvailable(format!(
            "migrate_oisi: file does not exist: {}",
            p.display()
        )));
    }

    let Some(old_tree) = isi_analysis::io::read_analysis_params_attr(&p)? else {
        return Ok(format!(
            "{}: no /analysis_params attribute — nothing to migrate.",
            p.display()
        ));
    };
    if !isi_analysis::io::is_pre_2026_analysis_params(&p)? {
        return Ok(format!(
            "{}: /analysis_params already in the current schema — no migration needed.",
            p.display()
        ));
    }

    let new_tree = isi_analysis::migrate::translate_pre_2026_analysis_params(&old_tree)?;
    isi_analysis::io::write_analysis_params_attr(&p, &new_tree)?;
    Ok(format!("Migrated /analysis_params on {}", p.display()))
}

/// Inner implementation of `set_active_oisi` that takes `&SharedState`
/// directly. Public for integration testing — callers can construct
/// an `Arc<AppState>` and invoke without a Tauri runtime.
pub fn set_active_oisi_impl(state: &SharedState, path: String) -> AppResult<()> {
    if path.is_empty() {
        *state.active_oisi.lock() = None;
    } else {
        let p = std::path::PathBuf::from(&path);
        if !p.exists() {
            return Err(AppError::NotAvailable(format!(
                "set_active_oisi: file does not exist: {}",
                p.display()
            )));
        }
        *state.active_oisi.lock() = Some(p);
    }
    Ok(())
}

/// Read the active `.oisi`'s `/analysis_params` registry tree, OR the
/// current Registry tree if no file is active. Display-only.
///
/// Users edit analysis parameters via the standard `set_params` /
/// `set_active_params` calls on the Registry — there is no special
/// per-`.oisi` editing path. Re-running analysis on the current
/// Registry produces a new `/analysis_params` tree stamped into the
/// `.oisi`.
#[tauri::command]
pub fn get_analysis_params(state: State<'_, SharedState>) -> AppResult<serde_json::Value> {
    get_analysis_params_impl(&state)
}

pub fn get_analysis_params_impl(state: &SharedState) -> AppResult<serde_json::Value> {
    // Capture the active path and a registry snapshot, dropping each guard
    // before any I/O.
    let path = state.active_oisi.lock().clone();
    let snapshot = state.registry.lock().snapshot();

    if let Some(p) = path
        && let Some(tree) = isi_analysis::io::read_analysis_params_attr(&p)?
    {
        return Ok(tree);
    }
    // No file or no stored tree → return the current Registry tree.
    Ok(snapshot.to_json_for_target(openisi_params::PersistTarget::Analysis))
}

/// Read any result dataset from a .oisi file. Returns typed data.
/// Dispatches on `isi_analysis::io::classify_result_type` so the
/// type-tag rules live in one place; UI and reader stay in sync.
#[tauri::command]
pub fn read_result(path: String, name: String) -> AppResult<serde_json::Value> {
    let file = hdf5::File::open(&path).map_err(|e| {
        AppError::Analysis(isi_analysis::AnalysisError::Hdf5(format!(
            "Failed to open {path}: {e}"
        )))
    })?;
    let ds = file.dataset(&format!("results/{name}")).map_err(|e| {
        AppError::Analysis(isi_analysis::AnalysisError::Hdf5(format!(
            "Failed to open results/{name}: {e}"
        )))
    })?;
    let shape = ds.shape();
    let result_type = isi_analysis::io::classify_result_type(&name, &shape, None);
    let hdf5_err = |e: hdf5::Error, ctx: &str| {
        AppError::Analysis(isi_analysis::AnalysisError::Hdf5(format!(
            "reading {ctx} {name}: {e}"
        )))
    };

    match result_type.as_str() {
        "sign_array" => {
            let data: Vec<i32> = ds.read_1d().map_err(|e| hdf5_err(e, "1D"))?.to_vec();
            Ok(serde_json::json!({ "type": "sign_array", "data": data }))
        }
        "label_map" => {
            let (h, w) = (shape[0], shape[1]);
            let data: ndarray::Array2<i32> = ds.read().map_err(|e| hdf5_err(e, "label_map"))?;
            let flat: Vec<i32> = data.into_raw_vec_and_offset().0;
            Ok(serde_json::json!({
                "type": "label_map", "width": w, "height": h, "data": flat,
            }))
        }
        "bool_mask" => {
            let (h, w) = (shape[0], shape[1]);
            let data: ndarray::Array2<u8> = ds.read().map_err(|e| hdf5_err(e, "bool_mask"))?;
            let flat: Vec<u8> = data.into_raw_vec_and_offset().0;
            Ok(serde_json::json!({
                "type": "bool_mask", "width": w, "height": h, "data": flat,
            }))
        }
        "scalar_map" => {
            let (h, w) = (shape[0], shape[1]);
            let data: ndarray::Array2<f64> = ds.read().map_err(|e| hdf5_err(e, "scalar_map"))?;
            let flat: Vec<f64> = data.into_raw_vec_and_offset().0;
            Ok(serde_json::json!({
                "type": "scalar_map", "width": w, "height": h, "data": flat,
            }))
        }
        other => Err(AppError::Validation(format!(
            "classify_result_type returned unknown tag {other:?} for results/{name}"
        ))),
    }
}

/// Read the anatomical image from a .oisi file.
#[tauri::command]
pub fn read_anatomical(path: String) -> AppResult<serde_json::Value> {
    let data = isi_analysis::io::read_anatomical(std::path::Path::new(&path))?;
    let (h, w) = data.dim();
    let flat: Vec<u8> = data.into_raw_vec_and_offset().0;
    Ok(serde_json::json!({
        "width": w,
        "height": h,
        "data": flat,
    }))
}

/// Export a result map as a PNG file. Uses the shared [`crate::render`]
/// renderer driven by the dataset's `MapMeta` (palette, display range, wrap
/// period, sentinel semantics) — the SAME logic the headless figure exporter
/// and the interactive GUI use, so the exported PNG matches what's on screen.
/// Legacy files lacking `MapMeta` attrs fall back to an auto-fit jet map.
#[tauri::command]
pub fn export_map_png(path: String, map_name: String, output_path: String) -> AppResult<()> {
    let src = std::path::Path::new(&path);
    let data = isi_analysis::io::read_result_map(src, &map_name)?;
    let (h, w) = data.dim();

    let meta = isi_analysis::io::read_result_meta(src, &map_name).unwrap_or_else(|| {
        // Pre-2026 files written before the self-describing MapMeta attrs:
        // auto-fit a jet map over the finite range (the old behavior).
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for &v in data.iter() {
            if v.is_finite() {
                lo = lo.min(v);
                hi = hi.max(v);
            }
        }
        if !lo.is_finite() {
            (lo, hi) = (0.0, 1.0);
        }
        isi_analysis::MapMeta {
            palette: "jet".into(),
            units: "".into(),
            display_min: lo,
            display_max: hi,
            wrap_period: 0.0,
            nan_means: "".into(),
            zero_means: "".into(),
        }
    });

    let (rgba, _label) = crate::render::render_map(&data, &meta, None);

    let mut png_data = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png_data, w as u32, h as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| AppError::Io(std::io::Error::other(format!("PNG header: {e}"))))?;
        writer
            .write_image_data(&rgba)
            .map_err(|e| AppError::Io(std::io::Error::other(format!("PNG write: {e}"))))?;
    }

    std::fs::write(&output_path, &png_data)?;
    Ok(())
}
