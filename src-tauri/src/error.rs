//! Unified application error type for Tauri commands.
//!
//! Every command returns `Result<T, AppError>`. Tauri serializes the error
//! via `serde::Serialize` — we emit a structured `AppErrorWire` with a
//! stable category + machine-readable code + human-readable message + a
//! `details` JSON for category-specific info. Frontend code can render
//! `message` directly and reveal `details` on user click.
//!
//! **Errors are `thiserror` enums; their stable codes are `strum`-derived.**
//! Each error enum carries its `E_*` code as a `#[strum_discriminants(...)]`
//! attribute *on the variant* — the single, co-located source of truth, with no
//! hand-maintained mapping table. The library errors own their codes in their own
//! crates: `isi_analysis::AnalysisError` (analysis pipeline) and
//! [`AcquisitionError`] (real-time worker threads; `From<crossbeam SendError>` lets
//! a dead receiver exit the worker via `?`). [`AppError`] is the Tauri-IPC façade:
//! it wraps them via `#[from]`, derives its *category* via `strum::IntoStaticStr`,
//! and [`AppError::code`] delegates to the wrapped error so each code has one home.
//!
//! [`error_catalog`] enumerates every `(code, category)` pair from those `strum`
//! derivations — the cross-language SSoT that generates the frontend's
//! `ui/src/lib/error-codes.generated.js` (drift-guarded by `error_codes_js_in_sync`),
//! so the JS branches on generated constants and can never drift from the backend.

use parking_lot::{Mutex, MutexGuard};

// ----------------------------------------------------------------------------
// AcquisitionError — real-time worker thread errors
// ----------------------------------------------------------------------------

/// Errors that occur inside the camera/stimulus/sequencer worker threads
/// during acquisition. These can't `?`-propagate out of a `loop`, so the
/// pattern is: each thread's `run()` is a thin wrapper around a
/// `run_inner() -> Result<(), AcquisitionError>`. On `Err`, the wrapper
/// emits a `Fatal` event before exiting so the main thread can stop the
/// run cleanly and offer partial save.
///
/// Each variant carries its stable `E_*` code as a `strum` attribute (the SSoT,
/// co-located with the variant); the companion fieldless [`AcquisitionCode`] enum
/// gives both the runtime lookup and compile-time enumeration from that one
/// declaration — same pattern as [`isi_analysis::AnalysisError`].
#[derive(Debug, thiserror::Error, strum::EnumDiscriminants)]
#[strum_discriminants(
    name(AcquisitionCode),
    vis(pub),
    derive(strum::IntoStaticStr, strum::EnumIter),
)]
pub enum AcquisitionError {
    /// PCO SDK / camera hardware error message verbatim.
    #[error("Camera: {0}")]
    #[strum_discriminants(strum(serialize = "E_CAMERA"))]
    Camera(String),

    /// Stimulus thread / monitor render error message verbatim.
    #[error("Stimulus: {0}")]
    #[strum_discriminants(strum(serialize = "E_STIMULUS"))]
    Stimulus(String),

    /// `crossbeam_channel::send` failed because the receiver is gone —
    /// indicates the consumer thread (typically the main thread) has
    /// died. The worker must exit so we don't accumulate buffered
    /// frames forever.
    #[error("Channel closed: {context}")]
    #[strum_discriminants(strum(serialize = "E_CHANNEL_CLOSED"))]
    ChannelClosed { context: &'static str },

    /// Frame drop exceeded the configured catastrophic threshold during
    /// acquisition. Carries a human-readable description.
    #[error("Frame drop: {0}")]
    #[strum_discriminants(strum(serialize = "E_FRAME_DROP"))]
    FrameDrop(String),

    /// Hardware disconnected mid-acquisition. Distinct from `Camera`
    /// because the recovery policy is different (stop the run, mark
    /// `acquisition_complete=false`).
    #[error("Disconnected mid-acquisition")]
    #[strum_discriminants(strum(serialize = "E_DISCONNECTED"))]
    DisconnectedDuringAcquisition,
}

impl AcquisitionError {
    /// Stable machine-readable code, derived from the variant's `strum` attribute.
    pub fn code(&self) -> &'static str {
        AcquisitionCode::from(self).into()
    }
}

