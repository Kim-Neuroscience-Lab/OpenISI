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
        // A manifest/coverage/decode violation is a *tooling* fault (an unclassified
        // fixture, a stale manifest entry, a corrupt blob) — distinct from, and more
        // severe than, numerical drift; surface it as the error.
        let report = report?;

        // Always log the measured cross-toolchain drift — even on success — so the
        // CI run records the actual agreement margin (the grounding data for the K
        // in `FloatTol`, the same way the magnification tolerance was set from
        // measured cross-CPU drift). Silent success would hide that record.
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
             the drift is legitimate cross-toolchain rounding — raise the relevant K in \
             `FloatTol` to the smallest power of two covering the measured drift above.",
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

// ── fixture dtype manifest + ε-grounded freshness comparison ─────────────────
//
// The freshness gate regenerates every fixture on the CURRENT toolchain and
// compares it to the committed copy. That copy was produced on the dev host
// (Windows; Octave 11.2.0; a given Python), but CI regenerates on a different
// toolchain (ubuntu apt-Octave 8.4.0; Python 3.13). Float results therefore
// differ at the bit level by legitimate cross-toolchain rounding — a byte-
// identity check would be a device-identity check, which is *not* a validity
// check (the project's standing rule: agreement is tolerance-based, grounded in
// IEEE-754 ε, never bit/byte-identity — see crates/agreement). So the comparison
// goes through the same `agreement::Tol` the goldens and equivalence harness use.
//
// Each fixture is a flat little-endian array of one dtype; the dtype is declared
// here (the single source of truth, mirrored by — and verified against — how the
// tests read each blob back). Discrete fixtures (integer masks, labels, raw
// camera frames) must be bit-exact across toolchains: a flipped pixel is a real
// classification change, not rounding, so they use `Tol::exact()`. Float fixtures
// use a relative ε-tolerance whose K is grounded in the measured cross-toolchain
// drift the gate itself prints.

/// On-disk element type of a fixture's flat little-endian payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dtype {
    /// `<f8` (numpy) / `'double'` (Octave): 8-byte IEEE-754.
    F64,
    /// `<f4`: 4-byte IEEE-754 (the f32 compute-backend outputs).
    F32,
    /// `<u2`: raw 16-bit camera frames (integer counts).
    U16,
    /// 1-byte boolean/label masks.
    U8,
    /// `<i4`: connected-component labels.
    I32,
}

impl Dtype {
    /// Discrete (integer-valued) payloads must not drift at all across toolchains.
    fn is_discrete(self) -> bool {
        matches!(self, Dtype::U16 | Dtype::U8 | Dtype::I32)
    }

    /// Widen a flat little-endian blob to `f64` for comparison. Errors if the
    /// byte length is not a whole number of elements (a corrupt/truncated blob).
    fn decode(self, b: &[u8], name: &str) -> Result<Vec<f64>, String> {
        let width = match self {
            Dtype::F64 => 8,
            Dtype::F32 | Dtype::I32 => 4,
            Dtype::U16 => 2,
            Dtype::U8 => 1,
        };
        if !b.len().is_multiple_of(width) {
            return Err(format!(
                "{name}: {} bytes is not a whole number of {width}-byte {self:?} elements",
                b.len()
            ));
        }
        let out = match self {
            Dtype::F64 => b
                .chunks_exact(8)
                .map(|c| f64::from_le_bytes(c.try_into().unwrap()))
                .collect(),
            Dtype::F32 => b
                .chunks_exact(4)
                .map(|c| f64::from(f32::from_le_bytes(c.try_into().unwrap())))
                .collect(),
            Dtype::U16 => b
                .chunks_exact(2)
                .map(|c| f64::from(u16::from_le_bytes(c.try_into().unwrap())))
                .collect(),
            Dtype::U8 => b.iter().map(|&x| f64::from(x)).collect(),
            Dtype::I32 => b
                .chunks_exact(4)
                .map(|c| f64::from(i32::from_le_bytes(c.try_into().unwrap())))
                .collect(),
        };
        Ok(out)
    }

    /// The ε-grounded tolerance for this dtype's agreement check.
    fn tol(self) -> Tol {
        if self.is_discrete() {
            // Masks/labels/raw frames: any cross-toolchain change is a real
            // discrete difference, never rounding.
            Tol::exact()
        } else {
            // Float fixtures: relative ε-tolerance with a same-magnitude floor,
            // each precision grounded in IEEE-754 ε (K·ε). K is set from the
            // cross-toolchain drift this gate itself measured on CI (logged each
            // run) — exactly how the equivalence `magnification_axis` bound was set
            // from measured drift. Not magic absolutes: every bound is K·ε.
            match self {
                Dtype::F32 => Tol::rel(FloatTol::K, Eps::F32, FloatTol::K),
                _ => Tol::rel(FloatTol::K, Eps::F64, FloatTol::K),
            }
        }
    }
}

