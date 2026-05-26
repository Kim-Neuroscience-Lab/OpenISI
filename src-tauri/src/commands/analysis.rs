//! Analysis pipeline commands: run analysis, read results, export.

use tauri::State;

use crate::error::{lock_state, AppError, AppResult};

use super::SharedState;

/// Return a human-readable label for the device the analysis pipeline
/// is using — `"CUDA device 0"`, `"Apple Metal (MPS)"`, or
/// `"CPU (libtorch)"`. Read-only; the device is auto-detected at
/// startup and there's no override toggle. Used by the analysis
/// sidebar to surface compute backend without the user having to read
/// stderr.
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
        "acquisition_cycles": caps.acquisition_cycles,
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
/// (not read from `AppState.active_oisi_path` mid-run).
#[tauri::command]
pub fn run_analysis(state: State<'_, SharedState>, path: String) -> AppResult<String> {
    let app = lock_state(&state, "run_analysis")?;
    let snapshot = {
        let reg = app.registry.lock().map_err(|_| AppError::LockPoisoned {
            context: "registry".into(),
        })?;
        reg.snapshot()
    };
    drop(app); // Release AppState lock during analysis.

    let path_buf = std::path::PathBuf::from(&path);

    // Refuse pre-2026 schema files — the user must migrate explicitly.
    if isi_analysis::io::is_pre_2026_analysis_params(&path_buf)? {
        return Err(AppError::Validation(format!(
            "{} has pre-2026 /analysis_params schema. Run `oisi migrate {}` first.",
            path_buf.display(),
            path_buf.display(),
        )));
    }

    let params = isi_analysis::bridge::analysis_params_from_snapshot(&snapshot);
    let params_tree = snapshot.to_json_for_target(openisi_params::PersistTarget::Analysis);

    let progress = isi_analysis::SilentProgress;
    let cancel = std::sync::atomic::AtomicBool::new(false);

    isi_analysis::analyze(&path_buf, &params, &progress, &cancel)?;

    // Stamp the registry tree into /analysis_params for provenance.
    isi_analysis::io::write_analysis_params_attr(&path_buf, &params_tree)?;

    Ok("Analysis complete".into())
}

/// Set the active `.oisi` file — the one the UI currently has open.
/// `get_analysis_params` / `set_analysis_params` target this path.
/// Pass an empty string to clear the active file.
#[tauri::command]
pub fn set_active_oisi(state: State<'_, SharedState>, path: String) -> AppResult<()> {
    set_active_oisi_impl(&state, path)
}

/// Inner implementation of `set_active_oisi` that takes `&SharedState`
/// directly. Public for integration testing — callers can construct
/// an `Arc<Mutex<AppState>>` and invoke without a Tauri runtime.
pub fn set_active_oisi_impl(state: &SharedState, path: String) -> AppResult<()> {
    let mut app = lock_state(state, "set_active_oisi")?;
    if path.is_empty() {
        app.active_oisi_path = None;
    } else {
        let p = std::path::PathBuf::from(&path);
        if !p.exists() {
            return Err(AppError::NotAvailable(format!(
                "set_active_oisi: file does not exist: {}", p.display()
            )));
        }
        app.active_oisi_path = Some(p);
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
    let app = lock_state(state, "get_analysis_params")?;
    let path = app.active_oisi_path.clone();
    // Take a registry snapshot while we hold the AppState lock; release
    // before any I/O.
    let snapshot = {
        let reg = app.registry.lock().map_err(|_| AppError::LockPoisoned {
            context: "registry".into(),
        })?;
        reg.snapshot()
    };
    drop(app);

    if let Some(p) = path {
        if let Some(tree) = isi_analysis::io::read_analysis_params_attr(&p)? {
            return Ok(tree);
        }
    }
    // No file or no stored tree → return the current Registry tree.
    Ok(snapshot.to_json_for_target(openisi_params::PersistTarget::Analysis))
}

/// Read any result dataset from a .oisi file. Returns typed data.
/// Dispatches on `isi_analysis::io::classify_result_type` so the
/// type-tag rules live in one place; UI and reader stay in sync.
#[tauri::command]
pub fn read_result(path: String, name: String) -> AppResult<serde_json::Value> {
    let file = hdf5::File::open(&path)
        .map_err(|e| AppError::Analysis(isi_analysis::AnalysisError::Hdf5(
            format!("Failed to open {path}: {e}"),
        )))?;
    let ds = file.dataset(&format!("results/{name}"))
        .map_err(|e| AppError::Analysis(isi_analysis::AnalysisError::Hdf5(
            format!("Failed to open results/{name}: {e}"),
        )))?;
    let shape = ds.shape();
    let result_type = isi_analysis::io::classify_result_type(&name, &shape, None);
    let hdf5_err = |e: hdf5::Error, ctx: &str| AppError::Analysis(
        isi_analysis::AnalysisError::Hdf5(format!("reading {ctx} {name}: {e}")),
    );

    match result_type.as_str() {
        "sign_array" => {
            let data: Vec<i32> = ds.read_1d()
                .map_err(|e| hdf5_err(e, "1D"))?
                .to_vec();
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

/// Export a result map as a PNG file.
#[tauri::command]
pub fn export_map_png(path: String, map_name: String, output_path: String) -> AppResult<()> {
    let data = isi_analysis::io::read_result_map(std::path::Path::new(&path), &map_name)?;
    let (h, w) = data.dim();

    // Normalize to 0-255 for PNG.
    let mut min_val = f64::INFINITY;
    let mut max_val = f64::NEG_INFINITY;
    for &v in data.iter() {
        if v.is_finite() {
            if v < min_val { min_val = v; }
            if v > max_val { max_val = v; }
        }
    }
    let range = (max_val - min_val).max(1e-10);

    // Encode as RGB PNG with jet colormap.
    let mut rgb = Vec::with_capacity(h * w * 3);
    for &v in data.iter() {
        let t = ((v - min_val) / range).clamp(0.0, 1.0);
        let (r, g, b) = jet_colormap(t);
        rgb.push(r);
        rgb.push(g);
        rgb.push(b);
    }

    let mut png_data = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png_data, w as u32, h as u32);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()
            .map_err(|e| AppError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("PNG header: {e}"),
            )))?;
        writer.write_image_data(&rgb)
            .map_err(|e| AppError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("PNG write: {e}"),
            )))?;
    }

    std::fs::write(&output_path, &png_data)?;
    Ok(())
}

fn jet_colormap(t: f64) -> (u8, u8, u8) {
    let t = t.clamp(0.0, 1.0);
    let (r, g, b) = if t < 0.125 {
        (0.0, 0.0, 0.5 + t / 0.125 * 0.5)
    } else if t < 0.375 {
        (0.0, (t - 0.125) / 0.25, 1.0)
    } else if t < 0.625 {
        ((t - 0.375) / 0.25, 1.0, 1.0 - (t - 0.375) / 0.25)
    } else if t < 0.875 {
        (1.0, 1.0 - (t - 0.625) / 0.25, 0.0)
    } else {
        (1.0 - (t - 0.875) / 0.125 * 0.5, 0.0, 0.0)
    };
    ((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}
