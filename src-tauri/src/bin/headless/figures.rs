//! Figure generation for the headless CLI — scalar-map figure export, the
//! method/stage-variant comparison grids, the VFS-threshold sweep grids, the
//! tiny bitmap label font, and the figure-provenance `meta.json`. Split out of
//! the `headless` binary root so that root holds only the CLI command handlers.
//!
//! The rendering primitives (palettes, `render_map`, PNG writing) live in the
//! shared `openisi_lib::render` module; this module orchestrates them into the
//! multi-panel comparison/sweep figures.


use openisi_lib::render::{jet, render_map, write_rgba_png};

// `compare_method_variants` and the figure-provenance fallback re-load the typed
// config to source per-variant tunables / the current analysis tree;
// `load_config_store` lives in the binary root.
use super::load_config_store;

pub(crate) fn compare_method_variants(
    oisi_path: &std::path::Path,
    base_params: &isi_analysis::AnalysisParams,
    figures_dir: &std::path::Path,
) {
    use isi_analysis::methods::{CortexSourceExt, CortexSourceMethod};

    let compare_dir = figures_dir.join("compare");
    if let Err(e) = std::fs::create_dir_all(&compare_dir) {
        tracing::error!(error = %e, "failed to create compare dir");
        return;
    }

    println!("Comparing method variants per stage...");

    // Build all CortexSourceMethod variants with the canonical default tunables.
    // The default-variant tunables (`SnlcGarrett2014ImBound`) come straight from
    // the typed `CortexSource::default()` (the SSoT); the `Reliability` variant's
    // default threshold has no persisted home in the tagged config (only the
    // active variant's tunables are stored), so its canonical default is named
    // explicitly here. The method choice itself is enumerated locally to drive
    // the comparison.
    use openisi_params::config::analysis::CortexSource;
    let CortexSource::SnlcGarrett2014ImBound { k, close, dilate } = CortexSource::default() else {
        unreachable!("CortexSource::default() is SnlcGarrett2014ImBound");
    };
    let cortex_variants = vec![
        CortexSourceMethod::NoRestriction,
        CortexSourceMethod::Reliability { threshold: 0.5 },
        CortexSourceMethod::SnlcGarrett2014ImBound { k, close, dilate },
    ];

    compare_stage_variants(
        "cortex_source",
        oisi_path,
        base_params,
        &compare_dir,
        cortex_variants,
        |params, variant| {
            params.cortex_source = variant;
        },
        |variant| variant.short_label(),
        // Figures affected by cortex_source choice — gates segmentation,
        // so essentially all downstream patch-derived maps shift.
        &[
            "cortex_mask",
            "area_labels",
            "area_borders",
            "eccentricity",
            "magnification",
            "contours_azi",
            "contours_alt",
            "vfs_smoothed_thresholded",
        ],
    );

    // Future: when additional method variants land in patch_threshold,
    // patch_refinement, or sign_map_smoothing, add comparable calls
    // here. Each per-stage `all_variants()` controls what's tried.
}

/// Run the pipeline once per variant of a single stage and produce a
/// grid composite per affected figure.
// genuinely 8 distinct inputs (stage, paths, params, variants, closures);
// bundling would obscure, not clarify
// Justified `#[allow]`: internal dev-figure helper; the inputs are distinct
// types (paths, params, the variant list, two closures, a label slice), so
// positional swaps are compile errors. CLI tooling, not a production API.
#[allow(clippy::too_many_arguments)]
fn compare_stage_variants<V, Apply, Label>(
    stage_name: &str,
    oisi_path: &std::path::Path,
    base_params: &isi_analysis::AnalysisParams,
    compare_dir: &std::path::Path,
    variants: Vec<V>,
    apply: Apply,
    label: Label,
    affected_figures: &[&str],
) where
    V: Clone,
    Apply: Fn(&mut isi_analysis::AnalysisParams, V),
    Label: Fn(&V) -> &'static str,
{
    let stage_dir = compare_dir.join(stage_name);
    if let Err(e) = std::fs::create_dir_all(&stage_dir) {
        tracing::error!(stage = %stage_name, error = %e, "failed to create stage dir");
        return;
    }

    let mut successful: Vec<(String, std::path::PathBuf)> = Vec::new();
    let progress = isi_analysis::SilentProgress;
    let cancel = std::sync::atomic::AtomicBool::new(false);

    for variant in variants {
        let variant_label = label(&variant);
        let variant_dir = stage_dir.join(variant_label);

        // Copy the input to a temp .oisi so we don't trash the primary
        // run's persisted /results. The copy is deleted after rendering.
        let temp_oisi = stage_dir.join(format!(".tmp_{}.oisi", variant_label));
        if let Err(e) = std::fs::copy(oisi_path, &temp_oisi) {
            tracing::error!(stage = %stage_name, variant = %variant_label, error = %e, "copy failed");
            continue;
        }

        // Clear any stored /analysis_params so the temp run uses our params.
        if let Ok(file) = hdf5::File::open_rw(&temp_oisi) {
            let _ = file.delete_attr("analysis_params");
        }

        let mut params = base_params.clone();
        apply(&mut params, variant);

        match isi_analysis::analyze(&temp_oisi, &params, None, &progress, &cancel) {
            Ok(()) => {
                if let Err(e) = std::fs::create_dir_all(&variant_dir) {
                    tracing::error!(stage = %stage_name, variant = %variant_label, error = %e, "mkdir failed");
                } else {
                    export_all_figures(&temp_oisi, &variant_dir.to_string_lossy());
                    successful.push((variant_label.to_string(), variant_dir.clone()));
                    println!("  {stage_name}/{variant_label}: ✓");
                }
            }
            Err(e) => {
                println!("  {stage_name}/{variant_label}: skipped — {e}");
            }
        }

        let _ = std::fs::remove_file(&temp_oisi);
    }

    if successful.len() < 2 {
        println!(
            "[compare/{stage_name}] only {} variant(s) succeeded — no grid produced",
            successful.len()
        );
        return;
    }

    // Composite per-figure grids.
    for fig_name in affected_figures {
        composite_variant_grid(stage_name, fig_name, &successful, &stage_dir);
    }
}

