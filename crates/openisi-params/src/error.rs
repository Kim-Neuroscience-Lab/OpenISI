//! Error type for the params crate.
//!
//! The Tauri shell's `AppError` wraps this via a `From<ParamsError> for
//! AppError` impl, flattening each variant onto the matching AppError
//! variant so the IPC wire format is unchanged.

#[derive(Debug, thiserror::Error)]
pub enum ParamsError {
    /// Parameter value failed validation (range / type / constraint).
    #[error("Validation: {0}")]
    Validation(String),

    /// Malformed configuration shape — bad TOML structure, dotted-path
    /// collision, unrecognized variant string. Distinct from `Validation`
    /// because the user typed something the parser couldn't structurally
    /// understand, vs a value that parsed but doesn't satisfy a range.
    #[error("Config: {0}")]
    Config(String),

    /// A required precondition isn't met (e.g. no active .oisi, no
    /// hardware context). Distinct from `Validation` so the UI can
    /// render a different affordance ("connect hardware" vs "fix value").
    #[error("Not available: {0}")]
    NotAvailable(String),

    /// Filesystem I/O failure surfaced verbatim.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type ParamsResult<T> = std::result::Result<T, ParamsError>;
