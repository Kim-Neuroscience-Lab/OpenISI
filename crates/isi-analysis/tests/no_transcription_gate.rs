//! Structural no-transcription / no-shim gate (objective 1 of the pinned
//! "independently-validated, reproducible correctness foundation" goal).
//!
//! This runs in the DEFAULT `cargo test` (pure filesystem checks, no interpreter)
//! and LOCKS the genuine-oracle cutover: it proves the era-wrong `_allen_oracle`
//! shim is gone and that the transcription generators retired during the cutover
//! never silently return. It is deliberately CONSERVATIVE — it asserts only what
//! is mechanically verifiable, so it can never itself become a "trust asserted,
//! not earned" check.
//!
//! What it does NOT yet do: a full per-generator allowlist classification
//! (library-primitive vs regression-lock vs formula-pin vs genuine-run). That
//! distinction can't be made by a content heuristic without mislabelling
//! regression-locks-that-use-numpy as library-primitives, so it requires a
//! hand-authored, source-verified manifest (the remaining objective-1 formalisation,
//! tracked in the oracle-harness memory). This gate fences the invariants that
//! ARE certain today.

use std::path::PathBuf;

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

/// The `np.int`/`xrange` compat SHIM is forbidden (objective 3) and was deleted
/// once the period-correct uv-locked NAT env let the reference run natively. It
/// must never come back, and no generator may import it.
#[test]
fn allen_oracle_shim_is_gone() {
    let dir = golden_dir();
    assert!(
        !dir.join("_allen_oracle.py").exists(),
        "the forbidden _allen_oracle.py compat shim has reappeared"
    );
    // Also reject any compiled-bytecode resurrection.
    assert!(
        !dir.join("__pycache__").join("_allen_oracle.cpython-310.pyc").exists()
            && !dir.join("__pycache__").join("_allen_oracle.cpython-313.pyc").exists(),
        "_allen_oracle bytecode has reappeared under __pycache__"
    );
    // No surviving generator may import the shim.
    for entry in std::fs::read_dir(&dir).expect("read golden dir") {
        let path = entry.expect("dir entry").path();
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or_default();
        if name.starts_with("gen_") && name.ends_with(".py") {
            let src = std::fs::read_to_string(&path).expect("read generator");
            assert!(
                !src.contains("_allen_oracle"),
                "{name} still imports the forbidden _allen_oracle shim"
            );
        }
    }
}

/// Transcription generators retired during the genuine-oracle cutover — each
/// re-implemented a RUNNABLE reference (Allen/SNLC) and has been superseded by a
/// live test driving the genuine reference. They must stay deleted: their return
/// would re-introduce a self-authored oracle (objective 1).
#[test]
fn retired_transcription_generators_stay_deleted() {
    // Generators whose expected values came from code WE wrote (inline formula or
    // a scipy/skimage re-implementation of a reference orchestration), now replaced
    // by a live genuine-reference test. NOT in this list: library-primitive
    // generators (call scipy/skimage/Octave-IPT) and genuine-run `.m` callers, which
    // are objective-1 compliant even while frozen.
    const RETIRED_TRANSCRIPTIONS: &[&str] = &[
        "gen_combine_golden.m",         // inlined Gprocesskret lines 88-99
        "gen_amplitude_golden.py",      // inlined Gprocesskret magS
        "gen_sigmaarea_golden.py",      // inlined Patch.getSigmaArea
        "gen_magnification_golden.py",  // inlined _getDeterminantMap + 1/det
        "gen_patchvs_golden.py",        // inlined Patch.getVisualSpace
        "gen_dft_golden.py",            // np.fft re-derivation (now live numpy.fft)
        "gen_patch_extraction_golden.py", // scipy composition mimicking _getRawPatchMap
        "gen_splitpatch_golden.py",     // scipy/skimage transcription of Patch.split2
        "gen_eccfull_golden.py",        // inlined eccentricityMap + getPixelVisualCenter
        // _allen_oracle-shim genuine-run-via-shim generators (now live, shim-free):
        "gen_vfs_golden.py",
        "gen_dilation_patches2_golden.py",
        "gen_ecc_golden.py",
        "gen_is_adjacent_golden.py",
        "gen_local_min_golden.py",
        "gen_merge_two_golden.py",
        "gen_patchsign_majority_golden.py", // inlined majority-sign transcription
        "gen_patchsign_golden.m",       // dead Octave generator (no consumer)
    ];
    let dir = golden_dir();
    let still_present: Vec<&str> = RETIRED_TRANSCRIPTIONS
        .iter()
        .copied()
        .filter(|g| dir.join(g).exists())
        .collect();
    assert!(
        still_present.is_empty(),
        "retired transcription generators have reappeared: {still_present:?}"
    );
}
