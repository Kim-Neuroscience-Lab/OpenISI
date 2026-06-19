# Synthetic ground-truth validation — methodology

Companion to [`VALIDATION_SCORECARD.md`](VALIDATION_SCORECARD.md) (what we
validate today) and [`PIPELINE_METHODS.md`](PIPELINE_METHODS.md) (the methods +
their oracles).

## Status & scope (read this first)

**Built** (`crates/synth`, dev-only):
- The bedrock primitives — the analytic ground-truth map (`map::LogMap`) and the
  Kalatsky–Stryker encoder (`encode`), unit-tested against their known properties.
- **Phase A (2026-06-18; delay model corrected 2026-06-19):** the realism layer's
  hemodynamic delay (canonical `Hemodynamic::PhaseLag` — a known positive phase ∠H,
  unit gain; plus the optional physical difference-of-gamma `Hrf` stress knob) +
  sensor noise (`realism`), deterministic seeded randomness (`rng`, ChaCha
  substreams), recording assembly (`acquire` → `Synthetic`), and the **full-pipeline
  recover-and-compare correctness test** (`crates/isi-analysis/tests/
  synthetic_fullmovie.rs`): a physically-valid synthetic movie is run through the
  real from-raw pipeline and recovers the known retinotopy (altitude median ~0.002°,
  azimuth ~0.34°; position recovered even with ΔR/R below the shot-noise floor).