/// Read each variant's `<figure>.png` and stitch into a horizontal grid
/// with a label header per cell. Output: `<stage_dir>/grid_<figure>.png`.
fn composite_variant_grid(
    _stage_name: &str,
    fig_name: &str,
    successful: &[(String, std::path::PathBuf)],
    stage_dir: &std::path::Path,
) {
    let png_name = format!("{fig_name}.png");
    let mut cells: Vec<(String, Vec<u8>, u32, u32)> = Vec::new();

    for (label, dir) in successful {
        let path = dir.join(&png_name);
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let decoder = png::Decoder::new(&bytes[..]);
        let mut reader = match decoder.read_info() {
            Ok(r) => r,
            Err(_) => continue,
        };
        let info = reader.info().clone();
        let mut buf = vec![0u8; reader.output_buffer_size()];
        if reader.next_frame(&mut buf).is_err() {
            continue;
        }
        // Force to RGBA for uniform compositing.
        let rgba = match info.color_type {
            png::ColorType::Rgba => buf,
            png::ColorType::Rgb => {
                let mut out = vec![255u8; (info.width * info.height * 4) as usize];
                for i in 0..(info.width * info.height) as usize {
                    out[i * 4] = buf[i * 3];
                    out[i * 4 + 1] = buf[i * 3 + 1];
                    out[i * 4 + 2] = buf[i * 3 + 2];
                    out[i * 4 + 3] = 255;
                }
                out
            }
            png::ColorType::Grayscale => {
                let mut out = vec![255u8; (info.width * info.height * 4) as usize];
                for i in 0..(info.width * info.height) as usize {
                    out[i * 4] = buf[i];
                    out[i * 4 + 1] = buf[i];
                    out[i * 4 + 2] = buf[i];
                    out[i * 4 + 3] = 255;
                }
                out
            }
            _ => continue,
        };
        cells.push((label.clone(), rgba, info.width, info.height));
    }

    if cells.len() < 2 {
        return;
    }

    let cell_w = cells[0].2;
    let cell_h = cells[0].3;
    let label_h: u32 = 22;
    let pad: u32 = 6;
    let n = cells.len() as u32;
    let total_w = n * cell_w + (n + 1) * pad;
    let total_h = label_h + cell_h + 2 * pad;
    let mut canvas = vec![240u8; (total_w * total_h * 4) as usize];
    // White background
    for px in 0..(total_w * total_h) as usize {
        canvas[px * 4] = 245;
        canvas[px * 4 + 1] = 245;
        canvas[px * 4 + 2] = 245;
        canvas[px * 4 + 3] = 255;
    }

    for (i, (label, rgba, w, h)) in cells.iter().enumerate() {
        if *w != cell_w || *h != cell_h {
            // Skip mismatched sizes — shouldn't happen in practice
            // (same input file, same dimensions), but be defensive.
            continue;
        }
        let x0 = pad + (i as u32) * (cell_w + pad);
        let y0 = pad + label_h;
        // Draw label centered at the top of this cell.
        let text_x = x0 as usize + 4;
        let text_y = (pad + 4) as usize;
        draw_text(
            &mut canvas,
            total_w as usize,
            total_h as usize,
            text_x,
            text_y,
            label,
            (40, 40, 40),
            1,
        );
        // Blit cell pixels.
        for row in 0..(*h as usize) {
            let src_start = row * (*w as usize) * 4;
            let dst_start = ((y0 as usize + row) * total_w as usize + x0 as usize) * 4;
            canvas[dst_start..dst_start + (*w as usize) * 4]
                .copy_from_slice(&rgba[src_start..src_start + (*w as usize) * 4]);
        }
    }

    let out = stage_dir.join(format!("grid_{fig_name}.png"));
    write_rgba_png(&out, total_w, total_h, &canvas);
    println!(
        "  {} ({}x{}, {} variants)",
        out.strip_prefix(stage_dir.parent().unwrap_or(stage_dir))
            .unwrap_or(&out)
            .display(),
        total_w,
        total_h,
        cells.len()
    );
}

