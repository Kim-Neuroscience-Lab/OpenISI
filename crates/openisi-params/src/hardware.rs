//! Hardware context — runtime hardware capabilities injected into the registry.
//!
//! Not persisted. Camera capabilities arrive on camera connect, monitor info
//! on display select, measured refresh on display validate.

/// Runtime hardware context. Updated when hardware state changes.
/// Constraint functions read these to compute effective bounds.
#[derive(Debug, Clone, Default)]
pub struct HardwareContext {
    // Camera capabilities (set on camera connect)
    pub camera_min_exposure_us: Option<u32>,
    pub camera_max_exposure_us: Option<u32>,
    pub camera_max_binning: Option<u16>,

    // Monitor info (set on display select)
    pub monitor_width_px: Option<u32>,
    pub monitor_height_px: Option<u32>,
    pub monitor_width_cm: Option<f64>,
    pub monitor_height_cm: Option<f64>,
    pub monitor_refresh_hz: Option<u32>,

    // Measured (set on display validate)
    pub measured_refresh_hz: Option<f64>,
}

/// Resolve the effective value of a hardware-influenced param.
///
/// Precedence (single source of truth — both `RegistrySnapshot` and
/// `Registry` delegate here, never reimplement):
///
///   1. **user override** — if the user has explicitly set a value via
///      the UI or a TOML overlay, that wins unconditionally.
///   2. **hardware-detected** — EDID / camera SDK / wgpu surface value,
///      gated through a sanity predicate (typically "value > 0").
///   3. **None** — no override and no usable hardware reading; the caller
///      decides whether that's an error or a fall-back-to-shipped.
///
/// `raw_user_value` is the registry's current value for the param (the
/// shipped default plus any user override, but *not* hardware data).
/// `is_user_override` is true iff the user has actively set this param.
/// `hardware_value` is the raw EDID/SDK reading. `is_valid` filters it
/// (e.g. `|w| *w > 0.0` rejects sentinel zeros).
pub fn effective_hardware_value<T: Copy>(
    is_user_override: bool,
    raw_user_value: T,
    hardware_value: Option<T>,
    is_valid: impl FnOnce(&T) -> bool,
) -> Option<T> {
    if is_user_override {
        return Some(raw_user_value);
    }
    hardware_value.filter(|v| is_valid(v))
}
