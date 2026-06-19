//! OpenISI dev-task runner — the oracle golden-fixture harness.
//!
//! The project validates its Rust pipeline against the field's reference
//! implementations (the "oracles"): SNLC/Garrett MATLAB (`reference/ISI`, run
//! under Octave) and Allen/Zhuang Python (`reference/corticalmapping`,
//! transcribed in the `gen_*_golden.py` scripts). Each oracle is run **offline**
//! to emit committed golden fixtures (`crates/isi-analysis/tests/golden/
//! fixtures/*.bin`); the test suite then validates against those committed
//! blobs, so a normal `cargo test` — and the user/release build — needs no
//! MATLAB/Octave/Python at all.
//!
//! This binary is the **dev-only** bridge to those oracles, so regeneration is
//! one command instead of hand-running ~40 scripts and hunting for interpreter
//! installs:
//!
//!   cargo xtask goldens            # regenerate every golden fixture
//!   cargo xtask goldens combine    # regenerate only generators matching "combine"
//!   cargo xtask goldens --check    # regenerate to a sandbox + diff vs committed
//!                                  #   (the CI freshness gate; restores the tree)
//!   cargo xtask figures            # run the render_*.py comparison-figure tools
//!                                  #   (a render_X.py with a sibling
//!                                  #   examples/X.rs runs that dump first, so
//!                                  #   the figure reflects the current code —
//!                                  #   e.g. render_oracle_state ← oracle_state)
//!
//! Interpreter discovery is declared, not guessed: `OPENISI_OCTAVE` /
//! `OPENISI_PYTHON` env vars override; otherwise the PATH and a small set of
//! known install locations are tried, with an actionable error if absent. See
//! `tools/golden/README.md` for one-time toolchain setup (`requirements.txt`).
//!
//! Build separation: `xtask` is its own workspace member that nothing depends
//! on, so `cargo build -p isi-analysis` / the Tauri app never compile it and
//! never acquire a Python/Octave dependency.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("goldens") => cmd_goldens(&args[1..]),
        Some("figures") => cmd_figures(&args[1..]),
        Some("help") | Some("--help") | Some("-h") | None => {
            print_usage();
            Ok(())
        }
        Some(other) => Err(format!("unknown task {other:?}\n\n{USAGE}")),
    };
    if let Err(e) = result {
        eprintln!("xtask: error: {e}");
        std::process::exit(1);
    }
}

const USAGE: &str = "\
usage: cargo xtask <task> [args]

tasks:
  goldens [filter] [--check]   regenerate golden fixtures by running each oracle
                               generator; with --check, regenerate into a sandbox
                               and diff against the committed fixtures (drift gate,
                               restores the working tree). `filter` runs only the
                               generators whose name contains the substring.
  figures [filter]             run the render_*.py comparison-figure scripts;
                               a render_X.py with a sibling examples/X.rs runs
                               that dump example first (e.g. oracle_state).
  help                         show this message.

toolchain (declared, not guessed):
  OPENISI_OCTAVE   path to octave-cli (else PATH / known install dirs)
  OPENISI_PYTHON   path to python with tools/golden/requirements.txt installed
                   (else PATH: python3, python)
";

fn print_usage() {
    println!("{USAGE}");
}

/// Absolute path to the repo root (this crate lives at `<root>/xtask`).
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask is a child of the repo root")
        .to_path_buf()
}

fn golden_dir() -> PathBuf {
    repo_root().join("crates/isi-analysis/tests/golden")
}

fn fixtures_dir() -> PathBuf {
    golden_dir().join("fixtures")
}

// ── interpreter discovery ───────────────────────────────────────────────────

