# Genuine-oracle validation harness

This is the **validation instrument**: OpenISI's correctness claims are proven by
comparison against the field's *genuine reference code, executed live* in a
per-oracle locked environment — never against our own re-reading of a method.
A green oracle gate is therefore independent proof of faithfulness, not a tautology.

See the pinned goal: *An independently-validated, reproducible correctness
foundation*.

## How it works

- **Per-oracle, locked, isolated environments.** Each reference codebase has its own
  directory here with a pinned, reproducible environment the harness provisions —
  never the machine's ambient toolchain.
  - `nat/` — **NeuroAnalysisTools 3.1.0** (Allen/Zhuang retinotopy), Python, via
    [`uv`](https://docs.astral.sh/uv/): `pyproject.toml` + committed `uv.lock`
    pin Python 3.9 + a period-correct stack (numpy 1.23.5, scipy 1.9.3,
    scikit-image 0.18.3, …). In that era `np.int` exists **and**
    `skimage.morphology.watershed` still exists (the reference's `Patch.split2`
    calls it; it was removed in skimage 0.19), so the vendored reference runs
    **natively — no shim**. See `nat/pyproject.toml` for the full pin derivation.
  - `snlc/` — SNLC/Garrett MATLAB (`reference/ISI/*.m`), executed via **Octave
    11.2.0** + the `image` package (`OPENISI_OCTAVE` → `octave-cli`; CI installs
    `octave octave-image`). Live arms: getPatchSign, getPatchCoM, and the IPT
    builtins watershed/bwdist/imimposemin.
- **The reference code is byte-pristine.** Nothing here modifies
  `reference/…`; the env is shaped to fit the reference, not the reverse.
- **Live, not frozen.** `bridge.py` (per oracle) is a pure caller — it marshals
  arrays across the process boundary and a dispatch table maps a function id to a
  **direct call of the genuine reference function** (module functions and class
  methods, the latter constructed in the bridge and driven as the real method).
  No per-golden scripts, no committed output fixtures: the oracle is computed on
  every run.
- **Rust side.** `test_support::oracle::{nat, nat_raw}` invokes the bridge through
  `uv run --project <oracle>`. The validation tests live in-crate (they need
  crate-internal compute), gated behind the `oracle_live` Cargo feature, so the
  default `cargo test` needs no interpreter.

## Running

```
# requires `uv` on PATH (or OPENISI_UV=<path to uv>)
cargo test -p isi-analysis --features oracle_live genuine_nat -- --nocapture
```
The locked env is provisioned automatically by `uv` from the committed `uv.lock`,
so a clean machine / CI reproduces the same result.

## Irreducible gaps (stated, never assumed away)

- **Period-correct reconstruction, not the authors' exact machine.** NAT's
  `requirements.txt` is *unversioned*, so the locked env is a reconstruction from
  the code's hard constraints (`np.int` ⇒ numpy<1.24) + the 3.1.0 era — defensible
  and reproducible, but not provably the authors' exact toolchain (which is
  unknowable). Pins live in `nat/pyproject.toml` / `nat/uv.lock`.
