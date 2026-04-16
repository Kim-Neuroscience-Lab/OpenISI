//! Analysis pipeline commands: run analysis, read results, export.

use tauri::State;

use crate::error::{lock_state, AppError, AppResult};

use super::SharedState;

/// Inspect a .oisi file — what data is present.
#[tauri::command]
pub fn inspect_oisi(path: String) -> AppResult<serde_json::Value> {
    let caps = isi_analysis::io::inspect(std::path::Path::new(&path))?;
    Ok(serde_json::json!({
        "has_anatomical": caps.has_anatomical,
        "has_acquisition": caps.has_acquisition,
        "has_complex_maps": caps.has_complex_maps,
        "has_results": caps.has_results,
        "dimensions": caps.dimensions,
        "acquisition_cycles": caps.acquisition_cycles,
        "results": caps.results,
    }))
}

/// Run analysis on a .oisi file.
#[tauri::command]
pub fn run_analysis(state: State<'_, SharedState>, path: String) -> AppResult<String> {
    let app = lock_state(&state, "run_analysis")?;
    let reg = lock_state(&app.registry, "run_analysis registry")?;
    let snap = reg.snapshot();
    drop(reg);
    drop(app); // Release lock during analysis

    let seg_params = Some(isi_analysis::params::SegmentationParams {
        sign_map_filter_sigma: snap.sign_map_filter_sigma(),
        sign_map_threshold: snap.sign_map_threshold(),
        open_radius: snap.open_radius(),
        close_radius: snap.close_radius(),
        dilate_radius: snap.dilate_radius(),
        pad_border: snap.pad_border(),
        spur_iterations: snap.spur_iterations(),
        split_overlap_threshold: snap.split_overlap_threshold(),
        merge_overlap_threshold: snap.merge_overlap_threshold(),
        merge_dilate_radius: snap.merge_dilate_radius(),
        merge_close_radius: snap.merge_close_radius(),
        eccentricity_radius: snap.eccentricity_radius(),
    });

    let params = isi_analysis::AnalysisParams {
        smoothing_sigma: snap.smoothing_sigma(),
        rotation_k: snap.rotation_k(),
        azi_angular_range: snap.azi_angular_range(),
        alt_angular_range: snap.alt_angular_range(),
        offset_azi: snap.offset_azi(),
        offset_alt: snap.offset_alt(),
        epsilon: snap.epsilon(),
        segmentation: seg_params,
    };

    let progress = isi_analysis::SilentProgress;
    let cancel = std::sync::atomic::AtomicBool::new(false);

    isi_analysis::analyze(
        std::path::Path::new(&path),
        &params,
        &progress,
        &cancel,
    )?;

    Ok("Analysis complete".into())
}

/// Get analysis parameters (from rig.toml [analysis]).
#[tauri::command]
pub fn get_analysis_params(state: State<'_, SharedState>) -> AppResult<serde_json::Value> {
    let app = lock_state(&state, "get_analysis_params")?;
    let reg = lock_state(&app.registry, "get_analysis_params registry")?;
    Ok(serde_json::json!({
        "smoothing_sigma": reg.smoothing_sigma(),
        "rotation_k": reg.rotation_k(),
        "azi_angular_range": reg.azi_angular_range(),
        "alt_angular_range": reg.alt_angular_range(),
        "offset_azi": reg.offset_azi(),
        "offset_alt": reg.offset_alt(),
        "epsilon": reg.epsilon(),
        "segmentation": {
            "sign_map_filter_sigma": reg.sign_map_filter_sigma(),
            "sign_map_threshold": reg.sign_map_threshold(),
            "open_radius": reg.open_radius(),
            "close_radius": reg.close_radius(),
            "dilate_radius": reg.dilate_radius(),
            "pad_border": reg.pad_border(),
            "spur_iterations": reg.spur_iterations(),
            "split_overlap_threshold": reg.split_overlap_threshold(),
            "merge_overlap_threshold": reg.merge_overlap_threshold(),
            "merge_dilate_radius": reg.merge_dilate_radius(),
            "merge_close_radius": reg.merge_close_radius(),
            "eccentricity_radius": reg.eccentricity_radius(),
        },
    }))
}

