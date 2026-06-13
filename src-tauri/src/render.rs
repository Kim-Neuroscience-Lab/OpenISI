//! Shared scalar-map renderer — the single Rust implementation of the
//! `MapMeta` → RGBA figure mapping, used by BOTH the headless figure exporter
//! (`bin/headless.rs`) and the Tauri `export_map_png` command. Previously each
//! had its own colormap + range logic (and the command auto-fit min/max with a
//! jet-only palette, silently ignoring the per-map palette/range/sentinel that
//! `MapMeta` carries). This is the one place those decisions live.
//!
//! The interactive GUI renders in JS/canvas across the WASM boundary; it reads
//! the same `MapMeta` attrs, so the two agree by construction. The colormaps
//! here are matplotlib-faithful (pinned by goldens in this module's tests).

use isi_analysis::MapMeta;
use ndarray::Array2;
use std::path::Path;

/// Render a scalar `/results` map to an RGBA buffer using ONLY the dataset's
/// `MapMeta` (palette, display range, wrap period, sentinel semantics) — zero
/// name-matching, zero auto-fit. Returns `(rgba, human_label)`.
///
/// `anatomical`: optional grayscale `[h*w]` underlay shown beneath
/// sentinel-zero pixels (when the meta declares a `zero_means` sentinel and the
/// dims match) — the conventional vasculature-under-patches view.
pub fn render_map(
    data: &Array2<f64>,
    meta: &MapMeta,
    anatomical: Option<&[u8]>,
) -> (Vec<u8>, String) {
    let (h, w) = data.dim();

    let lo = meta.display_min;
    let hi = meta.display_max;
    let range = (hi - lo).max(1e-10);
    let wrap_period = (meta.wrap_period > 0.0).then_some(meta.wrap_period);
    let has_zero_sentinel = !meta.zero_means.is_empty();

    let palette: fn(f64) -> (u8, u8, u8) = match meta.palette.as_ref() {
        "hsv_circular" => hsv_circular,
        "hot" => hot,
        "jet" => jet,
        "binary" | "categorical" => jet, // fallbacks; bool/labels handled elsewhere
        _ => jet,
    };

    // Sentinel-zero pixels render as the anatomical underlay (when
    // provided + dimensions match) or stay white.
    let anat_for_meta = has_zero_sentinel
        .then_some(anatomical)
        .flatten()
        .filter(|a| a.len() == h * w);

    let mut rgba = vec![255u8; h * w * 4];
    if let Some(anat) = anat_for_meta {
        for i in 0..h * w {
            let g = anat[i];
            rgba[i * 4] = g;
            rgba[i * 4 + 1] = g;
            rgba[i * 4 + 2] = g;
            rgba[i * 4 + 3] = 255;
        }
    }
    for (i, &v) in data.iter().enumerate() {
        if has_zero_sentinel && v == 0.0 {
            continue;
        }
        if !v.is_finite() {
            continue;
        }
        let t = match wrap_period {
            Some(p) => ((v - lo).rem_euclid(p)) / p,
            None => ((v - lo) / range).clamp(0.0, 1.0),
        };
        let (r, g, b) = palette(t);
        rgba[i * 4] = r;
        rgba[i * 4 + 1] = g;
        rgba[i * 4 + 2] = b;
        rgba[i * 4 + 3] = 255;
    }

    // Descriptive label uses the meta's units to pick the formatter.
    let unit_label = |v: f64| -> String {
        match meta.units.as_ref() {
            "rad" => format!("{:+.1}°", v.to_degrees()),
            "deg" => format!("{v:+.1}°"),
            _ => format!("{v:+.3}"),
        }
    };
    let cmap_label = match meta.palette.as_ref() {
        "hsv_circular" => "HSV",
        "hot" if has_zero_sentinel => "hot-sentinel",
        "hot" => "hot (normalized)",
        "jet" if has_zero_sentinel => "jet-sentinel",
        "jet" => "jet",
        other => other,
    };
    let label = format!("{cmap_label} [{}, {}]", unit_label(lo), unit_label(hi));
    (rgba, label)
}