/// Resolve an interpreter: an explicit env override wins, else the first
/// candidate that exists / runs. Returns a launchable program string.
fn resolve_interpreter(
    env_var: &str,
    on_path: &[&str],
    known_paths: &[&str],
) -> Result<String, String> {
    if let Ok(p) = std::env::var(env_var) {
        if !p.is_empty() {
            return Ok(p);
        }
    }
    // A bare name resolves via PATH at spawn time; probe with `--version`.
    for name in on_path {
        if Command::new(name)
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return Ok((*name).to_string());
        }
    }
    for p in known_paths {
        if Path::new(p).exists() {
            return Ok((*p).to_string());
        }
    }
    Err(format!(
        "could not locate an interpreter for {env_var}. Set {env_var} to its \
         path, put one of {on_path:?} on PATH, or install per \
         tools/golden/README.md."
    ))
}

fn octave() -> Result<String, String> {
    resolve_interpreter(
        "OPENISI_OCTAVE",
        &["octave-cli", "octave"],
        &[
            // Windows default install (versioned dir varies; common cases).
            "C:/Users/ISI User/AppData/Local/Programs/GNU Octave/Octave-11.2.0/mingw64/bin/octave-cli.exe",
        ],
    )
}

fn python() -> Result<String, String> {
    resolve_interpreter("OPENISI_PYTHON", &["python3", "python"], &[])
}

// ── generator enumeration ───────────────────────────────────────────────────

struct Generator {
    path: PathBuf,
    /// "py" or "m".
    ext: String,
    /// File stem for display/filtering (e.g. "gen_combine_golden").
    stem: String,
}

/// Every `gen_*_golden.{py,m}` under the golden dir, sorted for determinism.
fn enumerate_generators(filter: Option<&str>) -> Result<Vec<Generator>, String> {
    let dir = golden_dir();
    let mut gens = Vec::new();
    let entries =
        fs::read_dir(&dir).map_err(|e| format!("reading {}: {e}", dir.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| format!("dir entry: {e}"))?.path();
        let Some(name) = path.file_name().and_then(OsStr::to_str) else {
            continue;
        };
        if !name.starts_with("gen_") {
            continue;
        }
        let ext = match path.extension().and_then(OsStr::to_str) {
            Some(e @ ("py" | "m")) => e.to_string(),
            _ => continue,
        };
        let stem = path
            .file_stem()
            .and_then(OsStr::to_str)
            .unwrap_or(name)
            .to_string();
        if let Some(f) = filter {
            if !stem.contains(f) {
                continue;
            }
        }
        gens.push(Generator { path, ext, stem });
    }
    gens.sort_by(|a, b| a.stem.cmp(&b.stem));
    if gens.is_empty() {
        return Err(match filter {
            Some(f) => format!("no generators match filter {f:?} in {}", dir.display()),
            None => format!("no gen_*.{{py,m}} generators in {}", dir.display()),
        });
    }
    Ok(gens)
}

/// Force a Python child to use UTF-8 for stdout/stderr regardless of the host
/// console codepage. Without this, a generator that merely `print`s a non-ASCII
/// character (e.g. `→`) crashes with `UnicodeEncodeError` on a cp1252 Windows
/// console — breaking regeneration on Windows even when the fixture math is
/// fine. `PYTHONUTF8=1` enables Python's UTF-8 mode (3.7+); `PYTHONIOENCODING`
/// is the belt-and-suspenders fallback.
fn force_utf8_stdio(cmd: &mut Command) {
    cmd.env("PYTHONUTF8", "1");
    cmd.env("PYTHONIOENCODING", "utf-8");
}

/// Run one generator with its language's interpreter, from the golden dir.
fn run_generator(gen: &Generator, py: &str, oct: &str) -> Result<(), String> {
    let mut cmd = match gen.ext.as_str() {
        "py" => {
            let mut c = Command::new(py);
            c.arg(&gen.path);
            force_utf8_stdio(&mut c);
            c
        }
        "m" => {
            let mut c = Command::new(oct);
            c.arg("--norc").arg(&gen.path);
            c
        }
        other => return Err(format!("unsupported generator extension {other:?}")),
    };
    cmd.current_dir(golden_dir());
    let out = cmd
        .output()
        .map_err(|e| format!("spawning {}: {e}", gen.stem))?;
    if !out.status.success() {
        return Err(format!(
            "{} exited with {}\n--- stdout ---\n{}\n--- stderr ---\n{}",
            gen.stem,
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        ));
    }
    Ok(())
}