pub(crate) fn export_all_figures(oisi_path: &std::path::Path, out_dir: &str) {
    let dir = std::path::Path::new(out_dir);
    if let Err(e) = std::fs::create_dir_all(dir) {
        eprintln!("Failed to create output dir: {e}");
        return;
    }

    let caps = match isi_analysis::io::inspect(oisi_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to inspect: {e}");
            return;
        }
    };

    println!("Exporting figures to {}/", out_dir);

    // Read every scalar_map result from HDF5 exactly once. The unified
    // renderer reads from this cache so the same HDF5 dataset isn't fetched
    // multiple times (e.g., when smoothed VFS reuses raw VFS).
    let mut scalar_maps: std::collections::HashMap<String, ndarray::Array2<f64>> =
        std::collections::HashMap::new();
    for result in &caps.results {
        if result.result_type != "scalar_map" {
            continue;
        }
        match isi_analysis::io::read_result_map(oisi_path, &result.name) {
            Ok(data) => {
                scalar_maps.insert(result.name.clone(), data);
            }
            Err(e) => eprintln!("  {}: read failed: {e}", result.name),
        }
    }

    // Rendering metadata (palette, range, units, NaN/zero semantics)
    // is read per-dataset from HDF5 attrs inside the loop below. The
    // renderer is now pure — it consumes only dataset + `MapMeta` and
    // does no `AnalysisParams` / `AcquisitionProperties` inference.

    // Anatomical grayscale used as the underlay for *masked* figures
    // (Sentinel kind: eccentricity, magnification). Allen and most
    // published mouse-retinotopy figures show vasculature beneath colored
    // patches; this is the same idea.
    let anatomical: Option<Vec<u8>> = isi_analysis::io::read_anatomical(oisi_path)
        .ok()
        .map(|arr| arr.into_iter().collect());

    for result in &caps.results {
        let name = &result.name;
        let rtype = &result.result_type;

        if rtype == "sign_array" {
            continue;
        } // metadata, not a map

        let out_path = dir.join(format!("{name}.png"));

        if rtype == "scalar_map" {
            if let Some(data) = scalar_maps.get(name) {
                let (h, w) = data.dim();
                let Some(meta) = isi_analysis::io::read_result_meta(oisi_path, name) else {
                    eprintln!(
                        "  {name}: skipped — MapMeta attrs absent \
                         (file analyzed before 2026-05-23 OR attr corruption); \
                         re-run `analyze` to attach current rendering metadata"
                    );
                    continue;
                };
                let (rgba, label) = render_map(data, &meta, anatomical.as_deref());
                write_rgba_png(&out_path, w as u32, h as u32, &rgba);
                println!("  {name}.png ({w}x{h}, {label})");
            }
        } else if rtype == "bool_mask" {
            // Two binary conventions, distinguished by name:
            // - *_mask, *_labels: area fills — render TRUE=white (highlighted
            //   region) on BLACK background. Matches fluorescence /
            //   anatomical-imaging convention.
            // - Line drawings (area_borders, contours_*): TRUE=black on
            //   WHITE background. Matches print/figure convention for line art.
            match isi_analysis::io::read_result_map(oisi_path, name) {
                Ok(data) => {
                    let (h, w) = data.dim();
                    let is_area_fill = name == "cortex_mask";
                    let (bg, fg): ([u8; 3], [u8; 3]) = if is_area_fill {
                        ([0, 0, 0], [255, 255, 255])
                    } else {
                        ([255, 255, 255], [0, 0, 0])
                    };
                    let mut rgba = vec![255u8; h * w * 4];
                    for (i, &v) in data.iter().enumerate() {
                        let col = if v > 0.5 { fg } else { bg };
                        rgba[i * 4] = col[0];
                        rgba[i * 4 + 1] = col[1];
                        rgba[i * 4 + 2] = col[2];
                        rgba[i * 4 + 3] = 255;
                    }
                    write_rgba_png(&out_path, w as u32, h as u32, &rgba);
                    println!("  {name}.png ({w}x{h}, {rtype})");
                }
                Err(e) => eprintln!("  {name}: read failed: {e}"),
            }
        } else if rtype == "label_map" {
            // Read as f64, color by label.
            match isi_analysis::io::read_result_map(oisi_path, name) {
                Ok(data) => {
                    let (h, w) = data.dim();
                    // Read area_signs for coloring.
                    let signs: Vec<i32> = match hdf5::File::open(oisi_path) {
                        Ok(f) => f
                            .dataset("results/area_signs")
                            .and_then(|ds| ds.read_1d::<i32>())
                            .map(|a| a.to_vec())
                            .unwrap_or_default(),
                        Err(_) => Vec::new(),
                    };

                    let mut rgba = vec![255u8; h * w * 4]; // white background
                    for (i, &v) in data.iter().enumerate() {
                        let label = v as i32;
                        if label > 0 && label <= signs.len() as i32 {
                            let sign = signs[(label - 1) as usize];
                            if sign > 0 {
                                rgba[i * 4] = 220;
                                rgba[i * 4 + 1] = 50;
                                rgba[i * 4 + 2] = 50;
                            } else {
                                rgba[i * 4] = 50;
                                rgba[i * 4 + 1] = 50;
                                rgba[i * 4 + 2] = 220;
                            }
                        }
                    }
                    write_rgba_png(&out_path, w as u32, h as u32, &rgba);
                    println!("  {name}.png ({w}x{h}, {rtype})");
                }
                Err(e) => eprintln!("  {name}: read failed: {e}"),
            }
        }
    }

    // Also export anatomical if present.
    if caps.has_anatomical {
        match isi_analysis::io::read_anatomical(oisi_path) {
            Ok(anat) => {
                let (h, w) = anat.dim();
                let mut rgba = vec![255u8; h * w * 4];
                for (i, &v) in anat.iter().enumerate() {
                    rgba[i * 4] = v;
                    rgba[i * 4 + 1] = v;
                    rgba[i * 4 + 2] = v;
                }
                let out_path = dir.join("anatomical.png");
                write_rgba_png(&out_path, w as u32, h as u32, &rgba);
                println!("  anatomical.png ({w}x{h})");
            }
            Err(e) => eprintln!("  anatomical: {e}"),
        }
    }

    // Per-direction phase diagnostic figures.
    //
    // The Kalatsky-Stryker / Garrett / Juavinett canonical methods specify
    // that each individual-direction phase map should already show a smooth
    // gradient across the cortex *before* any forward/reverse combination —
    // if each direction's phase is flat across cortex, the data lacks
    // position-tuned response (typically over-anesthesia / poor neurovascular
    // coupling) and no analysis can recover retinotopy.
    //
    // Per Juavinett 2017 Nat Protocols Troubleshooting Step 51 + West 2022:
    // "Reduce isoflurane flow, and wait for the mouse to wake up slightly
    //  such that breathing is >1 breath per s."
    // Per-direction phase diagnostic figures + circular phase statistics.
    // Each direction's complex map → real-valued phase array → unified
    // renderer with RenderKind::Wrapped (HSV, full ±π).
    if let Ok(maps) = isi_analysis::io::read_complex_maps(oisi_path) {
        let amp_azi: Vec<f64> = maps
            .azi_fwd
            .iter()
            .zip(maps.azi_rev.iter())
            .map(|(a, b)| 0.5 * (a.norm() + b.norm()))
            .collect();
        let amp_alt: Vec<f64> = maps
            .alt_fwd
            .iter()
            .zip(maps.alt_rev.iter())
            .map(|(a, b)| 0.5 * (a.norm() + b.norm()))
            .collect();

        println!("Per-direction phase variation across cortex (amp-weighted):");
        for (name, cm, amp) in [
            ("azi_fwd", &maps.azi_fwd, &amp_azi),
            ("azi_rev", &maps.azi_rev, &amp_azi),
            ("alt_fwd", &maps.alt_fwd, &amp_alt),
            ("alt_rev", &maps.alt_rev, &amp_alt),
        ] {
            let (mean_deg, std_deg) = circular_phase_stats(cm, amp);
            println!("  {name}: mean={mean_deg:>7.2}°  circular_std={std_deg:>6.2}°");
        }

        for (name, cm) in [
            ("azi_fwd_phase", &maps.azi_fwd),
            ("azi_rev_phase", &maps.azi_rev),
            ("alt_fwd_phase", &maps.alt_fwd),
            ("alt_rev_phase", &maps.alt_rev),
        ] {
            let phase = cm.mapv(|z| z.arg());
            let (h, w) = phase.dim();
            // Per-direction phase figures are not stored in `/results`,
            // so they have no `MapMeta` attached. Synthesize one matching
            // the radian-phase convention (HSV over [-π, π], wrap 2π).
            let meta = isi_analysis::MapMeta {
                palette: std::borrow::Cow::Borrowed("hsv_circular"),
                units: std::borrow::Cow::Borrowed("rad"),
                display_min: -std::f64::consts::PI,
                display_max: std::f64::consts::PI,
                wrap_period: std::f64::consts::TAU,
                nan_means: std::borrow::Cow::Borrowed(""),
                zero_means: std::borrow::Cow::Borrowed(""),
            };
            let (rgba, label) = render_map(&phase, &meta, None);
            let out_path = dir.join(format!("{name}.png"));
            write_rgba_png(&out_path, w as u32, h as u32, &rgba);
            println!("  {name}.png ({w}x{h}, {label})");
        }
    }

    println!("Done — {} figures exported", caps.results.len());
}