/// The ε-grounded relative-tolerance factor `K` for float fixtures (the bound is
/// `K·ε`, applied at each precision's own ε via [`Eps`]).
struct FloatTol;
impl FloatTol {
    /// `K = 64`, grounded in the cross-toolchain drift this gate measured on CI
    /// (dev host Octave 11.2.0 → ubuntu-24.04 Octave 8.4.0 / Python 3.13): the
    /// worst relative drift over every regenerated float fixture was `7.124e-15`
    /// on `v1ecc_alt.bin`, i.e. `32.08·ε_f64`. 64 is the smallest power of two
    /// that covers it. Every f32 fixture was bit-identical across the same pair
    /// (drift 0 ≤ 64·ε_f32), so the same factor bounds both precisions. Tighten
    /// only with new measured evidence; the gate logs the drift each run.
    const K: u32 = 64;
}

/// Declared on-disk dtype of every committed fixture — the single source of
/// truth for the freshness comparison, kept honest by `verify_manifest_coverage`
/// (every fixture on disk must be listed; no listed fixture may be missing). It
/// mirrors how each blob is read back by the tests (verified against the
/// generators' write dtypes), so a drift between the two surfaces here.
const FIXTURE_DTYPES: &[(&str, Dtype)] = &[
    // Octave cortex end-to-end: f64 VFS in, uint8 cortex mask out.
    ("cortex_full_vfs.bin", Dtype::F64),
    ("cortex_full_golden.bin", Dtype::U8),
    // cortex-from-reliability: f64 reliability inputs, uint8 masks
    // (`raw`/`expected` and the unused tie-case, all from gen_cortexrel_golden.py).
    ("cortexrel_azi_fwd.bin", Dtype::F64),
    ("cortexrel_azi_rev.bin", Dtype::F64),
    ("cortexrel_alt_fwd.bin", Dtype::F64),
    ("cortexrel_alt_rev.bin", Dtype::F64),
    ("cortexrel_raw.bin", Dtype::U8),
    ("cortexrel_expected.bin", Dtype::U8),
    ("cortexrel_tie_azi_fwd.bin", Dtype::F64),
    ("cortexrel_tie_azi_rev.bin", Dtype::F64),
    ("cortexrel_tie_alt_fwd.bin", Dtype::F64),
    ("cortexrel_tie_alt_rev.bin", Dtype::F64),
    ("cortexrel_tie_expected.bin", Dtype::U8),
    // ΔF/F: uint16 raw frames, f64 baselines, f32 ΔF/F.
    ("dff_frames.bin", Dtype::U16),
    ("dff_f0.bin", Dtype::F64),
    ("dff_f0_median.bin", Dtype::F64),
    ("dff_dff.bin", Dtype::F32),
    // largest-connected-component: uint8 in/out masks.
    ("largestcc_clear_input.bin", Dtype::U8),
    ("largestcc_clear_out.bin", Dtype::U8),
    ("largestcc_tie_input.bin", Dtype::U8),
    ("largestcc_tie_out.bin", Dtype::U8),
    // magnification / anisotropy: all f64.
    ("maganiso_axis.bin", Dtype::F64),
    ("maganiso_dhdx.bin", Dtype::F64),
    ("maganiso_dhdy.bin", Dtype::F64),
    ("maganiso_dvdx.bin", Dtype::F64),
    ("maganiso_dvdy.bin", Dtype::F64),
    ("maganiso_distortion.bin", Dtype::F64),
    // Octave magnitude-ROI: f64 magnitude + meta, uint8 ROI mask.
    ("magroi_in.bin", Dtype::F64),
    ("magroi_meta.bin", Dtype::F64),
    ("magroi_out.bin", Dtype::U8),
    // Allen power-SNR: f32 movie, uint8 mask.
    ("powersnr_movie.bin", Dtype::F32),
    ("powersnr_mask.bin", Dtype::U8),
    // patch threshold: f64 VFS, uint8 Allen/Garrett masks.
    ("pthr_vfs.bin", Dtype::F64),
    ("pthr_allen.bin", Dtype::U8),
    ("pthr_garrett.bin", Dtype::U8),
    // reliability: f32 complex parts, f64 expected coherence.
    ("rel_z_re.bin", Dtype::F32),
    ("rel_z_im.bin", Dtype::F32),
    ("rel_expected.bin", Dtype::F64),
    // spectral SNR: f32 ΔF/F, f64 timestamps + expected (small + large cases).
    ("snr_small_dff.bin", Dtype::F32),
    ("snr_small_ts.bin", Dtype::F64),
    ("snr_small_out.bin", Dtype::F64),
    ("snr_large_dff.bin", Dtype::F32),
    ("snr_large_ts.bin", Dtype::F64),
    ("snr_large_out.bin", Dtype::F64),
    // spherical (Marshel) stimulus geometry: all f64.
    ("sph_marshel_cm.bin", Dtype::F64),
    ("sph_marshel_deg.bin", Dtype::F64),
    ("sph_marshel_meta.bin", Dtype::F64),
    // V1 eccentricity: f64 maps/centers, i32 labels.
    ("v1ecc_alt.bin", Dtype::F64),
    ("v1ecc_azi.bin", Dtype::F64),
    ("v1ecc_labels.bin", Dtype::I32),
    ("v1ecc_rust_map.bin", Dtype::F64),
    ("v1ecc_rust_center.bin", Dtype::F64),
    ("v1ecc_snlc_map.bin", Dtype::F64),
    ("v1ecc_snlc_center.bin", Dtype::F64),
    // VFS phase-wrap stability (committed-only; no generator regenerates these,
    // so they never drift in the gate — classified for coverage completeness).
    ("vfs_wrap_phi1.bin", Dtype::F64),
    ("vfs_wrap_phi2.bin", Dtype::F64),
    ("vfs_wrap_allen_true.bin", Dtype::F64),
    ("vfs_wrap_allen_wrapped.bin", Dtype::F64),
];

