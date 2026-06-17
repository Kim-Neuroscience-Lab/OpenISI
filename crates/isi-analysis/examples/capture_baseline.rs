//! Capture per-stage baseline outputs from the current pipeline.
//!
//! Usage:
//!
//! ```text
//! cargo run --example capture_baseline -- <input.oisi> <output.baseline.oisi>
//! ```
//!
//! Algorithm:
//!
//!   1. Copy `input.oisi` → `output.baseline.oisi`.
//!   2. If `output.baseline.oisi` has a pre-2026 `/analysis_params` schema,
//!      translate it to the current (tagged `AnalysisConfig`) schema and write it back.
//!   3. Build an `AnalysisParams` from the file's `/analysis_params` (if
//!      present after migration) or from the typed-config defaults.
//!   4. Run `isi_analysis::analyze` on the copy. `analyze` writes
//!      `/complex_maps/*` (cached after the first run) and `/results/*` in
//!      place. The output file is the baseline — it has the same on-disk
//!      schema as any analyzed `.oisi`, so cross-implementation equivalence
//!      tests just compare per-dataset against this file.
//!
//! No special baseline format. The baseline is a snapshot of the fixture
//! after the pipeline ran on it. This tool captures the per-stage baseline
//! the equivalence test compares against: that test re-runs `analyze` on a
//! fresh copy of the fixture and asserts each `/results/<dataset>` matches
//! the baseline's same dataset within tolerance.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use isi_analysis::{self, AnalysisError, SilentProgress};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: capture_baseline <input.oisi> <output.baseline.oisi>");
        std::process::exit(2);
    }
    let input = Path::new(&args[1]);
    let output = Path::new(&args[2]);

    if !input.exists() {
        eprintln!(
            "[capture-baseline] error: input file does not exist: {}",
            input.display()
        );
        std::process::exit(1);
    }

    if let Err(e) = run(input, output) {
        eprintln!("[capture-baseline] error: {e}");
        std::process::exit(1);
    }
}

fn run(input: &Path, output: &Path) -> Result<(), Box<dyn std::error::Error>> {
    // Step 1: copy input → output so the fixture stays pristine.
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(input, output)?;
    let copied_bytes = output.metadata()?.len();
    println!(
        "[capture-baseline] copied {} → {} ({:.1} MB)",
        input.display(),
        output.display(),
        copied_bytes as f64 / 1_000_000.0,
    );

    // Step 1b: strip any derived caches so the baseline is a true recompute
    // from the rawest input (matches the equivalence harness, which does the
    // same — see `io::strip_derived_outputs`).
    isi_analysis::io::strip_derived_outputs(output)?;

    // Step 2: if pre-2026 schema, migrate /analysis_params in place.
    if isi_analysis::io::is_pre_2026_analysis_params(output)? {
        println!("[capture-baseline] migrating pre-2026 /analysis_params");
        let old = isi_analysis::io::read_analysis_params_attr(output)?.ok_or_else(|| {
            AnalysisError::Validation(
                "is_pre_2026_analysis_params returned true but read_analysis_params_attr \
                 returned None — file structure inconsistent"
                    .to_string(),
            )
        })?;
        let new = isi_analysis::migrate::translate_pre_2026_analysis_params(&old)?;
        isi_analysis::io::write_analysis_params_attr(output, &new)?;
    }

    // Step 3: build AnalysisParams. Prefer the file's /analysis_params tree
    // (now in the current schema after step 2); fall back to the typed
    // `AnalysisConfig` defaults if absent.
    let params = match isi_analysis::io::read_analysis_params_attr(output)? {
        Some(tree) => isi_analysis::bridge::analysis_params_from_oisi_tree(&tree)?,
        None => {
            println!("[capture-baseline] no /analysis_params — using config defaults");
            isi_analysis::AnalysisParams::from(&openisi_params::config::AnalysisConfig::default())
        }
    };

    // Step 4: run the full pipeline. Writes /complex_maps/* (cached) and
    // /results/* atomically, stamping the params tree into /analysis_params in
    // the same transaction (so the baseline carries the params it was made with
    // and subsequent equivalence tests reuse them).
    println!("[capture-baseline] running isi_analysis::analyze");
    let progress = SilentProgress;
    let cancel = AtomicBool::new(false);
    let tree = serde_json::to_value(openisi_params::config::AnalysisConfig::from(&params))
        .map_err(|e| AnalysisError::Validation(format!("serialize analysis params: {e}")))?;
    isi_analysis::analyze(output, &params, Some(&tree), &progress, &cancel)?;

    let final_bytes = output.metadata()?.len();
    println!(
        "[capture-baseline] wrote baseline {} ({:.1} MB)",
        output.display(),
        final_bytes as f64 / 1_000_000.0,
    );
    Ok(())
}