// ── fixtures snapshot (for --check) ─────────────────────────────────────────

/// Read every file under `fixtures/` as bytes, keyed by file name.
fn snapshot_fixtures() -> Result<BTreeMap<String, Vec<u8>>, String> {
    let dir = fixtures_dir();
    let mut snap = BTreeMap::new();
    if !dir.exists() {
        return Ok(snap);
    }
    for entry in fs::read_dir(&dir).map_err(|e| format!("reading fixtures: {e}"))? {
        let path = entry.map_err(|e| format!("fixtures entry: {e}"))?.path();
        if path.is_file() {
            let name = path
                .file_name()
                .and_then(OsStr::to_str)
                .unwrap_or_default()
                .to_string();
            let bytes = fs::read(&path).map_err(|e| format!("reading {}: {e}", path.display()))?;
            snap.insert(name, bytes);
        }
    }
    Ok(snap)
}

/// Restore the fixtures dir to exactly `snap` (rewrite changed, remove added).
fn restore_fixtures(snap: &BTreeMap<String, Vec<u8>>) -> Result<(), String> {
    let dir = fixtures_dir();
    let current = snapshot_fixtures()?;
    // Remove files that didn't exist in the snapshot.
    for name in current.keys() {
        if !snap.contains_key(name) {
            let _ = fs::remove_file(dir.join(name));
        }
    }
    // Rewrite any file whose bytes changed (or that was removed by a generator).
    for (name, bytes) in snap {
        if current.get(name).map(Vec::as_slice) != Some(bytes.as_slice()) {
            fs::write(dir.join(name), bytes)
                .map_err(|e| format!("restoring {name}: {e}"))?;
        }
    }
    Ok(())
}

// ── tasks ───────────────────────────────────────────────────────────────────

fn cmd_goldens(args: &[String]) -> Result<(), String> {
    let check = args.iter().any(|a| a == "--check");
    let filter = args.iter().find(|a| !a.starts_with("--")).map(String::as_str);

    let gens = enumerate_generators(filter)?;
    let py = python()?;
    let oct = octave()?;
    println!(
        "xtask goldens: {} generator(s){}\n  python = {py}\n  octave = {oct}",
        gens.len(),
        if check { " [--check: sandbox + diff]" } else { "" },
    );

    // For --check, snapshot first so we can diff and restore.
    let before = if check { Some(snapshot_fixtures()?) } else { None };

    let mut failures = Vec::new();
    for gen in &gens {
        match run_generator(gen, &py, &oct) {
            Ok(()) => println!("  ✓ {}", gen.stem),
            Err(e) => {
                println!("  ✗ {}", gen.stem);
                failures.push(e);
            }
        }
    }

    if let Some(before) = before {
        let after = snapshot_fixtures()?;
        let diffs = diff_snapshots(&before, &after);
        // Always restore the committed state — --check must not mutate the tree.
        restore_fixtures(&before)?;

        if !failures.is_empty() {
            return Err(format!(
                "{} generator(s) failed:\n\n{}",
                failures.len(),
                failures.join("\n\n")
            ));
        }
        if diffs.is_empty() {
            println!("\nfreshness OK: every regenerated fixture is byte-identical to the committed copy.");
            return Ok(());
        }
        let mut msg = format!("{} fixture(s) drifted from their generators:\n", diffs.len());
        for d in &diffs {
            msg.push_str(&format!("  {d}\n"));
        }
        msg.push_str(
            "\nthe committed fixtures no longer match what the generators + current \
             toolchain produce. Regenerate with `cargo xtask goldens` and review, \
             or pin the toolchain in tools/golden/requirements.txt to the version \
             that produced them.",
        );
        return Err(msg);
    }

    if !failures.is_empty() {
        return Err(format!(
            "{} generator(s) failed:\n\n{}",
            failures.len(),
            failures.join("\n\n")
        ));
    }
    println!("\nregenerated {} fixture set(s). Review `git diff` before committing.", gens.len());
    Ok(())
}