fn dtype_of(name: &str) -> Option<Dtype> {
    FIXTURE_DTYPES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, d)| *d)
}

/// Self-check that the dtype manifest exactly covers the committed fixture set:
/// every fixture present in either snapshot must be classified, and no manifest
/// entry may be stale (absent from the committed `before` set). This is the
/// teeth that forces a new fixture to be classified — the same discipline as the
/// no-transcription gate's MANIFEST.
fn verify_manifest_coverage(
    before: &BTreeMap<String, Vec<u8>>,
    after: &BTreeMap<String, Vec<u8>>,
) -> Result<(), String> {
    let mut problems = Vec::new();
    for name in before.keys().chain(after.keys()) {
        if dtype_of(name).is_none() {
            problems.push(format!(
                "unclassified fixture {name:?}: add it to FIXTURE_DTYPES in xtask"
            ));
        }
    }
    for (name, _) in FIXTURE_DTYPES {
        if !before.contains_key(*name) {
            problems.push(format!(
                "stale manifest entry {name:?}: no such committed fixture (remove it from FIXTURE_DTYPES)"
            ));
        }
    }
    problems.sort();
    problems.dedup();
    if problems.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "fixture dtype manifest is out of sync with the fixtures dir:\n  {}",
            problems.join("\n  ")
        ))
    }
}

/// Outcome of the ε-grounded freshness comparison.
struct FreshnessReport {
    /// Human-readable agreement failures (drift beyond tolerance, structural
    /// add/remove, or element-count mismatch). Empty ⇔ fresh.
    problems: Vec<String>,
    /// Per-float-fixture measured drift, logged for the grounding record.
    measurements: Vec<String>,
    worst_rel: f64,
    worst_abs: f64,
}

/// Compare regenerated fixtures to the committed copies through `agreement::Tol`.
/// Returns `Err` only on a tooling fault (unclassified/stale manifest or a
/// corrupt blob); numerical/structural disagreements are reported as `problems`.
fn build_freshness_report(
    before: &BTreeMap<String, Vec<u8>>,
    after: &BTreeMap<String, Vec<u8>>,
) -> Result<FreshnessReport, String> {
    verify_manifest_coverage(before, after)?;

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
        // dtype is guaranteed present by verify_manifest_coverage above.
        let dt = dtype_of(name).expect("classified");
        let committed = dt.decode(b, name)?;
        let regenerated = dt.decode(a, name)?;
        if committed.len() != regenerated.len() {
            problems.push(format!(
                "changed: {name} (element count {} → {}; structural, not rounding)",
                committed.len(),
                regenerated.len()
            ));
            continue;
        }
        let drift = dt.tol().check(&regenerated, &committed);
        if !dt.is_discrete() {
            worst_rel = worst_rel.max(drift.max_rel);
            worst_abs = worst_abs.max(drift.max_abs);
            measurements.push(format!(
                "{name} ({dt:?}): max_rel={:.3e}, max_abs={:.3e} over {} finite px{}",
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
            let kind = if dt.is_discrete() { "discrete change" } else { "float drift" };
            problems.push(format!(
                "drift: {name} ({dt:?}, {kind}) — {} px exceed tolerance, {} NaN-position \
                 mismatch(es) (max_rel={:.3e}, max_abs={:.3e})",
                drift.n_fail, drift.n_nan_mismatch, drift.max_rel, drift.max_abs,
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