impl<T> From<crossbeam_channel::SendError<T>> for AcquisitionError {
    fn from(_: crossbeam_channel::SendError<T>) -> Self {
        Self::ChannelClosed {
            context: "acquisition channel",
        }
    }
}

// ----------------------------------------------------------------------------
// AppError — Tauri-IPC façade
// ----------------------------------------------------------------------------

/// Unified error type for all Tauri IPC commands. Wraps the crate-internal error
/// types via `#[from]` impls so handlers can use `?` throughout.
///
/// The per-variant `#[strum(serialize = …)]` is the **category** string (the SSoT,
/// co-located); [`AppError::category`] returns it via `IntoStaticStr`. The
/// **code** comes from [`AppError::code`] — delegated to the wrapped error for the
/// `Analysis`/`Acquisition` variants (so analysis/acquisition codes have a single
/// home in their own crate), and a co-located constant for the leaf variants.
#[derive(Debug, thiserror::Error, strum::IntoStaticStr)]
pub enum AppError {
    #[error("{0}")]
    #[strum(serialize = "Analysis")]
    Analysis(#[from] isi_analysis::AnalysisError),

    #[error("{0}")]
    #[strum(serialize = "Acquisition")]
    Acquisition(#[from] AcquisitionError),

    #[error("Hardware: {0}")]
    #[strum(serialize = "Hardware")]
    Hardware(String),

    #[error("I/O: {0}")]
    #[strum(serialize = "Io")]
    Io(#[from] std::io::Error),

    #[error("Config: {0}")]
    #[strum(serialize = "Config")]
    Config(String),

    #[error("Validation: {0}")]
    #[strum(serialize = "Validation")]
    Validation(String),

    #[error("Not available: {0}")]
    #[strum(serialize = "NotAvailable")]
    NotAvailable(String),
}

// Flatten openisi-params errors onto the matching AppError variant so
// the IPC wire format stays unchanged. ParamsError is the params crate's
// shell-agnostic error type; AppError is the Tauri-facing union.
impl From<openisi_params::ParamsError> for AppError {
    fn from(e: openisi_params::ParamsError) -> Self {
        use openisi_params::ParamsError::*;
        match e {
            Validation(s) => AppError::Validation(s),
            Config(s) => AppError::Config(s),
            NotAvailable(s) => AppError::NotAvailable(s),
            Io(e) => AppError::Io(e),
        }
    }
}

// The capture-write path composes the `oisi` format-I/O primitives directly
// (re-exported as `isi_analysis::io::*`), so handlers can surface an
// `OisiError` from `?`. Lift it onto the `Analysis` variant via
// `AnalysisError: From<OisiError>` — the format error's four variants carry the
// same IPC codes there (E_IO / E_HDF5 / E_INVALID_PACKAGE / E_MISSING_DATA), so
// the wire format is unchanged.
impl From<isi_analysis::OisiError> for AppError {
    fn from(e: isi_analysis::OisiError) -> Self {
        AppError::Analysis(isi_analysis::AnalysisError::from(e))
    }
}

// ----------------------------------------------------------------------------
// AppErrorWire — structured IPC payload for the frontend
// ----------------------------------------------------------------------------

/// Wire format for errors serialized to the frontend. Includes a stable
/// category + machine-readable code so the frontend can recognise known
/// error classes (e.g. show retry button for `E_CHANNEL_CLOSED`,
/// "delete attribute and re-run" for `E_INVALID_PACKAGE`), and a
/// `details` JSON for category-specific structured info.
///
/// The frontend's default display is `message`; UI affordances can
/// surface `details` on user click.
#[derive(Debug, serde::Serialize)]
pub struct AppErrorWire {
    /// Top-level category: "Analysis" | "Acquisition" | "Config" |
    /// "Hardware" | "Io" | "Validation" | "NotAvailable" | "Internal".
    pub category: &'static str,

    /// Stable machine-readable code, e.g. `"E_INVALID_PACKAGE"`. Stable
    /// across versions — frontends and scripts can pattern-match on it.
    pub code: &'static str,

    /// Human-readable message (from the underlying error's `Display`
    /// impl). Already includes the variant payload string.
    pub message: String,

    /// `.oisi` file path involved, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,

    /// Pipeline stage involved, if any (e.g. `"Computing retinotopy"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<&'static str>,

    /// Category-specific structured details, free-form JSON.
    #[serde(skip_serializing_if = "serde_json::Value::is_null")]
    pub details: serde_json::Value,
}

impl AppError {
    /// Stable top-level category (`"Analysis"`, `"Hardware"`, …), derived from the
    /// variant's `strum` attribute via `IntoStaticStr`.
    pub fn category(&self) -> &'static str {
        self.into()
    }

    /// Stable machine-readable code (`"E_HDF5"`, …). For the wrapper variants it
    /// delegates to the wrapped error's own `code()` (single home per crate); for
    /// the leaf variants it is the co-located constant.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Analysis(e) => e.code(),
            Self::Acquisition(e) => e.code(),
            Self::Hardware(_) => "E_HARDWARE",
            Self::Io(_) => "E_IO",
            Self::Config(_) => "E_CONFIG",
            Self::Validation(_) => "E_VALIDATION",
            Self::NotAvailable(_) => "E_NOT_AVAILABLE",
        }
    }