/// Amplitude-weighted circular mean and circular std of a phase map, in
/// degrees. Standard definitions (Mardia 1972):
///   mean φ̄ = arg( Σ w·exp(iφ) / Σ w )
///   R = | Σ w·exp(iφ) / Σ w |     (resultant length, ∈ [0, 1])
///   circular std = sqrt(-2·ln R)   (radians; equals σ for small-spread limit)
fn circular_phase_stats(
    cm: &ndarray::Array2<isi_analysis::Complex64>,
    weights: &[f64],
) -> (f64, f64) {
    let mut sum_re = 0.0_f64;
    let mut sum_im = 0.0_f64;
    let mut sum_w = 0.0_f64;
    for (z, &w) in cm.iter().zip(weights.iter()) {
        if !w.is_finite() || w <= 0.0 {
            continue;
        }
        let phi = z.arg();
        sum_re += w * phi.cos();
        sum_im += w * phi.sin();
        sum_w += w;
    }
    if sum_w <= 0.0 {
        return (0.0, 0.0);
    }
    let mean_phi = sum_im.atan2(sum_re);
    let r = (sum_re * sum_re + sum_im * sum_im).sqrt() / sum_w;
    let r_clamped = r.clamp(1e-12, 1.0);
    let std_rad = (-2.0 * r_clamped.ln()).sqrt();
    (mean_phi.to_degrees(), std_rad.to_degrees())
}


// ═══════════════════════════════════════════════════════════════════════
// Tiny 5x7 bitmap font for grid cell labels
//
// Hand-coded for the subset of ASCII chars the threshold-sweep grids need:
//   0-9 . = | > x s g c K A l e n V F S k h r f i o t a m d  space
// Each glyph is 7 rows of 5 bits (LSB is rightmost column).
// ═══════════════════════════════════════════════════════════════════════

