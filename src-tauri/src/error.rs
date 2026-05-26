//! Unified application error type for Tauri commands.
//!
//! Every command returns `Result<T, AppError>`. Tauri serializes the error
//! via `serde::Serialize` — we emit a structured `AppErrorWire` with a
//! stable category + machine-readable code + human-readable message + a
//! `details` JSON for category-specific info. Frontend code can render
//! `message` directly and reveal `details` on user click.
//!
//! Two error enums are used in this codebase:
//! - `isi_analysis::AnalysisError` — propagated through the batch analysis
//!   pipeline via `?`. Variants: `Io`, `Hdf5`, `InvalidPackage`,
//!   `MissingData`, `Compute`, `Cancelled`.
//! - `AcquisitionError` — propagated within real-time acquisition worker
//!   threads (camera / stimulus). Includes `From<crossbeam_channel::
//!   SendError<T>>` so a closed receiver (main thread dead) cleanly
//!   exits the worker via `?`.
//!
//! `AppError` is the Tauri-IPC façade that wraps both and serializes to
//! the structured wire format.

use std::sync::{Mutex, MutexGuard};

// ----------------------------------------------------------------------------
// AcquisitionError — real-time worker thread errors
// ----------------------------------------------------------------------------

/// Errors that occur inside the camera/stimulus/sequencer worker threads
/// during acquisition. These can't `?`-propagate out of a `loop`, so the
/// pattern is: each thread's `run()` is a thin wrapper around a
/// `run_inner() -> Result<(), AcquisitionError>`. On `Err`, the wrapper
/// emits a `Fatal` event before exiting so the main thread can stop the
/// run cleanly and offer partial save.
#[derive(Debug, thiserror::Error)]
pub enum AcquisitionError {
    /// PCO SDK / camera hardware error message verbatim.
    #[error("Camera: {0}")]
    Camera(String),

    /// Stimulus thread / monitor render error message verbatim.
    #[error("Stimulus: {0}")]
    Stimulus(String),

    /// `crossbeam_channel::send` failed because the receiver is gone —
    /// indicates the consumer thread (typically the main thread) has
    /// died. The worker must exit so we don't accumulate buffered
    /// frames forever.
    #[error("Channel closed: {context}")]
    ChannelClosed { context: &'static str },

    /// Frame drop exceeded the configured catastrophic threshold during
    /// acquisition. Carries a human-readable description.
    #[error("Frame drop: {0}")]
    FrameDrop(String),

    /// Hardware disconnected mid-acquisition. Distinct from `Camera`
    /// because the recovery policy is different (stop the run, mark
    /// `acquisition_complete=false`).
    #[error("Disconnected mid-acquisition")]
    DisconnectedDuringAcquisition,
}

impl<T> From<crossbeam_channel::SendError<T>> for AcquisitionError {
    fn from(_: crossbeam_channel::SendError<T>) -> Self {
        Self::ChannelClosed { context: "acquisition channel" }
    }
}

// ----------------------------------------------------------------------------
// AppError — Tauri-IPC façade
// ----------------------------------------------------------------------------

/// Unified error type for all Tauri IPC commands. Wraps the
/// crate-internal error types via `#[from]` impls so handlers can use
/// `?` throughout.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("State lock poisoned: {context}")]
    LockPoisoned { context: String },

    #[error("{0}")]
    Analysis(#[from] isi_analysis::AnalysisError),

    #[error("{0}")]
    Acquisition(#[from] AcquisitionError),

    #[error("Hardware: {0}")]
    Hardware(String),

    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),

    #[error("Config: {0}")]
    Config(String),

    #[error("Validation: {0}")]
    Validation(String),

    #[error("Not available: {0}")]
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
            LockPoisoned { context } => AppError::LockPoisoned {
                context: context.to_string(),
            },
        }
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
    /// Map this error to its `(category, code)` pair. Both are stable
    /// strings the frontend / scripts can rely on.
    fn category_and_code(&self) -> (&'static str, &'static str) {
        match self {
            Self::LockPoisoned { .. } => ("Internal", "E_LOCK_POISONED"),
            Self::Analysis(e) => ("Analysis", analysis_error_code(e)),
            Self::Acquisition(e) => ("Acquisition", acquisition_error_code(e)),
            Self::Hardware(_) => ("Hardware", "E_HARDWARE"),
            Self::Io(_) => ("Io", "E_IO"),
            Self::Config(_) => ("Config", "E_CONFIG"),
            Self::Validation(_) => ("Validation", "E_VALIDATION"),
            Self::NotAvailable(_) => ("NotAvailable", "E_NOT_AVAILABLE"),
        }
    }

    /// Build the structured wire payload from this error.
    pub fn to_wire(&self) -> AppErrorWire {
        let (category, code) = self.category_and_code();
        AppErrorWire {
            category,
            code,
            message: self.to_string(),
            file_path: None,
            stage: None,
            details: serde_json::Value::Null,
        }
    }
}

fn analysis_error_code(e: &isi_analysis::AnalysisError) -> &'static str {
    use isi_analysis::AnalysisError::*;
    match e {
        Io(_) => "E_IO",
        Hdf5(_) => "E_HDF5",
        InvalidPackage(_) => "E_INVALID_PACKAGE",
        MissingData(_) => "E_MISSING_DATA",
        Compute(_) => "E_COMPUTE",
        Validation(_) => "E_VALIDATION",
        Cancelled => "E_CANCELLED",
    }
}

fn acquisition_error_code(e: &AcquisitionError) -> &'static str {
    use AcquisitionError::*;
    match e {
        Camera(_) => "E_CAMERA",
        Stimulus(_) => "E_STIMULUS",
        ChannelClosed { .. } => "E_CHANNEL_CLOSED",
        FrameDrop(_) => "E_FRAME_DROP",
        DisconnectedDuringAcquisition => "E_DISCONNECTED",
    }
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

/// Lock a `Mutex<T>` with a descriptive context for the error message.
///
/// Replaces the repeated `.map_err(|_| "Internal state error".to_string())?`
/// pattern throughout command handlers.
pub fn lock_state<'a, T>(mutex: &'a Mutex<T>, context: &str) -> Result<MutexGuard<'a, T>, AppError> {
    mutex.lock().map_err(|_| AppError::LockPoisoned {
        context: context.to_string(),
    })
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
}
