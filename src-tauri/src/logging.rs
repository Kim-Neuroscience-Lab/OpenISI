//! Process-wide `tracing` subscriber initialization.
//!
//! One subscriber for the whole process, installed once at startup by both the
//! Tauri app (`setup`) and the headless CLI (`main`). Diagnostics across the
//! workspace go through `tracing` macros (`info!`/`warn!`/`error!`/`debug!`);
//! this routes them to stderr, filtered by level.
//!
//! **Diagnostics vs. user-facing output.** `tracing` is for *diagnostics*. The
//! headless CLI's user-facing output — usage text, command results — stays on
//! `println!`/`eprintln!`: it is the program's interface, not a log stream, and
//! must not be silenced or reformatted by the log filter.
//!
//! **Filter.** Controlled by `OPENISI_LOG` (preferred) or `RUST_LOG`, e.g.
//! `OPENISI_LOG=debug` or `OPENISI_LOG=isi_analysis=debug,openisi=info`. Default
//! is `info`, so normal runs are quiet but informative.

use std::sync::Once;

static INIT: Once = Once::new();

/// Install the global subscriber exactly once. Safe to call from multiple entry
/// points (Tauri setup, headless main, tests); subsequent calls are no-ops.
pub fn init() {
    INIT.call_once(|| {
        use tracing_subscriber::{EnvFilter, fmt};

        let filter = std::env::var("OPENISI_LOG")
            .ok()
            .and_then(|v| EnvFilter::try_new(v).ok())
            .or_else(|| EnvFilter::try_from_default_env().ok())
            .unwrap_or_else(|| EnvFilter::new("info"));

        fmt()
            .with_env_filter(filter)
            .with_target(false)
            .with_writer(std::io::stderr)
            .init();
    });
}