const FONT_CHARS: &[(char, [u8; 7])] = &[
    (
        '0',
        [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
    ),
    (
        '1',
        [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
    ),
    (
        '2',
        [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
    ),
    (
        '3',
        [
            0b01110, 0b10001, 0b00001, 0b00110, 0b00001, 0b10001, 0b01110,
        ],
    ),
    (
        '4',
        [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
    ),
    (
        '5',
        [
            0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110,
        ],
    ),
    (
        '6',
        [
            0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
    ),
    (
        '7',
        [
            0b11111, 0b00001, 0b00010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
    ),
    (
        '8',
        [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
    ),
    (
        '9',
        [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100,
        ],
    ),
    (
        '.',
        [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00110, 0b00110,
        ],
    ),
    (
        '=',
        [
            0b00000, 0b00000, 0b11111, 0b00000, 0b11111, 0b00000, 0b00000,
        ],
    ),
    (
        '|',
        [
            0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
    ),
    (
        '>',
        [
            0b10000, 0b01000, 0b00100, 0b00010, 0b00100, 0b01000, 0b10000,
        ],
    ),
    (
        '<',
        [
            0b00001, 0b00010, 0b00100, 0b01000, 0b00100, 0b00010, 0b00001,
        ],
    ),
    (
        'x',
        [
            0b00000, 0b00000, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001,
        ],
    ),
    (
        's',
        [
            0b00000, 0b00000, 0b01111, 0b10000, 0b01110, 0b00001, 0b11110,
        ],
    ),
    (
        'g',
        [
            0b00000, 0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b01110,
        ],
    ),
    (
        'c',
        [
            0b00000, 0b00000, 0b01110, 0b10000, 0b10000, 0b10001, 0b01110,
        ],
    ),
    (
        'K',
        [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
    ),
    (
        'A',
        [
            0b00100, 0b01010, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001,
        ],
    ),
    (
        'l',
        [
            0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
    ),
    (
        'e',
        [
            0b00000, 0b00000, 0b01110, 0b10001, 0b11111, 0b10000, 0b01110,
        ],
    ),
    (
        'n',
        [
            0b00000, 0b00000, 0b10110, 0b11001, 0b10001, 0b10001, 0b10001,
        ],
    ),
    (
        'V',
        [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
    ),
    (
        'F',
        [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
    ),
    (
        'S',
        [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
    ),
    (
        'k',
        [
            0b10000, 0b10000, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010,
        ],
    ),
    (
        'h',
        [
            0b10000, 0b10000, 0b10110, 0b11001, 0b10001, 0b10001, 0b10001,
        ],
    ),
    (
        'r',
        [
            0b00000, 0b00000, 0b10110, 0b11001, 0b10000, 0b10000, 0b10000,
        ],
    ),
    (
        'f',
        [
            0b00110, 0b01001, 0b01000, 0b11110, 0b01000, 0b01000, 0b01000,
        ],
    ),
    (
        'i',
        [
            0b00100, 0b00000, 0b01100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
    ),
    (
        'o',
        [
            0b00000, 0b00000, 0b01110, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
    ),
    (
        't',
        [
            0b01000, 0b01000, 0b11110, 0b01000, 0b01000, 0b01001, 0b00110,
        ],
    ),
    (
        'a',
        [
            0b00000, 0b00000, 0b01110, 0b00001, 0b01111, 0b10001, 0b01111,
        ],
    ),
    (
        'm',
        [
            0b00000, 0b00000, 0b11010, 0b10101, 0b10101, 0b10101, 0b10001,
        ],
    ),
    (
        'd',
        [
            0b00001, 0b00001, 0b01111, 0b10001, 0b10001, 0b10001, 0b01111,
        ],
    ),
    (
        'p',
        [
            0b00000, 0b00000, 0b11110, 0b10001, 0b11110, 0b10000, 0b10000,
        ],
    ),
    (
        ' ',
        [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000,
        ],
    ),
    (
        '-',
        [
            0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
        ],
    ),
    (
        ':',
        [
            0b00000, 0b00100, 0b00100, 0b00000, 0b00100, 0b00100, 0b00000,
        ],
    ),
    (
        ',',
        [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00110, 0b00110, 0b00100,
        ],
    ),
];

fn glyph_for(ch: char) -> [u8; 7] {
    for &(c, g) in FONT_CHARS {
        if c == ch {
            return g;
        }
    }
    // Fallback: solid box so missing glyphs are visible.
    [0b11111; 7]
}

/// Draw `text` into the RGBA `buf` at pixel `(x, y)` using the 5×7 bitmap
/// font scaled by `scale`. Pixels off-canvas are silently clipped. The glyph
/// pitch is 6 columns at scale 1 (5px glyph + 1px spacing).
// genuinely 8 distinct inputs (buffer, dims, position, text, color, scale)
// Justified `#[allow]`: internal bitmap-font blitter for the figure labels.
// Several `usize` args (canvas dims, x/y, scale) ARE swap-able in principle, but
// it has a handful of co-located call sites in this dev-only file and is not a
// production API; a `Canvas` param object would be disproportionate here.
#[allow(clippy::too_many_arguments)]
fn draw_text(
    buf: &mut [u8],
    total_w: usize,
    total_h: usize,
    x: usize,
    y: usize,
    text: &str,
    color: (u8, u8, u8),
    scale: usize,
) {
    let mut cursor_x = x;
    for ch in text.chars() {
        let g = glyph_for(ch);
        for (row, &bits) in g.iter().enumerate() {
            for col in 0..5 {
                if bits & (1 << (4 - col)) == 0 {
                    continue;
                }
                for dy in 0..scale {
                    for dx in 0..scale {
                        let px = cursor_x + col * scale + dx;
                        let py = y + row * scale + dy;
                        if px >= total_w || py >= total_h {
                            continue;
                        }
                        let i = (py * total_w + px) * 4;
                        buf[i] = color.0;
                        buf[i + 1] = color.1;
                        buf[i + 2] = color.2;
                        buf[i + 3] = 255;
                    }
                }
            }
        }
        cursor_x += 6 * scale;
    }
}

#[derive(Copy, Clone)]
enum ThresholdApproach {
    AllenFixed,
    SnlcGlobalStd,
    CortexMaskedStd,
}

pub(crate) fn export_threshold_sweep_grids(
    oisi_path: &std::path::Path,
    out_dir: &std::path::Path,
    params: &isi_analysis::AnalysisParams,
) {
    use isi_analysis::io::read_result_map;

    // Read the smoothed VFS — the array segmentation thresholds.
    // `/results/vfs` is the raw mathematical VFS; `/results/vfs_smoothed`
    // is the Gaussian-smoothed stage this diagnostic operates on.
    let vfs_smooth = match read_result_map(oisi_path, "vfs_smoothed") {
        Ok(a) => a,
        Err(e) => {
            eprintln!("threshold-sweep: read vfs_smoothed: {e}");
            return;
        }
    };
    // Read per-direction reliability and derive cortex via the same
    // formula production uses (cortex_from_reliability with the
    // configured threshold). The diagnostic now operates on the exact
    // same cortex as the production pipeline.
    let rel = match (
        read_result_map(oisi_path, "reliability_azi_fwd"),
        read_result_map(oisi_path, "reliability_azi_rev"),
        read_result_map(oisi_path, "reliability_alt_fwd"),
        read_result_map(oisi_path, "reliability_alt_rev"),
    ) {
        (Ok(a), Ok(b), Ok(c), Ok(d)) => (a, b, c, d),
        _ => {
            eprintln!(
                "threshold-sweep: reliability maps missing — run analyze first \
                       on a file with raw per-cycle data"
            );
            return;
        }
    };
    // Pull the reliability threshold from the configured CortexSourceMethod
    // if it's the Reliability variant; otherwise fall back to the canonical
    // default threshold (the tagged config stores tunables only for the active
    // variant, so a non-Reliability source has no persisted threshold).
    let reliability_threshold = match &params.cortex_source {
        isi_analysis::methods::CortexSourceMethod::Reliability { threshold } => *threshold,
        _ => 0.5,
    };
    let cortex_mask = isi_analysis::segmentation::cortex_from_reliability(
        &rel.0,
        &rel.1,
        &rel.2,
        &rel.3,
        reliability_threshold,
    );
    let anatomical: Option<Vec<u8>> = isi_analysis::io::read_anatomical(oisi_path)
        .ok()
        .map(|arr| arr.into_iter().collect());

    let (h, w) = vfs_smooth.dim();
    if cortex_mask.dim() != (h, w) {
        eprintln!("threshold-sweep: shape mismatch");
        return;
    }

    // Diagnostic-only: Allen `smallPatchThr = 100` default. The actual
    // small-patch threshold lives inside `patch_extraction` method
    // variant params; threshold-sweep uses a fixed value to compare
    // patch counts across threshold values consistently.
    let small_patch_thr: usize = 100;

    let global_std = stddev(vfs_smooth.iter().copied().filter(|v| v.is_finite()));
    let cortex_std = stddev(
        vfs_smooth
            .iter()
            .zip(cortex_mask.iter())
            .filter_map(|(&v, &m)| if m && v.is_finite() { Some(v) } else { None }),
    );

    println!("[threshold-sweep] global σ(vfs_smooth) = {:.4}", global_std);
    println!(
        "[threshold-sweep] cortex σ(vfs_smooth) = {:.4}  ({} pixels)",
        cortex_std,
        cortex_mask.iter().filter(|&&v| v).count()
    );

    let rows: [(ThresholdApproach, &str, [f64; 5]); 3] = [
        (
            ThresholdApproach::AllenFixed,
            "Allen fixed",
            [0.10, 0.15, 0.20, 0.25, 0.35],
        ),
        (
            ThresholdApproach::SnlcGlobalStd,
            "K x global s",
            [1.0, 1.5, 2.0, 2.5, 3.0],
        ),
        (
            ThresholdApproach::CortexMaskedStd,
            "K x cortex s",
            [1.0, 1.5, 2.0, 2.5, 3.0],
        ),
    ];

    render_threshold_grid_patches(
        &vfs_smooth,
        &cortex_mask,
        small_patch_thr,
        global_std,
        cortex_std,
        &rows,
        out_dir,
        anatomical.as_deref(),
    );
    render_threshold_grid_vfs(
        &vfs_smooth,
        global_std,
        cortex_std,
        &rows,
        out_dir,
        anatomical.as_deref(),
    );
}

fn stddev(values: impl Iterator<Item = f64>) -> f64 {
    let (mut n, mut s, mut sq) = (0u64, 0.0_f64, 0.0_f64);
    for v in values {
        n += 1;
        s += v;
        sq += v * v;
    }
    if n < 2 {
        return 0.0;
    }
    let mean = s / n as f64;
    ((sq / n as f64) - mean * mean).max(0.0).sqrt()
}

fn threshold_for_cell(
    approach: ThresholdApproach,
    p: f64,
    global_std: f64,
    cortex_std: f64,
) -> f64 {
    match approach {
        ThresholdApproach::AllenFixed => p,
        ThresholdApproach::SnlcGlobalStd => p * global_std,
        ThresholdApproach::CortexMaskedStd => p * cortex_std,
    }
}

/// Grid layout: left margin holds row headers, top of each cell holds its
/// per-cell label. Returns `(cell_w, cell_h, label_h, pad, header_w,
/// total_w, total_h, rgba_buf)`.
fn build_grid_canvas(
    h: usize,
    w: usize,
    n_rows: usize,
    n_cols: usize,
) -> (usize, usize, usize, usize, usize, usize, usize, Vec<u8>) {
    let cell_h = h / 2;
    let cell_w = w / 2;
    let label_h = 14usize; // 7px font @ 2× scale
    let header_w = 156usize; // room for "K x cortex s" (12 chars × 12 px + margin) at scale 2
    let pad = 6usize;
    let row_h = label_h + cell_h;
    let total_w = header_w + n_cols * cell_w + (n_cols + 1) * pad;
    let total_h = n_rows * row_h + (n_rows + 1) * pad;
    let mut buf = vec![245u8; total_w * total_h * 4]; // light gray background
    for i in 0..total_w * total_h {
        buf[i * 4 + 3] = 255;
    }
    (
        cell_w, cell_h, label_h, pad, header_w, total_w, total_h, buf,
    )
}

/// Compact per-cell label — just the threshold info, no approach prefix.
fn cell_label_short(approach: ThresholdApproach, p: f64, threshold: f64) -> String {
    match approach {
        ThresholdApproach::AllenFixed => format!("thr={:.2}", p),
        ThresholdApproach::SnlcGlobalStd => format!("{:.1}xsg={:.3}", p, threshold),
        ThresholdApproach::CortexMaskedStd => format!("{:.1}xsc={:.3}", p, threshold),
    }
}

// genuinely 8 distinct inputs (maps, thresholds, std stats, rows, paths)
// Justified `#[allow]`: internal dev-figure helper assembling the threshold-
// sweep grid. Inputs are mostly distinct (two maps, a usize, a rows array, a
// path, an optional anatomical buffer); the two `_std: f64` args are the only
// swap-able pair. CLI tooling, not a production API.
#[allow(clippy::too_many_arguments)]
fn render_threshold_grid_patches(
    vfs_smooth: &ndarray::Array2<f64>,
    cortex_mask: &ndarray::Array2<bool>,
    small_patch_thr: usize,
    global_std: f64,
    cortex_std: f64,
    rows: &[(ThresholdApproach, &str, [f64; 5]); 3],
    out_dir: &std::path::Path,
    anatomical: Option<&[u8]>,
) {
    let (h, w) = vfs_smooth.dim();
    let n_rows = rows.len();
    let n_cols = 5;
    let (cell_w, cell_h, label_h, pad, header_w, total_w, total_h, mut buf) =
        build_grid_canvas(h, w, n_rows, n_cols);

    for (row_idx, (approach, row_label, params)) in rows.iter().enumerate() {
        // Row header in the left margin, vertically centered against the cell.
        let row_y = pad + row_idx * (label_h + cell_h + pad) + label_h + cell_h / 2 - 7;
        draw_text(
            &mut buf,
            total_w,
            total_h,
            4,
            row_y,
            row_label,
            (30, 30, 30),
            2,
        );

        for (col_idx, &p) in params.iter().enumerate() {
            let threshold = threshold_for_cell(*approach, p, global_std, cortex_std);
            let cell_text = cell_label_short(*approach, p, threshold);

            let (area_labels, area_signs) = isi_analysis::segmentation::segment_threshold_only(
                vfs_smooth,
                cortex_mask,
                threshold,
                small_patch_thr,
            );
            let n_patches = area_signs.len();

            let mut full = vec![255u8; h * w * 4];
            if let Some(anat) = anatomical {
                if anat.len() == h * w {
                    for i in 0..h * w {
                        let g = anat[i];
                        full[i * 4] = g;
                        full[i * 4 + 1] = g;
                        full[i * 4 + 2] = g;
                        full[i * 4 + 3] = 255;
                    }
                }
            } else {
                for i in 0..h * w {
                    full[i * 4 + 3] = 255;
                }
            }
            for r in 0..h {
                for c in 0..w {
                    let l = area_labels[[r, c]];
                    if l == 0 {
                        continue;
                    }
                    let sign = area_signs[(l - 1) as usize];
                    let (rc, gc, bc) = if sign > 0 {
                        (220, 50, 50)
                    } else {
                        (50, 50, 220)
                    };
                    full[(r * w + c) * 4] = rc;
                    full[(r * w + c) * 4 + 1] = gc;
                    full[(r * w + c) * 4 + 2] = bc;
                }
            }

            let cell_x = header_w + pad + col_idx * (cell_w + pad);
            let cell_y = pad + row_idx * (label_h + cell_h + pad) + label_h;
            place_downsampled_cell(
                &full, w, h, &mut buf, total_w, cell_x, cell_y, cell_w, cell_h,
            );

            // Per-cell label above the cell.
            let label_str = format!("{}  n={}", cell_text, n_patches);
            draw_text(
                &mut buf,
                total_w,
                total_h,
                cell_x,
                cell_y - label_h,
                &label_str,
                (30, 30, 30),
                2,
            );
        }
    }

    let path = out_dir.join("threshold_sweep_patches.png");
    write_rgba_png(&path, total_w as u32, total_h as u32, &buf);
    println!("  threshold_sweep_patches.png ({total_w}x{total_h}, {n_rows}x{n_cols} grid)");
}

fn render_threshold_grid_vfs(
    vfs_smooth: &ndarray::Array2<f64>,
    global_std: f64,
    cortex_std: f64,
    rows: &[(ThresholdApproach, &str, [f64; 5]); 3],
    out_dir: &std::path::Path,
    anatomical: Option<&[u8]>,
) {
    let (h, w) = vfs_smooth.dim();
    let n_rows = rows.len();
    let n_cols = 5;
    let (cell_w, cell_h, label_h, pad, header_w, total_w, total_h, mut buf) =
        build_grid_canvas(h, w, n_rows, n_cols);

    for (row_idx, (approach, row_label, params)) in rows.iter().enumerate() {
        let row_y = pad + row_idx * (label_h + cell_h + pad) + label_h + cell_h / 2 - 7;
        draw_text(
            &mut buf,
            total_w,
            total_h,
            4,
            row_y,
            row_label,
            (30, 30, 30),
            2,
        );

        for (col_idx, &p) in params.iter().enumerate() {
            let threshold = threshold_for_cell(*approach, p, global_std, cortex_std);
            let cell_text = cell_label_short(*approach, p, threshold);

            // Render vfs_smooth in jet [-1, +1] only where |VFS| ≥ threshold;
            // pixels below threshold render as background (white).
            let mut full = vec![255u8; h * w * 4];
            if let Some(anat) = anatomical {
                if anat.len() == h * w {
                    for i in 0..h * w {
                        let g = anat[i];
                        full[i * 4] = g;
                        full[i * 4 + 1] = g;
                        full[i * 4 + 2] = g;
                        full[i * 4 + 3] = 255;
                    }
                }
            } else {
                for i in 0..h * w {
                    full[i * 4 + 3] = 255;
                }
            }
            for r in 0..h {
                for c in 0..w {
                    let v = vfs_smooth[[r, c]];
                    if !v.is_finite() || v.abs() < threshold {
                        continue;
                    }
                    let t = (0.5 + 0.5 * v.clamp(-1.0, 1.0)).clamp(0.0, 1.0);
                    let (rc, gc, bc) = jet(t);
                    full[(r * w + c) * 4] = rc;
                    full[(r * w + c) * 4 + 1] = gc;
                    full[(r * w + c) * 4 + 2] = bc;
                }
            }

            let cell_x = header_w + pad + col_idx * (cell_w + pad);
            let cell_y = pad + row_idx * (label_h + cell_h + pad) + label_h;
            place_downsampled_cell(
                &full, w, h, &mut buf, total_w, cell_x, cell_y, cell_w, cell_h,
            );

            draw_text(
                &mut buf,
                total_w,
                total_h,
                cell_x,
                cell_y - label_h,
                &cell_text,
                (30, 30, 30),
                2,
            );
        }
    }

    let path = out_dir.join("threshold_sweep_vfs.png");
    write_rgba_png(&path, total_w as u32, total_h as u32, &buf);
    println!("  threshold_sweep_vfs.png ({total_w}x{total_h}, {n_rows}x{n_cols} grid)");
}

/// 2×2 mean downsample of a full-resolution RGBA cell into the composite.
// genuinely 9 distinct inputs (src + dims, dst + dims, offsets, cell dims)
// Justified `#[allow]`: internal grid-compositor copying one downsampled cell
// into the canvas. Several `usize` geometry args (source/cell dims + offsets)
// ARE swap-able; it has two co-located call sites in this dev-only file and is
// not a production API, so a justified allow is preferred over a param object.
#[allow(clippy::too_many_arguments)]
fn place_downsampled_cell(
    full: &[u8],
    w: usize,
    h: usize,
    composite: &mut [u8],
    total_w: usize,
    ox: usize,
    oy: usize,
    cell_w: usize,
    cell_h: usize,
) {
    for dr in 0..cell_h {
        for dc in 0..cell_w {
            let mut sr = 0u32;
            let mut sg = 0u32;
            let mut sb = 0u32;
            for ddr in 0..2 {
                for ddc in 0..2 {
                    let r = dr * 2 + ddr;
                    let c = dc * 2 + ddc;
                    if r >= h || c >= w {
                        continue;
                    }
                    let i = (r * w + c) * 4;
                    sr += full[i] as u32;
                    sg += full[i + 1] as u32;
                    sb += full[i + 2] as u32;
                }
            }
            let dst = ((oy + dr) * total_w + (ox + dc)) * 4;
            composite[dst] = (sr / 4) as u8;
            composite[dst + 1] = (sg / 4) as u8;
            composite[dst + 2] = (sb / 4) as u8;
            composite[dst + 3] = 255;
        }
    }
}

// =============================================================================
// dev_figures: default layout, meta.json
//
// See docs/DEV_FIGURES.md.
// =============================================================================

/// `<repo_root>/dev_figures/<oisi_stem>/<device>-<utc>/`
pub(crate) fn default_figures_dir(
    oisi_path: &std::path::Path,
    _params: &isi_analysis::AnalysisParams,
) -> std::path::PathBuf {
    let stem = oisi_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".into());
    let device = isi_analysis::compute::device_tag();
    let unix_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let ts = format_utc_yyyymmddthhmm(unix_secs);

    repo_root()
        .join("dev_figures")
        .join(stem)
        .join(format!("{device}-{ts}"))
}

/// Walk up from this crate's manifest dir to find the workspace root (the
/// directory containing the workspace `Cargo.toml`). Falls back to the parent
/// of `CARGO_MANIFEST_DIR`.
pub(crate) fn repo_root() -> std::path::PathBuf {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or(manifest)
}

/// Format a unix timestamp (seconds) as `YYYYMMDDTHHMM` in UTC, with no
/// external date dependency. Uses Howard Hinnant's `civil_from_days`.
fn format_utc_yyyymmddthhmm(unix_secs: i64) -> String {
    let days = unix_secs.div_euclid(86400);
    let secs_today = unix_secs.rem_euclid(86400);
    let hour = secs_today / 3600;
    let minute = (secs_today % 3600) / 60;

    // civil_from_days — converts days-since-1970-01-01 to (y, m, d).
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    if m <= 2 {
        y += 1;
    }
    format!("{y:04}{m:02}{d:02}T{hour:02}{minute:02}")
}

fn git_capture(args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8(output.stdout).ok()?;
    let s = s.trim();
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

/// Write `meta.json` recording the full reproduction context for the figures
/// in `dir`. Uses portable identifiers (animal_id, created_at) so the
/// directory is shareable across machines.
pub(crate) fn write_meta_json(
    dir: &std::path::Path,
    oisi_path: &std::path::Path,
    _params: &isi_analysis::AnalysisParams,
) {
    let filename = oisi_path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let identity = isi_analysis::io::read_acquisition_identity(oisi_path).ok();

    let unix_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let stamp = format_utc_yyyymmddthhmm(unix_secs);
    // Pretty ISO-8601 for the meta.json human reader.
    let ts_iso = format!(
        "{}-{}-{}T{}:{}:00Z",
        &stamp[0..4],
        &stamp[4..6],
        &stamp[6..8],
        &stamp[9..11],
        &stamp[11..13],
    );

    let git_sha = git_capture(&["rev-parse", "--short=7", "HEAD"]);
    let git_branch = git_capture(&["rev-parse", "--abbrev-ref", "HEAD"]);
    let git_dirty = std::process::Command::new("git")
        .args(["diff", "--quiet"])
        .status()
        .map(|s| !s.success())
        .ok();

    // Source the analysis_params tree from the .oisi (the canonical
    // record). If the file hasn't been analyzed yet, fall back to the
    // current config tree.
    let analysis_params_tree = isi_analysis::io::read_analysis_params_attr(oisi_path)
        .ok()
        .flatten()
        .unwrap_or_else(|| match load_config_store() {
            Ok(store) => {
                // Fallback: current analysis config (the tagged `AnalysisConfig`).
                serde_json::to_value(store.analysis()).unwrap_or(serde_json::Value::Null)
            }
            Err(_) => serde_json::Value::Null,
        });

    let meta = serde_json::json!({
        "source": {
            "filename": filename,
            "animal_id": identity.as_ref().map(|i| &i.animal_id),
            "created_at": identity.as_ref().map(|i| &i.created_at),
        },
        "device": isi_analysis::compute::backend_info(),
        "git_sha": git_sha,
        "git_branch": git_branch,
        "git_dirty": git_dirty,
        "timestamp_utc": ts_iso,
        "analysis_params": analysis_params_tree,
    });

    let path = dir.join("meta.json");
    match serde_json::to_string_pretty(&meta) {
        Ok(s) => {
            if let Err(e) = std::fs::write(&path, s) {
                eprintln!("  failed to write meta.json: {e}");
            } else {
                println!("  meta.json");
            }
        }
        Err(e) => eprintln!("  meta.json serialize failed: {e}"),
    }
}