/// Names that changed / were added / removed between two snapshots.
fn diff_snapshots(
    before: &BTreeMap<String, Vec<u8>>,
    after: &BTreeMap<String, Vec<u8>>,
) -> Vec<String> {
    let mut out = Vec::new();
    for (name, b) in before {
        match after.get(name) {
            None => out.push(format!("removed: {name}")),
            Some(a) if a != b => out.push(format!("changed: {name}")),
            _ => {}
        }
    }
    for name in after.keys() {
        if !before.contains_key(name) {
            out.push(format!("added:   {name}"));
        }
    }
    out.sort();
    out
}

/// If `render_<stem>.py` has a sibling `crates/isi-analysis/examples/<stem>.rs`
/// dump binary, return `<stem>` — the example to run before rendering.
fn render_script_example(script: &Path) -> Option<String> {
    let stem = script.file_stem().and_then(OsStr::to_str)?;
    let example = stem.strip_prefix("render_")?;
    let rs = repo_root()
        .join("crates/isi-analysis/examples")
        .join(format!("{example}.rs"));
    rs.exists().then(|| example.to_string())
}

/// Run a dev-only dump example (`cargo run -p isi-analysis --example <name>`)
/// from the repo root, surfacing its output on failure.
fn run_dump_example(name: &str) -> Result<(), String> {
    println!("  → dumping data: cargo run --example {name}");
    let out = Command::new("cargo")
        .args(["run", "-q", "-p", "isi-analysis", "--example", name])
        .current_dir(repo_root())
        .output()
        .map_err(|e| format!("spawning example {name}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "example {name} exited with {}\n--- stdout ---\n{}\n--- stderr ---\n{}",
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        ));
    }
    Ok(())
}

fn cmd_figures(args: &[String]) -> Result<(), String> {
    let filter = args.iter().find(|a| !a.starts_with("--")).map(String::as_str);
    let py = python()?;
    let dir = golden_dir();
    let mut scripts = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|e| format!("reading {}: {e}", dir.display()))? {
        let path = entry.map_err(|e| format!("dir entry: {e}"))?.path();
        let Some(name) = path.file_name().and_then(OsStr::to_str) else {
            continue;
        };
        if name.starts_with("render_") && name.ends_with(".py") {
            if let Some(f) = filter {
                if !name.contains(f) {
                    continue;
                }
            }
            scripts.push(path);
        }
    }
    scripts.sort();
    if scripts.is_empty() {
        return Err("no render_*.py figure scripts found".into());
    }
    println!("xtask figures: {} script(s)\n  python = {py}", scripts.len());
    let mut failures = Vec::new();
    for s in &scripts {
        let name = s.file_name().and_then(OsStr::to_str).unwrap_or_default();
        // A `render_X.py` whose data is dumped by an `examples/X.rs` (the
        // codebase convention — see render_oracle_state.py / oracle_state.rs):
        // run the dump example first so the figure reflects the current code,
        // not a stale `target/` blob. No matching example → the script is
        // self-contained (reads committed fixtures directly).
        if let Some(example) = render_script_example(s) {
            if let Err(e) = run_dump_example(&example) {
                println!("  ✗ {name} (dump step)");
                failures.push(e);
                continue;
            }
        }
        let mut cmd = Command::new(&py);
        cmd.arg(s).current_dir(&dir);
        force_utf8_stdio(&mut cmd);
        let out = cmd
            .output()
            .map_err(|e| format!("spawning {name}: {e}"))?;
        if out.status.success() {
            println!("  ✓ {name}");
        } else {
            println!("  ✗ {name}");
            failures.push(format!(
                "{name}: {}\n{}",
                out.status,
                String::from_utf8_lossy(&out.stderr)
            ));
        }
    }
    if !failures.is_empty() {
        return Err(failures.join("\n\n"));
    }
    Ok(())
}
