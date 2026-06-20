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

/// No generator may use a `roifilt2` shim (objective 3 — no shims). Octave's
/// `image` package lacks `roifilt2`; the only honest options are to run the
/// reference shim-free (impossible for these) or drop the golden — never to
/// author a `roifilt2` stand-in, which would put self-written logic in the oracle
/// path. The three shim-contaminated SNLC composite goldens were removed; this
/// keeps any from returning.
#[test]
fn no_generator_uses_a_roifilt2_shim() {
    let dir = golden_dir();
    for entry in std::fs::read_dir(&dir).expect("read golden dir") {
        let path = entry.expect("dir entry").path();
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or_default();
        if name.starts_with("gen_") && (name.ends_with(".py") || name.ends_with(".m")) {
            let src = std::fs::read_to_string(&path).expect("read generator");
            assert!(
                !src.to_lowercase().contains("roifilt2"),
                "{name} references roifilt2 — a forbidden shim (Octave lacks it); \
                 run shim-free or drop the golden, never author a stand-in"
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
        "gen_visualgrid_golden.py",     // transcribed getVisualSpace (now live); was dead (orphan fixtures)
        // _allen_oracle-shim genuine-run-via-shim generators (now live, shim-free):
        "gen_vfs_golden.py",
        "gen_dilation_patches2_golden.py",
        "gen_ecc_golden.py",
        "gen_is_adjacent_golden.py",
        "gen_local_min_golden.py",
        "gen_merge_two_golden.py",
        "gen_patchsign_majority_golden.py", // inlined majority-sign transcription
        "gen_patchsign_golden.m",       // dead Octave generator (no consumer)
        // roifilt2-SHIM-contaminated genuine-run goldens (Octave lacks roifilt2;
        // their fixtures were generated through a self-authored shim → objective 3):
        "gen_smoothpatches_golden.m",
        "gen_splitpatchesx_golden.m",
        "gen_fusepatchesx_golden.m",
        // verbatim transcriptions of splitPatchesX.m's FILE-LOCAL subfunctions
        // (not separately callable; parent needs roifilt2) → no separable reference:
        "gen_overrep_golden.m",
        "gen_centerpatch_golden.m",
        "gen_resetpatch_golden.m",
        "gen_getnlocalmin_golden.m",
        // library-primitive frozen goldens retired for objective 6 (the method is
        // now computed LIVE each run against the genuine reference):
        "gen_reflect_wrap_golden.py",   // -> separable_filter_matches_genuine_scipy_live
        "gen_fftgauss_golden.m",        // -> fft_gaussian_smooth_matches_genuine_octave_live
        "gen_cortex_morph_golden.m",    // -> cortex_morphology_matches_genuine_octave_live (top-border scene ported)
        "gen_patch_morph_golden.py",    // dead: read the mask, wrote unconsumed patch_morph_*.bin
        "gen_watershed_markers_golden.py", // wrong-era ws_out.bin (matched ours, NOT locked skimage 0.18.3); -> watershed_from_markers_stress_matches_genuine_skimage_live
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

/// The ALLOW side: every surviving generator is hand-classified (source-verified)
/// into a NON-transcription category. There is no `Transcription` category — so by
/// construction the suite contains zero generators that re-implement a *runnable*
/// reference (objective 1). The categories:
///   - `LibraryPrimitive`: the expected value IS the output of a stdlib/IPT primitive
///     (scipy/skimage/numpy/Octave-IPT) called directly; the library is the oracle.
///   - `GenuineRun`: calls the genuine vendored `.m` (addpath) and uses its output.
///   - `RegressionLock`: pins OpenISI's OWN behaviour (no external reference exists for
///     the specific rule); honestly labelled as such at the source.
///   - `FormulaPin`: pins a PUBLISHED formula whose reference code is not runnable in the
///     locked env (py2-only, GUI-bundled, or non-separable) — an irreducible gap stated
///     at the source; never dressed as a live oracle.
///
/// The gate asserts the on-disk `gen_*` set EXACTLY matches this manifest, so a new
/// generator can't slip in unclassified and a manifest entry can't go stale.
#[test]
fn every_surviving_generator_is_classified_non_transcription() {
    // (filename, category) — source-verified by reading each generator's oracle source.
    const MANIFEST: &[(&str, &str)] = &[
        ("gen_cortex_full_golden.m", "LibraryPrimitive"),      // Octave-IPT sequence (OpenISI orchestration)
        ("gen_adaptsmooth_golden.m", "GenuineRun"),            // genuine adaptiveSmoother.m (no roifilt2)
        ("gen_snr_golden.py", "RegressionLock"),               // OpenISI's own multi-bin SNR rule, no ref
        ("gen_cortexrel_golden.py", "RegressionLock"),         // OpenISI cortex-from-reliability mask, no ref
        ("gen_patch_threshold_golden.py", "RegressionLock"),   // |signMapf|>=thr / k*std rule
        ("gen_largestcc_tie_golden.py", "RegressionLock"),     // largest-CC tie = max first-index (lang guarantee)
        ("gen_v1ecc_golden.py", "RegressionLock"),             // OpenISI V1-center pin + the non-separable SNLC formula-pin
        ("gen_reliability_golden.py", "FormulaPin"),           // Engel 1994 / Zhuang 2017 published coherence
        ("gen_dff_golden.py", "FormulaPin"),                   // np.mean (live) + dF/F; normalizeMovie py2-absent
        ("gen_power_snr_golden.py", "FormulaPin"),             // generatePhaseMap power branch py2-only
        ("gen_maganiso_golden.py", "FormulaPin"),              // Garrett getMagFactors anisotropy (no separable ref)
        ("gen_spherical_marshel_golden.py", "FormulaPin"),     // Marshel 2012 arctan; MonitorSetup.remap py2-only
        ("gen_magroi_golden.m", "FormulaPin"),                 // overlaymaps.m ROI block is commented-out dead code
    ];
    let manifest_files: std::collections::BTreeSet<&str> = MANIFEST.iter().map(|&(f, _)| f).collect();
    assert_eq!(manifest_files.len(), MANIFEST.len(), "duplicate filename in MANIFEST");

    let on_disk: std::collections::BTreeSet<String> = std::fs::read_dir(golden_dir())
        .expect("read golden dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("gen_") && (n.ends_with(".py") || n.ends_with(".m")))
        .collect();

    let unclassified: Vec<&String> = on_disk.iter().filter(|n| !manifest_files.contains(n.as_str())).collect();
    let stale: Vec<&str> = manifest_files.iter().copied().filter(|n| !on_disk.contains(*n)).collect();
    assert!(
        unclassified.is_empty(),
        "generator(s) on disk not in the MANIFEST (classify them, source-verified): {unclassified:?}"
    );
    assert!(
        stale.is_empty(),
        "MANIFEST entries with no generator on disk (stale): {stale:?}"
    );
}