/// Write an RGBA buffer to a PNG file (wraps the `png` crate). Logs and returns
/// on error rather than panicking — figure export is best-effort.
pub fn write_rgba_png(path: &Path, w: u32, h: u32, rgba: &[u8]) {
    let file = match std::fs::File::create(path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("  Failed to create {}: {e}", path.display());
            return;
        }
    };
    let mut encoder = png::Encoder::new(file, w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = match encoder.write_header() {
        Ok(w) => w,
        Err(e) => {
            eprintln!("  PNG header error: {e}");
            return;
        }
    };
    if let Err(e) = writer.write_image_data(rgba) {
        eprintln!("  PNG write error: {e}");
    }
}

/// Circular HSV colormap on `t ∈ [0, 1]` (hue 0 → 360°, full saturation and
/// value). Right choice for phase / angular data because there is no
/// discontinuity at the `±π` wrap.
pub fn hsv_circular(t: f64) -> (u8, u8, u8) {
    let t = t.clamp(0.0, 1.0);
    let h = (t * 6.0).rem_euclid(6.0);
    let c = 1.0_f64;
    let x = c * (1.0 - (h.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match h as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    ((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

/// Matplotlib `hot` colormap: black → red → yellow → white. Allen amplitude
/// convention (`cmap='hot'`). Linear-segmented:
///   r: 0..0.365   → 0..1, then 1
///   g: 0.365..0.746 → 0..1, then 1
///   b: 0.746..1.0 → 0..1
pub fn hot(t: f64) -> (u8, u8, u8) {
    let t = t.clamp(0.0, 1.0);
    let r = if t <= 0.365 { t / 0.365 } else { 1.0 };
    let g = if t <= 0.365 {
        0.0
    } else if t <= 0.746 {
        (t - 0.365) / (0.746 - 0.365)
    } else {
        1.0
    };
    let b = if t <= 0.746 {
        0.0
    } else {
        (t - 0.746) / (1.0 - 0.746)
    };
    ((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

/// Matplotlib `jet` colormap (the classic blue→cyan→yellow→red rainbow). Used
/// for VFS / sign / magnification maps and the threshold-sweep grids.
pub fn jet(t: f64) -> (u8, u8, u8) {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression-lock the three colormaps at their anchor points. These are the
    /// classic matplotlib `jet`/`hot` segmented maps and a full-saturation HSV
    /// wheel; this pins the exact RGB so a future edit can't silently shift a
    /// figure's colors (the Rust renderer and the JS GUI must stay in agreement,
    /// both keyed to these matplotlib conventions).
    #[test]
    fn colormaps_pinned_at_anchor_points() {
        // jet: classic blue → cyan → yellow → red. Ends are half-bright.
        assert_eq!(jet(0.0), (0, 0, 127));
        assert_eq!(jet(0.5), (127, 255, 127));
        assert_eq!(jet(1.0), (127, 0, 0));

        // hot: black → red → yellow → white (Allen amplitude convention).
        assert_eq!(hot(0.0), (0, 0, 0));
        assert_eq!(hot(0.5), (255, 90, 0));
        assert_eq!(hot(1.0), (255, 255, 255));

        // hsv_circular: pure R/G/B at thirds, wrapping at the ±π seam.
        assert_eq!(hsv_circular(0.0), (255, 0, 0));
        assert_eq!(hsv_circular(1.0 / 3.0), (0, 255, 0));
        assert_eq!(hsv_circular(2.0 / 3.0), (0, 0, 255));
        assert_eq!(hsv_circular(1.0), hsv_circular(0.0), "wraps with no seam");

        // Out-of-range inputs clamp rather than panic.
        assert_eq!(jet(-1.0), jet(0.0));
        assert_eq!(hot(2.0), hot(1.0));
    }
}
