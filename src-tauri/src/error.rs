//! Unified application error type for Tauri commands.
//!
//! Every command returns `Result<T, AppError>`. Tauri serializes the error
//! via `serde::Serialize`, which we implement as a plain string so the
//! frontend receives a human-readable message.

use std::sync::{Mutex, MutexGuard};

/// Unified error type for all Tauri IPC commands.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("State lock poisoned: {context}")]
    LockPoisoned { context: String },

    #[error("{0}")]
    Analysis(#[from] isi_analysis::AnalysisError),

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

/// Tauri 2 requires command error types to implement `Serialize`.
/// We serialize as a plain string so the frontend gets a readable message.
impl serde::Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
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
