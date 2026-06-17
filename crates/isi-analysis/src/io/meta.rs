//! Per-dataset rendering metadata (`MapMeta`) for `/results/*` datasets, plus
//! the result-type classifier. This is the data-layer ↔ renderer contract: the
//! single source of truth mapping a result's NAME (and shape) to its palette,
//! units, display range, and sentinel semantics. Split out of `io.rs` because
//! it is a distinct concern (presentation metadata — it changes when result
//! leaves or their visualization change, not when the HDF5 I/O does).
//!
//! The low-level HDF5 attribute helpers (`read_str_attr`, `write_str_attr`,
//! etc.) live in the parent `io` module and are reached via `super::`.

use ndarray::Array2;

use super::{read_f64_attr, read_str_attr, write_f64_attr, write_str_attr};
use crate::{AcquisitionProperties, AnalysisError};

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
pub(crate) fn map_meta_bool() -> MapMeta {
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
pub(crate) fn map_meta_labels() -> MapMeta {
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
pub fn meta_for_f64(name: &str, data: &Array2<f64>, acquisition: &AcquisitionProperties) -> MapMeta {
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
            display_max: std::f64::consts::PI,
            wrap_period: std::f64::consts::TAU,
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
            display_max: 1.0,
            wrap_period: 0.0,
            nan_means: lit(""),
            zero_means: lit(""),
        },
        "vfs_smoothed_thresholded" => MapMeta {
            palette: lit("jet"),
            units: lit("unitless"),
            display_min: -1.0,
            display_max: 1.0,
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
        // Hemodynamic delay (SNLC Gprocesskret delay_hor/_vert): a full-frame
        // angular magnitude in degrees, (0, 180]. Non-circular (it's a delay
        // length, not a wrapping phase) → jet over the finite data range, no
        // sentinel (0 is a valid near-flip delay, not "no data").
        "azi_delay" | "alt_delay" => {
            let (lo, hi) = finite_range(data);
            MapMeta {
                palette: lit("jet"),
                units: lit("deg"),
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
        // Polar angle: the SNLC `kmap_ang` companion to eccentricity. A circular
        // visual-field coordinate over [-180, 180]°, so it renders with the same
        // wrapping HSV palette as the phase maps (period 360°). `0.0` is the
        // patch-scope sentinel (zero outside `area_labels > 0`), as for ecc.
        "polar_angle" => MapMeta {
            palette: lit("hsv_circular"),
            units: lit("deg"),
            display_min: -180.0,
            display_max: 180.0,
            wrap_period: 360.0,
            nan_means: lit(""),
            zero_means: lit("outside_patch"),
        },
        // Magnification: Allen cortical magnification factor (px²/deg²) — the
        // reciprocal of the Jacobian determinant, high where cortex is
        // magnified. ROI-gated, so zeros mean "outside patch".
        "magnification" => {
            let (lo, hi) = sentinel_percentile(data, 0.02, 0.98);
            MapMeta {
                palette: lit("jet"),
                units: lit("px^2/deg^2"),
                display_min: lo,
                display_max: hi,
                wrap_period: 0.0,
                nan_means: lit(""),
                zero_means: lit("outside_patch"),
            }
        }
        // Unmasked Jacobian determinant |det J| (deg²/px²) — full frame (not
        // ROI-gated), so zeros are genuine low-magnification values, not
        // "outside patch". This is what the split criterion reads.
        "magnification_raw" => {
            let (lo, hi) = sentinel_percentile(data, 0.02, 0.98);
            MapMeta {
                palette: lit("jet"),
                units: lit("deg^2/px^2"),
                display_min: lo,
                display_max: hi,
                wrap_period: 0.0,
                nan_means: lit(""),
                zero_means: lit(""),
            }
        }
        // Magnification preferred axis (SNLC prefAxisMF): an axis in degrees,
        // periodic over 180° (an axis, not a vector) → wrapping HSV palette.
        // Full-frame; no sentinel.
        "magnification_axis" => MapMeta {
            palette: lit("hsv_circular"),
            units: lit("deg"),
            display_min: 0.0,
            display_max: 180.0,
            wrap_period: 180.0,
            nan_means: lit("isotropic"),
            zero_means: lit(""),
        },
        // Magnification distortion (SNLC Distrtion): anisotropy coherence,
        // bounded [0, 1] by construction → hot palette over the fixed range.
        "magnification_distortion" => MapMeta {
            palette: lit("hot"),
            units: lit("unitless"),
            display_min: 0.0,
            display_max: 1.0,
            wrap_period: 0.0,
            nan_means: lit(""),
            zero_means: lit(""),
        },
        // Reliability maps (Allen / Engel cross-cycle vector coherence):
        // bounded [0, 1] by construction. Hot palette over the full
        // range makes the cortex region pop visually.
        "reliability_azi_fwd"
        | "reliability_azi_rev"
        | "reliability_alt_fwd"
        | "reliability_alt_rev" => MapMeta {
            palette: lit("hot"),
            units: lit("unitless"),
            display_min: 0.0,
            display_max: 1.0,
            wrap_period: 0.0,
            nan_means: lit(""),
            zero_means: lit(""),
        },
        // Spectral responsiveness maps (spectral SNR + Allen power-SNR):
        // per-orientation. No canonical fixed range — jet over the 2-98
        // percentile of finite values.
        "spectral_snr_azi" | "spectral_snr_alt" | "allen_power_snr_azi" | "allen_power_snr_alt" => {
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
            if v < lo {
                lo = v;
            }
            if v > hi {
                hi = v;
            }
        }
    }
    if !lo.is_finite() {
        return (0.0, 1.0);
    }
    if (hi - lo).abs() < 1e-12 {
        (lo, lo + 1.0)
    } else {
        (lo, hi)
    }
}

/// Two-sided percentile of finite, non-zero values — the right range
/// for sentinel-zero maps (eccentricity, magnification) where `0.0`
/// means "no data" and shouldn't influence the colorbar.
fn sentinel_percentile(data: &Array2<f64>, p_lo: f64, p_hi: f64) -> (f64, f64) {
    let mut vals: Vec<f64> = data
        .iter()
        .copied()
        .filter(|v| v.is_finite() && *v != 0.0)
        .collect();
    if vals.is_empty() {
        return (0.0, 1.0);
    }
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = vals.len();
    let idx = |p: f64| -> usize { ((p * (n - 1) as f64).round() as usize).min(n - 1) };
    let lo = vals[idx(p_lo)];
    let hi = vals[idx(p_hi)];
    if (hi - lo).abs() < 1e-12 {
        (lo, lo + 1.0)
    } else {
        (lo, hi)
    }
}

/// Attach the `MapMeta` fields to a dataset as HDF5 attributes.
pub(crate) fn attach_meta(dataset: &hdf5::Dataset, m: &MapMeta) -> Result<(), AnalysisError> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn acq() -> AcquisitionProperties {
        AcquisitionProperties {
            azi_angular_range: 120.0,
            alt_angular_range: 110.0,
            offset_azi: 0.0,
            offset_alt: 0.0,
            ..Default::default()
        }
    }

    /// Pin the renderer contract — the `name → (palette, units, range,
    /// sentinel)` mapping renderers depend on. `meta_for_f64`'s `_` arm is an
    /// *intentional* sensible default (jet / unitless / 2–98 percentile) for
    /// unlisted maps — so this does NOT demand an arm per result; it locks the
    /// EXPLICIT leaves so a future edit can't silently change a colorbar, wrap
    /// period, units, or sentinel semantics without a test failing.
    #[test]
    fn meta_for_f64_pins_explicit_render_contract() {
        let z = Array2::<f64>::zeros((4, 4));
        let a = acq();

        // Radian phases: circular HSV, period 2π.
        let m = meta_for_f64("azi_phase", &z, &a);
        assert_eq!(m.palette, "hsv_circular");
        assert_eq!(m.units, "rad");
        assert_eq!(m.wrap_period, std::f64::consts::TAU);

        // Degree phases: period = the angular range, centered on the offset.
        let m = meta_for_f64("azi_phase_degrees", &z, &a);
        assert_eq!(m.units, "deg");
        assert_eq!(m.wrap_period, 120.0);
        assert_eq!(m.display_min, -60.0);
        assert_eq!(m.display_max, 60.0);

        // VFS family: jet over ±1; thresholded variant flags its 0-sentinel.
        let m = meta_for_f64("vfs", &z, &a);
        assert_eq!((m.palette.as_ref(), m.display_min, m.display_max), ("jet", -1.0, 1.0));
        assert_eq!(
            meta_for_f64("vfs_smoothed_thresholded", &z, &a).zero_means,
            "below_threshold"
        );

        // Magnification leaves: Allen CMF (px²/deg², ROI-gated) vs the raw
        // determinant (deg²/px², full-frame) — the units must not get swapped.
        let m = meta_for_f64("magnification", &z, &a);
        assert_eq!(m.units, "px^2/deg^2");
        assert_eq!(m.zero_means, "outside_patch");
        let m = meta_for_f64("magnification_raw", &z, &a);
        assert_eq!(m.units, "deg^2/px^2");
        assert_eq!(m.zero_means, "");

        // Eccentricity: degrees, sentinel-zero outside patches.
        let m = meta_for_f64("eccentricity", &z, &a);
        assert_eq!(m.units, "deg");
        assert_eq!(m.zero_means, "outside_patch");

        // Amplitude suffix routes to the hot palette.
        assert_eq!(meta_for_f64("azi_amplitude", &z, &a).palette, "hot");

        // Unlisted name → intentional default (jet / unitless), NOT a panic.
        let m = meta_for_f64("some_future_map", &z, &a);
        assert_eq!(m.palette, "jet");
        assert_eq!(m.units, "unitless");
    }

    /// `classify_result_type` routes bool masks and label maps to their typed
    /// readers; everything 2-D else is a scalar map, 1-D is a sign array.
    #[test]
    fn classify_result_type_routes_known_datasets() {
        assert_eq!(classify_result_type("cortex_mask", &[4, 4], None), "bool_mask");
        assert_eq!(classify_result_type("area_borders", &[4, 4], None), "bool_mask");
        assert_eq!(classify_result_type("area_labels", &[4, 4], None), "label_map");
        assert_eq!(classify_result_type("area_signs", &[7], None), "sign_array");
        assert_eq!(classify_result_type("magnification", &[4, 4], None), "scalar_map");
    }
}