- **Octave ≈ MATLAB, not identical — now executable under BOTH.** The SNLC reference
  is MATLAB; the bridge (`snlc/bridge.m`) is dual-runtime — it runs under genuine
  **MATLAB** (set `OPENISI_MATLAB`) *and* Octave (the open fallback, set
  `OPENISI_OCTAVE`). Octave's IPT functions match MATLAB's to high precision but are
  not bit-identical.
  **OPEN FINDING (objective 5 — surfaced by running genuine MATLAB R2025b, which the
  Octave approximation had HIDDEN):** `imclose(strel('disk',R,0))` on a
  **border-touching** object differs **MATLAB vs Octave**; `imopen`, `imdilate(disk)`,
  `imfill('holes')` are bit-identical between the two. **Exact rule, verified against
  MATLAB R2025b:** MATLAB `imclose` = *pad the image with 0 by the SE radius → naive
  `dilate→erode` → crop* (`imclose == imerode(imdilate(pad0(bw),se),se)` cropped,
  diff 0; pad-with-1 is wrong). Octave's `imclose` (and our `binary_closing_disk`) do
  the naive `dilate→erode` WITHOUT the border pad — so they agree with each other but
  differ from genuine MATLAB **only within R px of the image edge**.
  **RESOLVED — fixed to match the genuine MATLAB reference.** `binary_closing_disk` now
  pads with 0 by the SE radius → dilate→erode → crop, bit-exact to MATLAB R2025b's
  `imclose`. The change is border-only, so it moves NO real result: the cortex_full
  orchestration golden, the R43 equivalence baseline, and the synthetic baseline are all
  unchanged (their cortex ROI is interior — verified, the suites stay green). The
  `cortex_morphology` live test now passes under genuine MATLAB **exactly**; under the
  Octave fallback it asserts the divergence is **confined to within R px of the edge**
  (the documented Octave≈MATLAB `imclose` gap), so a real interior bug still fails.
  OpenISI is now faithful to genuine MATLAB `imclose`, with Octave's border behaviour
  honestly bounded — exactly the kind of divergence the genuine-MATLAB oracle exists to
  catch (Octave alone had hidden it).
- **Octave version: tolerance-based, not bit-pinned across versions.** The env is
  version-pinned per host (dev: Octave **11.2.0**; CI: ubuntu-24.04 → Octave
  **8.4.0**, recorded each run via `::notice`), but the two versions are NOT
  bit-identical to each other. The genuine-oracle suite is GREEN on **both** — direct
  evidence that the oracle gates are tolerance-based (device/version-independent
  validity), not bit-exact. A future Octave that drifts the result past a per-method
  tolerance would fail the gate loudly (the gate working), forcing a deliberate
  re-pin — never a silent change. (A digest-locked `container:` Octave would pin the
  exact patch version; ubuntu-24.04 fixes major/minor, which is what the tolerances
  are grounded against.)
- **Octave's `image` package lacks `roifilt2`** (`exist('roifilt2')==0` in 11.2.0),
  and the reference functions that call it — `smoothPatchesX.m`, `splitPatchesX.m`,
  `fusePatchesX.m`, and `Gprocesskret.m`'s *smoothing* branches — therefore **cannot
  be executed natively** in the Octave env. **RESOLVED (objective 3):** the
  `smpatch_*`/`spx_*`/`fpx_*` goldens for `smoothPatchesX`/`splitPatchesX`/
  `fusePatchesX` were generated by the genuine `.m` running against a **`roifilt2`
  SHIM** (a self-authored stand-in) — shim-contaminated, NOT clean genuine-reference
  values. Rather than keep falsely-"validated" goldens or author a `roifilt2`
  replacement (the forbidden circularity), they were **DELETED** (tests + fixtures +
  generators), and a gate test (`no_generator_uses_a_roifilt2_shim`) keeps any from
  returning. Those composites have **no oracle test** — honestly, because their
  reference can't run shim-free here; their constituent primitives
  (`fspecial`/`filter2`/morphology/`watershed`/`bwdist`/`interp2`) ARE validated live. (Gprocesskret's **no-smoothing** branch —
  combine, delay, magS — needs no `roifilt2` and IS genuinely live; the constituent
  primitives `fspecial`/`filter2`/morphology/`watershed`/`bwdist`/`interp2` ARE live.)
- **The reference composites bundle plotting.** `splitPatchesX.m` / `fusePatchesX.m`
  (and `getMouseAreasX.m`) interleave `figure`/`imagesc`/`contour` with the compute
  and won't run headless; `getMouseAreasX` is additionally the full GUI pipeline.
  Their end-to-end orchestration is OpenISI's, validated as a regression-lock against
  the frozen genuine-run fixture, with the building-block primitives validated live
  (the genuine end-to-end cannot be re-run headless in Octave — stated, not hidden).