- **Findings the synthetic surfaced** (oracle-faithfulness tests can't — the oracle
  shares the conventions), all grounded against **R43 (real SNLC sample data)**:
  (1) **the hemodynamic-delay valid-domain rule.** The cycle-combine inherits SNLC
  `Gprocesskret`'s `(0, π]` delay-forcing; with map phases `±p + ∠H` it forms
  `delay = ∠H` and the forcing adds an uncompensated π **iff ∠H is negative**,
  flipping the recovered position by half the range. So the method is invertible
  **iff ∠H ∈ (0, π]** (the general form of the zero-delay singularity;
  `position_flips_iff_delay_leaves_valid_domain` proves it deterministically). Real
  ISI lives in-domain: R43's positions are correct and its per-pixel delays cluster
  at ~85° (`azi_delay` median 98° / `alt_delay` 71°), with only a ~2–4%
  noise-dominated tail at the 0/π edges where even SNLC flips. The canonical recording
  therefore injects a known positive delay (`PhaseLag`, default = R43's ~85° median),
  not a gamma HRF whose bin-1 phase can wander out of domain. (2) a small uniform
  **~0.34° azimuth bias** — *decisively localized* (`delay_bias_math_vs_numerical`)
  to the **movie→complex-maps front-end**: the Kalatsky–Stryker formula is exact
  (machine-ε in pure f64) AND our pipeline is exact on exact complex maps (0.0000),
  so the bias is an f32-DFT + u16-quantization artifact, azimuth-specific because the
  per-pixel map errors cancel on the symmetric altitude map but not the fovea-
  asymmetric azimuth map. It is the **same magnitude under the unit-gain `PhaseLag`
  delay and the attenuated `Hrf`**, so it is NOT an attenuation effect (an earlier
  "reducible by a more realistic HRF / less attenuation" note was wrong — corrected).
  The delay subtraction only makes it *visible* as a position offset; it does not
  cause it. (An even earlier note attributing it to the delay subtraction was
  confounded by signal size — also corrected.)

**Raw-`.oisi` writer (BUILT 2026-06-19).** `oisi::io::write_raw_acquisition` emits
a **schema-conformant** raw `.oisi` containing the *source-agnostic* raw content
every raw acquisition genuinely has — the camera movie (+ ideal synthetic camera
clock) and the realized sweep `schedule` + geometry attrs. It deliberately does
**not** write the capture-time telemetry (`/acquisition/{stimulus,clock_sync,
quality}`): that comes from the stimulus presentation system (`StimulusDataset`) +
capture-export QA, which the *stimulus-agnostic* `oisi` format layer cannot
honestly produce — so the schema marks those subgroups `When("capture-time …")`
(the capture path still writes them, guarded by an explicit presence test in
`export.rs`). `source_type` marks the file synthetic so it is never mistaken for a
real capture, and `analyze()` lifts it back to `ProvenanceLevel::Synthetic`. The
recover-and-compare now routes through it: `synth → write .oisi → analyze() → read
/results`, so the synthetic exercises the exact `read_raw_acquisition` ingest path
a real capture uses. Living in the light `oisi` crate (the format's home), it needs
no analysis compute — the same writer a future frame-only importer would use, and
the producer of committable fixtures.

**Deferred** (designed below, not built): the remaining realism knobs (PSF,
physiological lines, drift, vasculature, saturation), the oracle-handoff adapters
(the input layer for the full-pass oracle golden), the hardened multi-area
wedge-dipole map, the **stress battery / cross-implementation benchmark**, and
publication.

**Why deferred — the honest cost/benefit.** OpenISI's thesis is *faithful
recapitulation of the field's methods*, and we already validate that
bit-identically against the field's accepted-correct oracles (SNLC/Garrett,
Allen/Zhuang). For this tool, "we reproduce the validated standard" is the
*primary* correctness claim; synthetic ground truth (recovery of an idealized
model *we* built) is a valuable but **incremental** complement, and it is not on
the critical path to Alpha (live-rig + UI) or Beta (self-containment). The
primitives above are a low-cost down payment that keeps the design alive; the
rest is completed when it is the highest-value work — and the full
cross-implementation, openly-released benchmark is a **separate paper-scale
contribution** (it would validate the field's *other* tools against ground truth
too, the way the pRF validation framework tested four implementations), with its
own research risk (defending a mouse forward model; showing the synthetic data
predicts real-data accuracy). The interface seam — "run on an `.oisi`, read back
`/results`" — is the cheap-now/expensive-later choice that keeps that paper a
natural extension rather than a rewrite.

## Why this exists — the gap synthetic data fills

OpenISI today validates on **real data**, two ways:

1. **Oracle faithfulness** — per-stage golden tests assert our output matches the
   field's reference implementations (SNLC/Garrett MATLAB, Allen/Zhuang Python)
   on the same input. This tests *faithfulness to the reference*.
2. **Bit-identical regression** — `regression_oisi` recomputes the pipeline from a
   real raw movie and asserts the scientific output doesn't move. This tests
   *reproducibility*.

Both are essential and neither answers a third question: **does the pipeline
recover the _correct_ answer?** A reference implementation can be wrong, or right
only in the regime it was tuned on; "we match the oracle" inherits its blind
spots. The only way to test correctness directly is to put in a **known ground
truth** and measure what comes back — which requires *generating* the data.

Retinotopy is unusually well-suited to this because it has a **known, invertible
forward model**: the cortical→visual map and the periodic-stimulus encoding are
both explicit, so we can synthesize a raw movie from a chosen true retinotopy and
check recovery exactly. This is the standard validation paradigm in the fMRI
population-receptive-field (pRF) literature — see *the validation framework* below
— ported to widefield/intrinsic-signal imaging.

Synthetic ground truth uniquely gives us:

- **Correctness** (recovers the true map, not just the reference's output).
- **An operating envelope** — accuracy-vs-condition curves (SNR, sweep count …)
  that characterize where the pipeline works and where it breaks, which a
  reference tool should publish.
- **Numerical-conditioning validation** — exact recovery error lets us check that
  error grows where the condition number predicts (ties to the `agreement`
  tolerance work; see [`VALIDATION_SCORECARD.md`](VALIDATION_SCORECARD.md)).
- **Committable, shareable fixtures** — a synthetic raw `.oisi` generated from a
  small committed spec is reproducible-from-seed, has no privacy/licensing
  constraints, and is sized to fit in git/CI — unlike the multi-GB private rig
  recordings, which can't be committed or shared.

## The forward model (what to generate, and the citations for each piece)

### 1. The ground-truth map — complex-log / wedge-dipole

Use an **analytic** cortical→visual map rather than a hand-drawn one, so the truth
is principled and parameterized. The canonical model is the **complex-logarithmic
(conformal) map** of striate cortex (Schwartz 1980), extended to the V1–V2–V3
complex as the **wedge-dipole** map (Balasubramanian, Polimeni & Schwartz 2002).
This is exactly how synthetic retinotopy ground truth is generated in the
human-retinotopy parameterization literature.

Two properties make it the right choice for *us* specifically:

- It produces realistic **cortical magnification** (the inverse-magnification
  super-linearity in the periphery), so the synthesized maps stress our
  `magnification` / `eccentricity` outputs with a *known* answer.
- The wedge-dipole model **analytically predicts magnification _anisotropy_** from
  the shared V1–V2/V2–V3 boundary conditions — i.e. it gives a closed-form ground
  truth for `magnification_axis` / `magnification_distortion` (oracle-coverage
  gap #3), which no real-data oracle does.

### 2. The encoding — periodic phase-encoded stimulation

From the ground-truth visual coordinates, synthesize each pixel's time series via
the **phase-encoded / temporally-encoded** paradigm OpenISI implements: a drifting
bar sweeping the visual field at frequency `f` makes a pixel respond when the bar
crosses its receptive field, so its time course is periodic at `f` with **phase =
the pixel's true visual position**, per sweep direction (Kalatsky & Stryker 2003;
the fMRI analogue is Engel, Glover & Wandell 1997). Forward and reverse sweeps in
each orientation are encoded as separate epochs on the acquisition schedule, which
is what lets delay-subtraction separate hemodynamic delay from position.

### 3. The realism layer — breaks the circularity (critical)

A pure single-frequency sinusoid is the *most idealized possible* input; recovering
it tests nothing. The discipline — the same "don't derive the test from the thing
you're testing" principle as the tolerance work — is that the **forward model must
be richer than the pipeline's internal assumptions**, so the test measures
robustness to *model mismatch*, not assumptions against themselves. Add, each as a
tunable knob:

- **Hemodynamic delay and spatial point-spread.** The intrinsic signal is
  hemodynamic, low-pass in space (~100 µm resolution) and delayed/blurred relative
  to neural activity (Sirotin, Hillman, Bordier & Das 2009; the triphasic HRF,
  Sirotin & Das, *J. Neurosci.* 2007). A known delay directly exercises the
  delay-subtraction (`azi_delay`/`alt_delay`); a known PSF tests robustness of the
  phase/sign recovery to blur.
- **Noise** — photon/Gaussian plus, optionally, physiological (heartbeat,
  breathing, vasomotion) components at off-stimulus frequencies, to test the
  frequency-selective DFT's rejection.
- **Stimulus harmonics, slow baseline drift (photobleaching), and a vascular
  pattern** at non-stimulus frequencies.

> **Note on the IDFT shortcut.** Inverse-transforming the R43 `complex_maps` yields
> a movie whose DFT returns those maps *by construction* — `DFT ∘ IDFT = identity` —
> so as a standalone test it is **circular** (stage 0 becomes a tautology and the
> downstream stages merely reproduce results the equivalence test already checks).
> It is, however, a useful **seed**: it supplies *realistic* phase structure (the
> real R43 retinotopy) cheaply. It becomes a genuine test only once the realism
> layer above is added on top, breaking the round-trip.

## The validation framework (synth → recover → compare → find failure modes)

The procedure and its value are published: the pRF **validation framework** of
Lerma-Usabiaga, Benson, Winawer & Wandell (2020) — (1) synthesize time series from
ground-truth parameters, (2) run the analysis software, (3) compare recovery to
the ground truth — and, notably, it *"identified realistic conditions that lead to
imperfect parameter recovery … that would remain undetected using classic
validation."* That failure-mode discovery is the point of the **stress battery**,
not just the benchmark. The forward-model-and-recover paradigm originates with the
pRF method itself (Dumoulin & Wandell 2008).

## Two roles

**Benchmark** — one realistic, clean(ish) synthetic recording with a known answer;
report recovery accuracy as the headline numbers.

**Stress battery** — sweep each corruption knob and report where recovery degrades.
Each stressor maps to a specific pipeline stage / output and a ground-truth source:

| Stressor (knob) | Tests | OpenISI output | Ground-truth source |
|---|---|---|---|
| SNR sweep | noise floor / reliability masking | `reliability`, `cortex_mask` | injected noise level |
| Known hemodynamic delay | delay/position separation | `azi_delay`/`alt_delay`, phase | injected delay |
| Hemodynamic PSF (blur) | robustness of phase/sign to blur | `vfs`, area borders | injected PSF |
| Field-sign layout (mirror/non-mirror, thin borders, near-threshold patches) | segmentation correctness | `area_labels`, `area_signs`, `area_borders` | the analytic map's sign |
| Retinotopic fold (near-singular Jacobian) | `1/det` conditioning | `magnification` | analytic `det J → 0` locus |
| Anisotropic magnification | anisotropy recovery | `magnification_axis`/`_distortion` | wedge-dipole prediction |
| Phase straddling ±π | wrap handling | phase maps, polar angle | injected wrap |
| Few sweeps / low frame rate | DFT degradation | all phase/amplitude | known stimulus |

## Recovery metrics

- **Phase / eccentricity / polar-angle error** (degrees) vs the analytic map.
- **Field-sign accuracy** and **area-boundary IoU** vs the analytic sign/areas.
- **Magnification + anisotropy error** vs the wedge-dipole closed form.
- **Operating-envelope curves** — accuracy as a function of SNR, sweep count, delay.
- **Conditioning check** — recovery error vs the predicted condition number at the
  synthetic singularities; this *validates the error model* the `agreement`
  tolerances are built on (it should blow up exactly where `κ` says it will).

## Relationship to the other validation tiers, and honest limits

Synthetic ground truth **complements** — does not replace — oracle faithfulness and
the bit-identical regression:

- **Synthetic** → correctness, the operating envelope, numerical conditioning, and
  *committable* fixtures.
- **Real data** (oracle goldens, the rig movies) → that the *forward-model
  assumptions themselves* match physical reality — real hemodynamics,
  neurovascular coupling, motion, and non-Gaussian noise that any forward model
  only approximates.

Two honest caveats:

1. Synthetic data is only as good as its forward model; it validates the **math
   against an idealized world**, so a fully-independent check still requires real
   recordings. The realism layer narrows but does not close that gap (the model
   and the pipeline both assume periodic encoding).
2. There is **no turnkey published synthetic benchmark for mouse widefield / ISI
   periodic retinotopy** — the synthetic-validation literature is fMRI-pRF
   (Lerma-Usabiaga) and human-retinotopy parameterization (complex-log); the mouse
   ISI references (Garrett, Zhuang) validate on real data. OpenISI would be
   **composing** the cited pieces — the validation framework, the analytic map, the
   IOS hemodynamic forward model, and Kalatsky–Stryker encoding — into the first
   such benchmark for this modality. That composition is itself a modest
   contribution, and the synthetic recording is the openly-shareable benchmark the
   field currently lacks.

## References

Verified this session against the listed source:

- **Kalatsky, V.A. & Stryker, M.P. (2003).** New paradigm for optical imaging:
  temporally encoded maps of intrinsic signal. *Neuron* 38(4):529–545.
  doi:10.1016/S0896-6273(03)00286-1.
- **Schwartz, E.L. (1980).** Computational anatomy and functional architecture of
  striate cortex: a spatial mapping approach to perceptual coding. *Vision
  Research* 20(8):645–669. doi:10.1016/0042-6989(80)90090-5.
- **Balasubramanian, M., Polimeni, J. & Schwartz, E.L. (2002).** The V1–V2–V3
  complex: quasiconformal dipole maps in primate striate and extra-striate cortex.
  *Neural Networks* 15(10):1157–1163.
- **Lerma-Usabiaga, G., Benson, N., Winawer, J. & Wandell, B.A. (2020).** A
  validation framework for neuroimaging software: the case of population receptive
  fields. *PLOS Computational Biology* 16(6):e1007924.
- **Sirotin, Y.B., Hillman, E.M.C., Bordier, C. & Das, A. (2009).** Spatiotemporal
  precision and hemodynamic mechanism of optical point spreads in alert primates.
  *PNAS* 106(43):18390–18395.
- **Juavinett, A.L., Nauhaus, I., Garrett, M.E., Zhuang, J. & Callaway, E.M.
  (2017).** Automated identification of mouse visual areas with intrinsic signal
  imaging. *Nature Protocols* 12(1):32–43. (PMC5381647.)
- **Zhuang, J., Ng, L., Williams, D., Valley, M., Li, Y., Garrett, M. & Waters, J.
  (2017).** An extended retinotopic map of mouse cortex. *eLife* 6:e18372.

Canonical landmarks cited from standard knowledge (confirm bibliographic details
against the source before any external/formal use):

- **Engel, S.A., Glover, G.H. & Wandell, B.A. (1997).** Retinotopic organization in
  human visual cortex and the spatial precision of functional MRI. *Cerebral
  Cortex* 7(2):181–192.
- **Dumoulin, S.O. & Wandell, B.A. (2008).** Population receptive field estimates in
  human visual cortex. *NeuroImage* 39(2):647–660.
- **Sereno, M.I. et al. (1995).** Borders of multiple visual areas in humans
  revealed by functional MRI. *Science* 268:889–893.
- **Garrett, M.E., Nauhaus, I., Marshel, J.H. & Callaway, E.M. (2014).** Topography
  and areal organization of mouse visual cortex. *J. Neurosci.* 34(37):12587–12600.
- **Marshel, J.H., Garrett, M.E., Nauhaus, I. & Callaway, E.M. (2011).** Functional
  specialization of seven mouse visual cortical areas. *Neuron* 72(6):1040–1054.
- **Sirotin, Y.B. & Das, A. (2007).** The triphasic intrinsic signal. *J.
  Neurosci.* 27(17):4572.