/// Update analysis parameters. Persists to rig.toml.
#[tauri::command]
pub fn set_analysis_params(
    state: State<'_, SharedState>,
    smoothing_sigma: f64,
    rotation_k: i32,
    azi_angular_range: f64,
    alt_angular_range: f64,
    offset_azi: f64,
    offset_alt: f64,
    sign_map_filter_sigma: Option<f64>,
    sign_map_threshold: Option<f64>,
    eccentricity_radius: Option<f64>,
) -> AppResult<()> {
    use crate::params::{ParamId, ParamValue};

    let app = lock_state(&state, "set_analysis_params")?;
    let mut reg = lock_state(&app.registry, "set_analysis_params registry")?;

    reg.batch(|r| -> Result<(), String> {
        r.set(ParamId::SmoothingSigma, ParamValue::F64(smoothing_sigma))?;
        r.set(ParamId::RotationK, ParamValue::I32(rotation_k))?;
        r.set(ParamId::AziAngularRange, ParamValue::F64(azi_angular_range))?;
        r.set(ParamId::AltAngularRange, ParamValue::F64(alt_angular_range))?;
        r.set(ParamId::OffsetAzi, ParamValue::F64(offset_azi))?;
        r.set(ParamId::OffsetAlt, ParamValue::F64(offset_alt))?;
        if let Some(v) = sign_map_filter_sigma {
            r.set(ParamId::SignMapFilterSigma, ParamValue::F64(v))?;
        }
        if let Some(v) = sign_map_threshold {
            r.set(ParamId::SignMapThreshold, ParamValue::F64(v))?;
        }
        if let Some(v) = eccentricity_radius {
            r.set(ParamId::EccentricityRadius, ParamValue::F64(v))?;
        }
        Ok(())
    }).map_err(|e| AppError::Validation(e))?;

    if let Err(e) = reg.save_rig() {
        eprintln!("[params] Failed to save analysis params: {e}");
    }
    Ok(())
}

/// Read any result dataset from a .oisi file. Returns typed data.
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

    // 1D array (e.g., area_signs).
    if shape.len() == 1 {
        let data: Vec<i32> = ds.read_1d()
            .map_err(|e| AppError::Analysis(isi_analysis::AnalysisError::Hdf5(
                format!("reading 1D {name}: {e}"),
            )))?
            .to_vec();
        return Ok(serde_json::json!({
            "type": "sign_array",
            "data": data,
        }));
    }

    // 2D dataset — determine type from name.
    let (h, w) = (shape[0], shape[1]);

    if name == "area_labels" {
        let data: ndarray::Array2<i32> = ds.read()
            .map_err(|e| AppError::Analysis(isi_analysis::AnalysisError::Hdf5(
                format!("reading {name}: {e}"),
            )))?;
        let flat: Vec<i32> = data.into_raw_vec_and_offset().0;
        return Ok(serde_json::json!({
            "type": "label_map",
            "width": w, "height": h,
            "data": flat,
        }));
    }

    if name == "area_borders" || name == "contours_azi" || name == "contours_alt" {
        let data: ndarray::Array2<u8> = ds.read()
            .map_err(|e| AppError::Analysis(isi_analysis::AnalysisError::Hdf5(
                format!("reading {name}: {e}"),
            )))?;
        let flat: Vec<u8> = data.into_raw_vec_and_offset().0;
        return Ok(serde_json::json!({
            "type": "bool_mask",
            "width": w, "height": h,
            "data": flat,
        }));
    }

    // Default: f64 scalar map.
    let data: ndarray::Array2<f64> = ds.read()
        .map_err(|e| AppError::Analysis(isi_analysis::AnalysisError::Hdf5(
            format!("reading {name}: {e}"),
        )))?;
    let flat: Vec<f64> = data.into_raw_vec_and_offset().0;
    Ok(serde_json::json!({
        "type": "scalar_map",
        "width": w, "height": h,
        "data": flat,
    }))
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
