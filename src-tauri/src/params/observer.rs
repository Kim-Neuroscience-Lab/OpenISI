//! Tauri-Emitter implementation of `ParamChangeObserver`.
//!
//! `openisi-params` defines the trait but doesn't depend on Tauri. The
//! Tauri shell wraps `tauri::AppHandle` here and forwards the event
//! payload as a `params:changed` IPC event to the JS UI.

use openisi_params::ParamChangeObserver;
use tauri::Emitter;

pub struct TauriParamObserver {
    handle: tauri::AppHandle,
}

impl TauriParamObserver {
    pub fn new(handle: tauri::AppHandle) -> Self {
        Self { handle }
    }
}

impl ParamChangeObserver for TauriParamObserver {
    fn notify(&self, payload: serde_json::Value) {
        if let Err(e) = self.handle.emit("params:changed", payload) {
            tracing::error!(error = %e, "failed to emit params:changed event");
        }
    }
}
