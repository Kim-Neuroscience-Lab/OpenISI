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

use agreement::{Eps, Tol};
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::io::Cursor;
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
        // Build the freshness report BEFORE restoring (it only reads), then always
        // restore — --check must not mutate the tree even if the report errors.
        let report = build_freshness_report(&before, &after);
        restore_fixtures(&before)?;

        if !failures.is_empty() {
            return Err(format!(
                "{} generator(s) failed:\n\n{}",
                failures.len(),
                failures.join("\n\n")
            ));
        }
        // A decode fault (a corrupt / unreadable `.npy`) is a *tooling* fault —
        // distinct from, and more severe than, numerical drift; surface it as the
        // error.
        let report = report?;

        // Always log the measured cross-toolchain drift — even on success — so the
        // CI run records the actual agreement margin (the grounding data for
        // `FLOAT_K`, the same way the magnification tolerance was set from measured
        // cross-CPU drift). Silent success would hide that record.
        if !report.measurements.is_empty() {
            println!("\nmeasured agreement vs committed (regenerated on this toolchain):");
            for m in &report.measurements {
                println!("  {m}");
            }
        }

        if report.problems.is_empty() {
            println!(
                "\nfreshness OK: every regenerated fixture agrees with its committed copy \
                 — discrete fixtures (masks/labels/raw-frames) bit-exact, float fixtures \
                 within the ε-grounded tolerance (worst over all floats: rel={:.3e}, abs={:.3e}).",
                report.worst_rel, report.worst_abs,
            );
            return Ok(());
        }
        let mut msg = format!(
            "{} fixture(s) disagree with their generators beyond tolerance:\n",
            report.problems.len()
        );
        for d in &report.problems {
            msg.push_str(&format!("  {d}\n"));
        }
        msg.push_str(
            "\nthe committed fixtures no longer match what the generators + current \
             toolchain produce, beyond the ε-grounded agreement tolerance. Discrete \
             (mask/label) drift is a real classification change — investigate it. Float \
             drift beyond tolerance means either a generator changed or the tolerance \
             is mis-grounded: regenerate with `cargo xtask goldens` and review, or — if \
             the drift is legitimate cross-toolchain rounding — raise `FLOAT_K` to the \
             smallest power of two covering the measured drift above.",
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

// ── self-describing .npy fixtures + ε-grounded freshness comparison ──────────
//
// The freshness gate regenerates every fixture on the CURRENT toolchain and
// compares it to the committed copy. That copy was produced on the dev host, but
// CI regenerates on a different toolchain (ubuntu apt-Octave / Python), so float
// results differ at the bit level by legitimate cross-toolchain rounding — a
// byte-identity check would be a device-identity check, not a validity check. So
// the comparison goes through the same `agreement::Tol` the goldens and the
// equivalence harness use.
//
// Fixtures are self-describing NumPy `.npy` arrays: the dtype AND shape live in
// the file header, read here via `npyz`. There is NO hand-maintained dtype
// manifest — the dtype is read from each fixture (so it cannot drift out of sync
// with the generator/reader), and a shape change is caught structurally. Discrete
// dtypes (integer masks, labels, raw camera frames) must be bit-exact across
// toolchains (`Tol::exact()`); float dtypes use a relative ε tolerance at the
// precision (`Eps::F32`/`F64`) read from the fixture, with a grounded K.

/// `K = 64` for float fixtures, grounded in the cross-toolchain drift this gate
/// measured on CI: the worst relative drift over every regenerated float fixture
/// was ≈ 32·ε of its precision; 64 is the smallest power of two covering it.
/// Applied at each fixture's own ε (`Eps::F32`/`F64`, read from its `.npy` dtype).
const FLOAT_K: u32 = 64;

/// A decoded `.npy` fixture: its shape + dtype (from the header) and values
/// widened to `f64` for comparison.
struct Npy {
    shape: Vec<u64>,
    /// numpy type char ('f' float, 'i' int, 'u' uint, 'b' bool) + byte size.
    kind: (char, u64),
    flat: Vec<f64>,
}

/// Decode a `.npy` blob: read dtype + shape from the header and widen to `f64`.
fn decode_npy(bytes: &[u8], name: &str) -> Result<Npy, String> {
    use npyz::TypeChar;
    let npy = npyz::NpyFile::new(Cursor::new(bytes))
        .map_err(|e| format!("{name}: not a valid .npy ({e})"))?;
    let shape = npy.shape().to_vec();
    let ts = match npy.dtype() {
        npyz::DType::Plain(ts) => ts,
        other => return Err(format!("{name}: unexpected .npy dtype {other:?}")),
    };
    let (tc, sz) = (ts.type_char(), ts.size_field());
    let kind_char = match tc {
        TypeChar::Float => 'f',
        TypeChar::Int => 'i',
        TypeChar::Uint => 'u',
        TypeChar::Bool => 'b',
        other => return Err(format!("{name}: unsupported .npy type char {other:?}")),
    };
    let map_err = |e: std::io::Error| format!("{name}: {e}");
    let flat: Vec<f64> = match (tc, sz) {
        (TypeChar::Float, 8) => npy.into_vec::<f64>().map_err(map_err)?,
        (TypeChar::Float, 4) => npy.into_vec::<f32>().map_err(map_err)?.into_iter().map(f64::from).collect(),
        (TypeChar::Int, 4) => npy.into_vec::<i32>().map_err(map_err)?.into_iter().map(f64::from).collect(),
        (TypeChar::Int, 1) => npy.into_vec::<i8>().map_err(map_err)?.into_iter().map(f64::from).collect(),
        (TypeChar::Uint, 2) => npy.into_vec::<u16>().map_err(map_err)?.into_iter().map(f64::from).collect(),
        (TypeChar::Uint, 1) | (TypeChar::Bool, 1) => npy.into_vec::<u8>().map_err(map_err)?.into_iter().map(f64::from).collect(),
        _ => return Err(format!("{name}: unsupported .npy dtype {kind_char}{sz}")),
    };
    Ok(Npy { shape, kind: (kind_char, sz), flat })
}

impl Npy {
    fn is_discrete(&self) -> bool {
        matches!(self.kind.0, 'i' | 'u' | 'b')
    }
    fn tol(&self) -> Tol {
        if self.is_discrete() {
            // Masks/labels/raw frames: any cross-toolchain change is a real
            // discrete difference, never rounding.
            Tol::exact()
        } else {
            // Float: relative ε bound at this fixture's stored precision.
            let base = if self.kind.1 == 4 { Eps::F32 } else { Eps::F64 };
            Tol::rel(FLOAT_K, base, FLOAT_K)
        }
    }
    fn dtype_str(&self) -> String {
        format!("{}{}", self.kind.0, self.kind.1)
    }
}

/// Outcome of the ε-grounded freshness comparison.
struct FreshnessReport {
    /// Human-readable agreement failures (drift beyond tolerance, structural
    /// add/remove/shape/dtype change). Empty ⇔ fresh.
    problems: Vec<String>,
    /// Per-float-fixture measured drift, logged for the grounding record.
    measurements: Vec<String>,
    worst_rel: f64,
    worst_abs: f64,
}

/// Compare regenerated fixtures to the committed copies through `agreement::Tol`.
/// Returns `Err` only on a tooling fault (a corrupt/unreadable `.npy`); numerical
/// or structural disagreements are reported as `problems`.
fn build_freshness_report(
    before: &BTreeMap<String, Vec<u8>>,
    after: &BTreeMap<String, Vec<u8>>,
) -> Result<FreshnessReport, String> {
    let mut problems = Vec::new();
    let mut measurements = Vec::new();
    let mut worst_rel = 0.0_f64;
    let mut worst_abs = 0.0_f64;

    for (name, b) in before {
        let Some(a) = after.get(name) else {
            problems.push(format!("removed: {name} (a generator no longer produces it)"));
            continue;
        };
        // Fast path: identical bytes always agree (same-toolchain regen, or a
        // genuinely deterministic fixture) — skip the decode + compare.
        if a == b {
            continue;
        }
        let committed = decode_npy(b, name)?;
        let regenerated = decode_npy(a, name)?;
        // A shape change (transpose/resize) or dtype change is structural — caught
        // from the .npy headers, even when the element count is preserved.
        if committed.shape != regenerated.shape {
            problems.push(format!(
                "changed: {name} (shape {:?} → {:?}; structural, not rounding)",
                committed.shape, regenerated.shape
            ));
            continue;
        }
        if committed.kind != regenerated.kind {
            problems.push(format!(
                "changed: {name} (dtype {} → {}; structural)",
                committed.dtype_str(),
                regenerated.dtype_str()
            ));
            continue;
        }
        let drift = committed.tol().check(&regenerated.flat, &committed.flat);
        if !committed.is_discrete() {
            worst_rel = worst_rel.max(drift.max_rel);
            worst_abs = worst_abs.max(drift.max_abs);
            measurements.push(format!(
                "{name} ({}): max_rel={:.3e}, max_abs={:.3e} over {} finite px{}",
                committed.dtype_str(),
                drift.max_rel,
                drift.max_abs,
                drift.n_finite,
                if drift.n_nan_mismatch > 0 {
                    format!(", {} NaN-position mismatch(es)", drift.n_nan_mismatch)
                } else {
                    String::new()
                },
            ));
        }
        if !drift.is_agreement() {
            let kind = if committed.is_discrete() { "discrete change" } else { "float drift" };
            problems.push(format!(
                "drift: {name} ({}, {kind}) — {} px exceed tolerance, {} NaN-position \
                 mismatch(es) (max_rel={:.3e}, max_abs={:.3e})",
                committed.dtype_str(), drift.n_fail, drift.n_nan_mismatch, drift.max_rel, drift.max_abs,
            ));
        }
    }
    for name in after.keys() {
        if !before.contains_key(name) {
            problems.push(format!(
                "added: {name} (a generator now produces a fixture not in git — commit it)"
            ));
        }
    }
    problems.sort();
    measurements.sort();
    Ok(FreshnessReport {
        problems,
        measurements,
        worst_rel,
        worst_abs,
    })
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
