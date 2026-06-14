//! Parameter-change observer hook.
//!
//! `openisi-params` is shell-agnostic; it does not import `tauri` and cannot
//! directly fire IPC events. The outer shell (the Tauri app, a CLI, a test
//! harness) registers an implementor on the [`ConfigStore`](crate::config::ConfigStore)
//! so the UI sees config changes without `openisi-params` knowing Tauri exists.

/// Hook for surfacing parameter-change events to whoever owns the outer shell.
/// Implementors receive a JSON payload describing what changed; the typed
/// schema-driven UI re-reads the full config on any change, so the payload need
/// only signal "config changed".
pub trait ParamChangeObserver: Send + Sync {
    fn notify(&self, payload: serde_json::Value);
}