    /// Build the structured wire payload from this error.
    pub fn to_wire(&self) -> AppErrorWire {
        AppErrorWire {
            category: self.category(),
            code: self.code(),
            message: self.to_string(),
            file_path: None,
            stage: None,
            details: serde_json::Value::Null,
        }
    }
}

/// Every stable `(code, category)` pair the IPC surface can emit — the single
/// source of truth for the cross-language error contract. Built by enumerating
/// each error enum's codes from its `strum` discriminants (analysis + acquisition)
/// and the leaf `AppError` variants (via their own `code()`/`category()`), so no
/// code string is written twice. Drives the generated frontend catalog
/// (`ui/src/lib/error-codes.generated.js`); the `error_codes_js_in_sync` test
/// fails if the committed file drifts.
pub fn error_catalog() -> Vec<(&'static str, &'static str)> {
    use strum::IntoEnumIterator;
    let mut out: Vec<(&'static str, &'static str)> = Vec::new();
    for c in isi_analysis::AnalysisCode::iter() {
        out.push((c.into(), "Analysis"));
    }
    for c in AcquisitionCode::iter() {
        out.push((c.into(), "Acquisition"));
    }
    // AppError's own (non-delegating) façade variants — sourced from their
    // runtime `code()`/`category()` so the leaf codes are single-sourced.
    for e in [
        AppError::Hardware(String::new()),
        AppError::Io(std::io::Error::other("")),
        AppError::Config(String::new()),
        AppError::Validation(String::new()),
        AppError::NotAvailable(String::new()),
    ] {
        out.push((e.code(), e.category()));
    }
    out
}

/// Render the frontend error-code catalog (`error-codes.generated.js`) from
/// [`error_catalog`] — the cross-language SSoT. Distinct, sorted code + category
/// constants as frozen objects, so the JS frontend branches on named constants
/// instead of string literals that could drift from the backend. (Test-only: it
/// drives the `error_codes_js_in_sync` drift guard / regeneration.)
#[cfg(test)]
fn render_error_codes_js() -> String {
    let catalog = error_catalog();
    let mut codes: Vec<&'static str> = catalog.iter().map(|(c, _)| *c).collect();
    codes.sort_unstable();
    codes.dedup();
    let mut cats: Vec<&'static str> = catalog.iter().map(|(_, k)| *k).collect();
    cats.sort_unstable();
    cats.dedup();

    let mut s = String::new();
    s.push_str(
        "// @generated by `cargo test -p openisi error_codes_js_in_sync` — DO NOT EDIT.\n\
         // Regenerate after changing a Rust error enum:\n\
         //   OISI_REGEN_ERROR_CODES=1 cargo test -p openisi error_codes_js_in_sync\n\
         //\n\
         // Single source of truth: the Rust error enums (AppError / AnalysisError /\n\
         // AcquisitionError). Every AppErrorWire carries one of these `code`s and a\n\
         // `category`. Branch on ERROR_CODES.X in the frontend, never on a string literal.\n\
         \n\
         export const ERROR_CODES = Object.freeze({\n",
    );
    for c in &codes {
        s.push_str(&format!("  {c}: '{c}',\n"));
    }
    s.push_str("});\n\nexport const ERROR_CATEGORIES = Object.freeze({\n");
    for k in &cats {
        s.push_str(&format!("  {k}: '{k}',\n"));
    }
    s.push_str("});\n");
    s
}