## Divergence ledger

Every difference between OpenISI and the genuine oracle is recorded here — fixed in
OpenISI, or stated as a justified deviation. No loosened tolerances, no cropping, no
skipped cases.

| Method | Oracle | Result | Note |
|---|---|---|---|
| `visualSignMap` (VFS) | NAT `RM.visualSignMap` | agree to 1.3e-5 (interior) | Discretization-order difference: our chain-rule phasor gradient vs Allen's `np.gradient`-of-wrapped-phase. Within the documented `5e-2` discretization tolerance; ours is the more wrap-stable variant (separate wrap test). **Not a defect.** |
| `dilationPatches2` | NAT `RM.dilationPatches2` | bit-identical (0 px) | — |
| `_getRawPatchMap` | NAT `RetinotopicMappingTrial._getRawPatchMap` | bit-identical (0 px) | Driven as the genuine class method (signMapThr=0.5 reproduces our binary input). |
| `eccentricityMap` | NAT `RM.eccentricityMap` | agree to f64 machine precision | — |
| `is_adjacent` | NAT `core.ImageAnalysis.is_adjacent` | bit-identical (all pairs × border-widths) | — |
| `_getDeterminantMap` | NAT `RetinotopicMappingTrial._getDeterminantMap` | agree to f32 precision (`\|det J\|`) | Driven as the genuine class method. f32 gradient + det-cancellation vs genuine f64. |
| `localMin` | NAT `RM.localMin` | bit-identical (integer marker maps) | — |
| `getSigmaArea` | NAT `Patch.getSigmaArea` | bit-identical incl. NaN cases | The audit *suspected* a NaN-handling divergence; the live oracle **disproved** it — ours propagates `0·NaN = NaN` (NaN outside the mask) exactly like genuine numpy. No divergence. |
| `getVisualSpace` | NAT `Patch.getVisualSpace` | bit-identical (0 px) | Driven as the genuine class method; `VisualGrid` built to NAT's hardcoded ranges (alt [-40,60), azi [-20,120)) since our `derive_visual_grid` is a regression-lock OpenISI choice, not the oracle. |
| `split_patch_from_ecc` | NAT `Patch.split2` (watershed branch) | patch count + order-free union identical | Driven as the genuine class method (variable-count patch dict). **Forced the env re-pin** that surfaced the skimage-0.19.3 anachronism (`sm.watershed` removed in 0.19) → re-pinned to skimage 0.18.3 / Python 3.9, skeletonize verified bit-identical so no existing result moved. |
| `position_phasor_delay_subtracted` + `delay_map` | **SNLC** `Gprocesskret.m` (Octave) | combine phasor agree (4·ε_f32), delay agree (512·ε_f32) | Driven as the genuine function. **The live run exposed a transcription artifact:** the frozen golden generator fed `Gprocesskret`'s formula the *post-negation* angles directly, hiding its internal `ang = angle(-ang_input)` step. To drive the genuine `.m` faithfully we feed `-exp(i·θ)` so its internal negation recovers θ — then the genuine kmap/delay match. Combine compared as a phasor (wrap-safe); delay single-valued in (0,π]. |
| `position_amplitude` | **SNLC** `Gprocesskret.m` `magS.hor` (Octave) | agree to f32 (rel 4·ε_f32) | `magS` is taken before the phase negation, so full complex fwd/rev fed directly. No-smoothing branch (no `roifilt2`). |
| `interp2_spline` | Octave `interp2(...,'spline')` | agree to f64 (rel 64·ε_f64) | Library-primitive not-a-knot tensor-product cubic spline. |
| `patch_com` | **SNLC** `getPatchCoM.m` (Octave) | centroid set identical (Tol abs 128·ε_f64) | Driven as the genuine `.m`; MATLAB `bwlabel` column-major vs our row-major → compared order-independently. Snap-correction path covered by the frozen fixture. |
| `watershed_octave{4,8}` | Octave IPT `watershed(A,conn)` | bit-identical i32 labels | Library-primitive — Octave's own watershed; our wrapper mirrors it. |
| `bwdist` | Octave IPT `bwdist` | identical to f32 (Octave returns single) | Library-primitive Euclidean DT. |
| `imimposemin` | Octave IPT `imimposemin` | agree to f64 (64·ε_f64) | Library-primitive morphological reconstruction. |
| `binary_{opening,closing,dilation}_disk`, `binary_fill_holes` | Octave IPT `imopen`/`imclose`/`imdilate`(`strel('disk',R,0)`)/`imfill('holes')` | bit-identical | Library-primitive cortex morphology; `strel('disk',R,0)` = exact Euclidean disk. |
| `getPatchSign` (signs) | **SNLC** `getPatchSign` (Octave) | region-wise identical (non-zero-mean) | **Documented deviation, zero-mean only:** MATLAB `sign(mean)=0` gives an *undefined* patch sign at exactly zero mean; ours takes a deterministic `+1` tie-break. Justified (a patch must get a sign). Separately: our `label_4conn` is row-major, MATLAB `bwlabel` column-major — different label *order*, identical signs; compared label-invariantly (per-pixel), so not a divergence. |
| `watershed_from_markers` | `skimage.segmentation.watershed` (`connectivity=ones(3,3)`, `watershed_line=False`) | **smooth fields: bit-identical labels;** tie-heavy fields: completeness + label-set identical, per-pixel assignment diverges | **Documented irreducible difference (two valid watersheds).** On smooth elevation fields (the realistic `split2` input) ours = skimage exactly (`..._matches_genuine_skimage_live`). On an adversarial flat/tied scene ours diverges ~21 px (basin interiors included) — different flooding order (skimage = priority-queue age; ours = ascending-elevation + first-labelled neighbour). The `..._stress_..._live` test pins only the sound invariants (no spurious watershed-line 0s; unchanged label set). `Patch.split2`'s count+union check is insensitive to internal boundary placement, so `split_patch_from_ecc` still agrees. **Finding:** the retired frozen `ws_out.bin` golden matched ours but NOT the locked skimage 0.18.3 — a wrong-era fixture (generator referenced skimage 0.25); deleted, replaced by the live test. |
| `SnlcAdaptiveSmoother` | **SNLC** `adaptiveSmoother.m` (Octave, `h = fspecial('gaussian',15,σ)`) | agree to f64 (rel 64·ε_f64) on re + im | Driven as the genuine `.m` live (no `roifilt2`, runs shim-free); drift = 225-tap `filter2` sum order + the local-variance division across runtimes. Migrated from a frozen genuine-run fixture to live (objective 6). |

*(Updated as each method migrates.)*

## Honest labels (regression-locks vs oracles)

Methods with **no external reference code** are regression-locks (they pin
OpenISI's own current behaviour), **not** oracles — and are labelled as such at the
source, never dressed as validated:

| Method | Label site | Why no oracle |
|---|---|---|
| `spectral_snr` | `compute/responsiveness.rs:10,48` — *"OpenISI heuristic, NO external oracle… regression-lock"* | OpenISI's own SNR ratio rule; nothing upstream computes it. |
| `cortex_from_reliability` | `segmentation/mod.rs:101`, `golden_cortex_morph.rs:611` — *"UNVALIDATED (regression-lock only)"* | Zhuang segments full-frame; the reliability-mask cortex restriction is OpenISI's. |
| `derive_visual_grid` | `methods/patch_refinement.rs:1466` — *"regression-lock, NOT an oracle"* | OpenISI rendering choice. |
| `compute_eccentricity` V1-center | `golden_cortex_morph.rs:643` — *"regression-lock"* | OpenISI V1-center selection differs from SNLC by design. |

**SNLC tests whose named "oracle" is NOT genuine executable reference code** (the
genuine-execution survey surfaced these — they are formula-pins / regression-locks,
relabelled honestly, not claimed as live code oracles):

| Test | Honest classification | Why not a live code oracle |
|---|---|---|
| `snlc_mag_threshold_roi` (`…matches_overlaymaps`) | **Formula-pin** (`mag^1.1` → rescale [0,1] → `≥0.12`) | `overlaymaps.m`'s ROI block (lines 205-215) is **entirely commented-out dead code** inside a 56-plot GUI script that loads data by animal name — not executable. The formula is elementary arithmetic, grounded in the (dead) reference, not a runnable oracle. |
| `keep_largest_component` (`…matches_snlc_argmax`) | **Regression-lock**; tie-break grounded in `max` first-index | `getMouseAreasX.m` is the full `figure`/`imagesc`/`contour` GUI pipeline (won't run headless); the largest-CC tie-break is MATLAB `max` returning the first index — a language guarantee, not a callable oracle. |
| `polar_angle` (`…matches_snlc_atan2…`) | **Formula-pin** (`atan2(Δalt,Δazi)·180/π` about V1 centre) | `atan2` is a primitive; the label-scoped V1-centre logic is OpenISI's. No reference `.m`. |
| `snlc_cortex_endtoend` | **Regression-lock** at the orchestration level | The full threshold→open→close→fill→dilate→largest-CC sequence is OpenISI's composition; **each primitive is validated live** (Octave IPT), but no single reference `.m` defines the end-to-end chain. |
| `magnification_anisotropy_matches_snlc_getmagfactors` | **Formula-pin** on fixed gradients (`getMagFactors.m` post-`gradient` block, reproduced verbatim) | `getMagFactors.m` **does run shim-free in Octave** — but it bundles `kmaps → fft-gaussian-smooth (σ=3, full-image) → gradient → anisotropy/jacobian` as one inseparable call that returns only the final maps, never its internal gradients. The op under test (`magnification_anisotropy`) takes *gradients*; there is no separable reference for "anisotropy from given gradients". A live **end-to-end** comparison is also not faithful to OpenISI's op: the bundled smoothing **dominates** the result (measured on varied synthetic kmaps: feeding the raw-kmap gradients instead of the smoothed ones moves interior distortion by up to **0.83** on a [0,0.94] range and the axis by **>110°**), and OpenISI deliberately replaces getMagFactors' kmap-smoothing with amplitude-weighted **phasor** smoothing (wrap-stable) — so the genuine end-to-end and OpenISI's pipeline magnification are *different ops by design*. **Irreducible for the isolated op** (objective 8); the post-gradient block is reproduced verbatim and pinned on fixed gradients, while the smoothing/gradient stages OpenISI *does* use are validated live separately (`gaussian_smooth` vs `scipy.gaussian_filter`; gradients via the determinant-map live test). A genuine live oracle is only reachable by adding a *non-pipeline* getMagFactors-faithful variant (fft-smooth + plain gradient) and validating that end-to-end — recorded as a scoped option, not built, since it validates a path OpenISI doesn't run. |

The cross-cycle `reliability` coherence is the Engel 1994 / Zhuang 2017 **published
formula** computed via numpy primitives (`sum`/`abs`) — a formula pin, labelled as
such (it has no canonical reference *code* to execute), not claimed as a code oracle.

**Library-primitive checks (numpy/scipy/skimage IS the oracle), not genuine-NAT
methods.** Some goldens were labelled as Allen/SNLC method oracles but are really a
single library primitive + a standard formula — the independent oracle is the
library, which they should compute *live* (condition 6), not the named reference:
- `dff` (ΔF/F): `F0 = np.mean(movie, axis=0)` + `(F−F0)/F0`. **`normalizeMovie` does
  NOT exist in NeuroAnalysisTools 3.1.0** (nor corticalmapping/retinotopic_mapping)
  — the old docstring's "Allen normalizeMovie" reference is absent; this is `np.mean`
  (numpy oracle) + the standard ΔF/F formula. Re-classified accordingly.
- The pure single-op goldens (`dft` vs `np.fft`, `gaussian`/`label`/`skeletonize`/
  `watershed`/`uniform_filter` vs scipy/skimage) are library-primitive by
  construction — the library is the genuine oracle; the remaining work for them is
  making them *live* (condition 6), not a reference migration.
  - **Now live** (computed every run in the locked env, no frozen fixture):
    `gaussian_smooth_f64` vs `scipy.ndimage.gaussian_filter` (max diff 1.1e-15),
    `label_4conn` vs `scipy.ndimage.label` (partition-identical, 4-conn cross),
    `binary_skeletonize_skimage` vs `skimage.morphology.skeletonize` (bit-identical,
    skimage 0.18.3 — bit-identical to 0.19.3 for the medial axis),
    `dft_projection_at_freq` vs `numpy.fft.fft(...)[1]` (single-bin, ≈8e-6 ≈ the
    f32 length-24 reduction vs numpy f64; same f32 values handed to numpy so only
    the reduction differs), `uniform_filter_finite` vs
    `scipy.ndimage.uniform_filter(mode='reflect')` (≈3e-15), `watershed_from_markers`
    vs `skimage.segmentation.watershed(connectivity=ones((3,3)), watershed_line=
    False)` (bit-identical — the call `Patch.split2` makes),
    `binary_opening_cross`/`binary_closing_cross` vs `scipy.ndimage.binary_opening`/
    `binary_closing` (4-conn cross, `border_value=0`; bit-identical — the patch-
    extraction morphology), `separable_filter` vs `scipy.ndimage.correlate1d`
    (mode='reflect', cols then rows; ≈5e-15 — exercises the large-radius
    periodic-wrap `reflect` fold), `temporal_mean_baseline` (the ΔF/F F0) vs
    `numpy.mean(movie, axis=0)` (bit-identical), `AllenZhuang2017ClipNegative`
    (half-wave rectify) vs `numpy.maximum(x, 0)` (bit-identical). `label` is
    compared label-invariantly (a CC labeling is defined only up to relabeling);
    the others bit-/precision-identical.
  - **Not a single-library-primitive → not made "live" as one (classified
    honestly):**
    - `patch_threshold` (`AllenZhuang2017FixedSignMapThr`, `Garrett2014SigmaScaled`)
      is a **threshold formula** (`|signMapf| ≥ thr`; `k·std·0.5` with MATLAB N−1
      std), not a standalone reference function. Its only library primitive is
      `np.std(ddof=1)`; the rest is OpenISI's rule with literature-grounded
      constants → a **formula-pin / regression-lock**, not a code oracle.
    - `keep_largest_component` (largest-CC tie-break) — the genuine reference is
      **SNLC `getMouseAreasX.m`** `[~,id]=max(S)` (first-max), a composition
      (label → component sizes → argmax → select), not one library call. It belongs
      to the **SNLC/Octave live batch** (executed against the real `.m` via the
      `snlc/` bridge), not a numpy/scipy primitive.

## Reproducibility (condition 7)

The harness is reproducible **by construction**: the genuine env is materialised
only from the committed `nat/uv.lock` (+ `pyproject.toml`), and `uv sync` rebuilds
the identical environment on any machine. The *execution* on a genuinely second
machine is the CI workflow **`.github/workflows/oracle.yml`**, which on a clean
GitHub runner installs `uv`, materialises the NAT env from the committed lock with
`uv sync --locked` (fails if the lock is stale → proves "from the committed lock
alone"), installs Octave + the image package, and runs the live suite
(`cargo test --features oracle_live`). That workflow IS the second-machine
reproducibility gate; it runs off the dev host on every change to the oracle/
harness/reference files.