/// Tauri 2 requires command error types to implement `Serialize`.
/// We serialize the structured `AppErrorWire` so the frontend gets
/// category + code + message + details.
impl serde::Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_wire().serialize(serializer)
    }
}

/// Convenience alias for command return types.
pub type AppResult<T> = std::result::Result<T, AppError>;

/// Lock a `parking_lot::Mutex<T>`. Infallible — `parking_lot` mutexes do not
/// poison, so this returns the guard directly. The `context` argument is
/// retained for call-site readability (documents *why* the lock is taken) and
/// future tracing instrumentation.
#[inline]
pub fn lock_state<'a, T>(mutex: &'a Mutex<T>, _context: &str) -> MutexGuard<'a, T> {
    mutex.lock()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analysis_error_serializes_as_wire() {
        let e = AppError::Analysis(isi_analysis::AnalysisError::InvalidPackage(
            "missing /analysis_params".into(),
        ));
        let v: serde_json::Value = serde_json::to_value(&e).unwrap();
        assert_eq!(v["category"], "Analysis");
        assert_eq!(v["code"], "E_INVALID_PACKAGE");
        assert!(v["message"].as_str().unwrap().contains("missing"));
    }

    #[test]
    fn acquisition_error_serializes_as_wire() {
        let e = AppError::Acquisition(AcquisitionError::ChannelClosed { context: "camera" });
        let v: serde_json::Value = serde_json::to_value(&e).unwrap();
        assert_eq!(v["category"], "Acquisition");
        assert_eq!(v["code"], "E_CHANNEL_CLOSED");
    }

    #[test]
    fn send_error_converts_to_acquisition_error() {
        // crossbeam SendError<T> wraps the unsent T; conversion drops it
        // and produces ChannelClosed.
        let (tx, rx) = crossbeam_channel::unbounded::<u32>();
        drop(rx);
        let err = tx.send(42).unwrap_err();
        let acq: AcquisitionError = err.into();
        assert!(matches!(acq, AcquisitionError::ChannelClosed { .. }));
    }

    #[test]
    fn config_error_has_stable_code() {
        let e = AppError::Config("rig.toml not found".into());
        let v: serde_json::Value = serde_json::to_value(&e).unwrap();
        assert_eq!(v["code"], "E_CONFIG");
    }

    /// The catalog is well-formed: every code is `E_…`, every category is
    /// PascalCase, and no `(code, category)` pair is emitted twice. Guards the
    /// strum SSoT against a malformed `serialize` attribute.
    #[test]
    fn error_catalog_is_well_formed() {
        let cat = error_catalog();
        assert!(!cat.is_empty());
        for (code, category) in &cat {
            assert!(
                code.starts_with("E_")
                    && code.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'),
                "malformed code {code:?}"
            );
            assert!(
                category.chars().next().is_some_and(|c| c.is_ascii_uppercase()),
                "malformed category {category:?}"
            );
        }
        let mut seen = std::collections::HashSet::new();
        for pair in &cat {
            assert!(seen.insert(*pair), "duplicate catalog pair {pair:?}");
        }
    }

    /// Drift guard: the committed frontend catalog must equal what the Rust SSoT
    /// renders. Regenerate after an error-enum change with
    /// `OISI_REGEN_ERROR_CODES=1`.
    #[test]
    fn error_codes_js_in_sync() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../ui/src/lib/error-codes.generated.js");
        let want = render_error_codes_js();
        if std::env::var("OISI_REGEN_ERROR_CODES").is_ok() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &want).unwrap();
            return;
        }
        let got = std::fs::read_to_string(&path).unwrap_or_default();
        assert_eq!(
            got, want,
            "ui/src/lib/error-codes.generated.js drifted from the Rust error SSoT; \
             regenerate with OISI_REGEN_ERROR_CODES=1"
        );
    }
}
