# Timing & Synchronization Data Model — Design

**Status:** Design proposal for review. No code. Supersedes nothing yet; when
accepted, drives schema additions (SSoT in `crates/oisi/src/schema.rs`), the
acquisition path, the analysis consumers, and the NWB export.

---

## 1. Why this document exists

Our governing failure mode — call it **the SNLC trap** — is *store too little,
derive forever*. The SNLC sample data (R43) omitted so much timing/stimulus
ground truth that the entire hemodynamic-delay investigation this project ran was
*reverse-engineering measurements that should have been recorded*. We will not
reproduce that trap one level up.

We are building a deliberately **modern, single-computer ISI rig**:

- **One computer** (powerful CPU+GPU), not the traditional two-computer rig.
- **Photodiode-free** in production.
- **TTL-primary** timing for the **stimulus monitor, camera, and light source**.
- **Camera + GPU-vsync hardware timestamps as the fallback** when TTL is absent.
- Rigorous, clean, self-describing timing forensics — enough to synchronize
  stimulus frames/angles with camera frames *without analytic re-derivation*.

This is a real modernization, but it is only **scientifically defensible** if the
timing data is rigorous enough to stand in for what the field-standard photodiode
rig measures. This document grounds that claim in the literature and specifies the
data model that earns it.

### The thesis (the bar we must clear)

> **Capture the same ground truth a photodiode rig captures — as TTL hardware
> edges plus a one-time monitor-light-latency calibration — with *cleaner*
> provenance because there is a single clock domain; state the sync source and its
> uncertainty explicitly in the data; and never make an analysis step derive what
> a measurement should have recorded.**

Four principles fall out of that thesis (§4).

### Verified finding (checked against the Allen reference source, not a comment)

This is not hypothetical. Our `compute/projection.rs` is a **line-faithful port of
Allen's `get_average_movie`** (`reference/corticalmapping/.../core/ImageAnalysis.py:1175–1212`):
`meanFrameDur = mean(diff(frameTS))`, `chunkFrameDur = ceil(chunkDur/meanFrameDur)`,
`onsetFrameInd = argmin(|frameTS − onset|)`, `mov[onsetFrameInd : +chunkFrameDur]`.
The compute kernel is correct and faithful.

**But the algorithm's accuracy hinges entirely on its input `onsetTimes`, and there
the fidelity breaks.** In the canonical Allen pipeline those onsets are the
**photodiode threshold-crossings** — the *actual measured light-onset* times
(`HighLevel.py:187`: `displayOnsets = get_onset_timeStamps(pdSignal, …)`, where
`pd = photodiode from the mapping jphys file`). In OpenISI today, the same input is
`sweep_start_sec` — the stimulus thread's **QPC timestamp of the *commanded*
sequencer sweep-start event** (software-timed, no photodiode).

> **We faithfully implement Allen's compute kernel, but feed it a *software-
> commanded* onset where Allen feeds a *photodiode-measured* display onset. We are
> faithful to the kernel, not to the method's timing rigor.**

The consequence is **not noise — it is a systematic phase offset in every recovered
map**, equal to the commanded→emitted monitor latency (~tens of ms, scanline-
dependent; §2). The photodiode is precisely what removes it in the real method. And
it is indistinguishable, in the recovered phase, from a hemodynamic delay — so the
onset-timing error *contaminates the very delay this project spent its effort
isolating*. **This is the concrete, source-verified instance of the SNLC trap, and
it is the primary thing this design exists to fix:** the photodiode-grade onset
(via TTL + a monitor-latency calibration) is the **required input** that makes the
faithful algorithm accurate — not a nicety.

---

## 2. What the field requires (literature grounding)

1. **The photodiode is the field-standard ground truth we are replacing.** The
   canonical ISI rigs (Allen/Garrett/Zhuang lineage) use *"a photodiode recording
   the timing of visual display events,"* typically on a two-computer rig (primary
   = camera + photodiode + light; secondary = stimulus; UDP between them)
   ([Juavinett et al., Sci Rep 2022](https://www.nature.com/articles/s41598-022-05932-2)).
   Its unique value: it measures **actual emitted photons**, not commanded/refresh
   time.

2. **Commanded/refresh time ≠ on-screen time, by a large, characterizable
   amount.** Measured TTL→photodiode latency ≈ **29 ms**, software→screen ≈ 31 ms,
   and it is **scanline-dependent** (top of the display lights before the bottom)
   ([PLOS One: Measuring Software Timing Errors](https://journals.plos.org/plosone/article?id=10.1371/journal.pone.0085108);
   [PhotoNeuro 2025](https://www.ncbi.nlm.nih.gov/pmc/articles/PMC12861737/)).
   A vsync timestamp — *or even a TTL on the GPU's refresh* — does **not** capture
   this; only a photodiode (or a per-monitor calibration of it) does.

3. **Monitor light latency is characterizable per-monitor**, by photodiode
   threshold-crossing (onset = luminance crosses ~40% of max, digitized at ~10 kHz
   → 0.1 ms resolution), and differs by panel technology (CRT vs LCD vs gaming/
   G-Sync) ([Behavior Research Methods 2018](https://link.springer.com/article/10.3758/s13428-018-1018-7);
   [Sci Rep 2020 CRT vs LCD](https://www.nature.com/articles/s41598-020-63853-4)).
   ⇒ **It is a one-time calibration, not a per-run measurement** ("calibrate once
   with a photodiode at commissioning, then run photodiode-free").

4. **TTL synchronization standard** (Open Ephys / Bonsai): route a **camera
   *shutter* TTL per frame** into a master clock; when streams have *different*
   hardware clocks, align them with **barcode sync** (unique temporal codes every
   ~30 s) ([Open Ephys: Synchronizing Data Streams](https://open-ephys.github.io/gui-docs/Tutorials/Data-Synchronization.html);
   [sync-barcodes](https://github.com/open-ephys/sync-barcodes)). **Our
   single-computer QPC domain sidesteps the cross-clock problem entirely — unless a
   TTL DAQ introduces its own clock, in which case we must sync and store the
   mapping.**

5. **NWB has a standard home for this.** `ndx-events` (folding into NWB core via
   NWBEP001, 2025) defines `EventsTable` (`timestamp` = `TimestampVectorData` in
   **seconds** + optional `duration` + categorical columns via
   `CategoricalVectorData`/`MeaningsTable`) — the canonical container for TTL
   pulses and stimulus events ([ndx-events](https://github.com/rly/ndx-events)).
   Timing forensics + calibration map to our `ndx-openisi` `TimingForensics`
   extension. ⇒ **the export target is fixed and standard.**

---

## 3. What we already capture (the strong foundation — not starting from zero)

Audited current state (`src-tauri/src/{timing.rs,export.rs,stimulus_thread.rs}`,
`crates/oisi/src/schema.rs`, `crates/openisi-stimulus/src/dataset.rs`):

- **Triple camera timestamps** — hardware (camera clock), system/QPC, sequence
  numbers — with hardware **sequence-gap drop detection** (authoritative).
- **One shared QPC clock domain** for camera *and* stimulus (stimulus times = DWM
  `qpcVBlank`, the vsync-interrupt QPC time). *This is the single-computer
  superpower: no cross-machine clock sync problem at all.*
- **Sub-frame timing model** (`timing.rs`): the camera↔stimulus **beat period** +
  **regime** (uniform / systematic / partial) + phase coverage, which *warns when
  sub-frame onset bias is uncorrectable*. More rigorous than most published rigs.
- **Clock-drift forensics** (camera-vs-system offset + drift), per-frame intensity
  (illumination drift), and a `stimulus_timing_validatable` + `display_scanout`
  flag that already refuses to trust timing on a non-physical scanout (RDP).

**Gaps (the subject of this design), primary first:**
- **The onset-timing input (verified, §1).** `sweep_start_sec` fed to the faithful
  Allen kernel is a *software-commanded* QPC onset, not a photodiode/TTL-measured
  display onset — a systematic, uncorrected monitor-latency offset in every map.
  *This is the headline; everything below supports fixing it.*
- No TTL capture; no light-source timing; no monitor-light-latency calibration to
  correct the commanded→emitted offset.
- Sync provenance + uncertainty not yet first-class.
- Per-frame stimulus **angle derived from `progress`, not stored** (lower severity
  — `progress` accumulates *real* QPC deltas so it is robust to dropped frames, and
  the bar is linear-in-time by design, so the angle is exactly recoverable; storing
  it is a render-independence improvement, not a correctness fix).

**Secondary forensics-rigor gaps found in the live code review (real, bounded):**
`onset_uncertainty_sec` omits the computed `cam_jitter_sec` from its RSS
(`timing.rs:156`); the clock-drift estimate is a 2-point first/last difference with
no midnight-wrap guard (`export.rs`); several thresholds are ungrounded magic
numbers (regime thresholds; `1.5×` timing-anomaly factor; `5%`/`60`-frame
catastrophic-drop; `1.5×` stimulus-drop); `t0` = first frame with no warmup-skip;
and the "shared QPC domain" is true for the *system* timestamps (used for
alignment) but the camera *hardware* timestamp is the camera's own clock — a
documentation-clarity gap, not a bug.

---

## 4. Design principles

- **P1 — Store, don't derive.** Anything a measurement (or the stimulus
  controller) *knows at acquisition time* is stored, not reconstructed downstream.
  Directly closes the SNLC trap. (Per-frame stimulus angle is the headline case.)
- **P2 — Hardware edges where available; software as *labeled* fallback.** TTL is
  Tier 1; camera-hardware-timestamp + GPU-vsync is Tier 2. The tier is recorded;
  analysis never has to guess which it got.
- **P3 — Preserve the single-clock advantage; if a clock is added, sync it
  explicitly.** A TTL DAQ may have its own oscillator. If so, store the DAQ↔QPC
  mapping (shared edge or barcode) *and its residual*, never an implicit
  assumption of lock.
- **P4 — Calibrate-once what you cannot measure-always.** The monitor's
  commanded→emitted-light latency (the photodiode's job) is a **stored
  calibration**, measured at commissioning, applied by analysis — not silently
  ignored.
- **P5 — Provenance + uncertainty are first-class.** Every timing quantity states
  its **source** (which signal/clock), and the file declares the sync **tier** and
  a quantified **uncertainty**. A reviewer can trust or reject a run from the data
  alone.
- **P6 — Ground in standards; export losslessly.** Every field maps to NWB
  (`ndx-events` / `ndx-openisi`) and is documented against the literature.

---

## 5. The clock & synchronization model

### 5.1 Clock domains
- **QPC domain** (today): camera system timestamps + stimulus vsync
  (`qpcVBlank`) + sweep schedule. The unified timeline `t₀` lives here.
- **Camera hardware domain**: the camera's internal clock (drift-tracked vs QPC).
- **DAQ domain** (new, optional): if a TTL DAQ is used, its sample clock. **Must
  be related to QPC by a stored mapping** (a co-recorded shared edge, or
  barcode-style alignment), with the residual error stored (P3).

### 5.2 Two timing tiers (recorded per signal, per run)
| Tier | Source | Signals | Fidelity |
|---|---|---|---|
| **Tier 1 — TTL** | DAQ-captured hardware edges | camera **shutter**, monitor **scanout/sync**, **light source** on/off | hardware-edge; highest |
| **Tier 2 — vsync fallback** | camera hardware timestamps + GPU vsync (`qpcVBlank`) | camera frame, stimulus refresh | software-at-interrupt; current |

The file records, **per signal**, which tier produced its timing, the clock
domain, and the uncertainty (P5). Both tiers can coexist (e.g. camera on TTL,
light source on vsync) — provenance is per-signal, not per-file.

### 5.3 The irreducible gap both tiers share — and how we close it
Neither a vsync timestamp **nor a TTL on the GPU refresh** is the emitted-photon
time; both are *commanded/refresh* time, offset from light by the monitor's
latency + scanline position (§2.2). We close this with the **monitor-latency
calibration** (§6.F, P4): a one-time photodiode characterization stored in the
file, which analysis applies to convert commanded→emitted time with a stated
uncertainty. *(The one exception: if the "monitor TTL" is itself a photodiode-
derived edge, it measures emitted light directly per-run — see open question
§10.1.)*

---

## 6. Proposed data model (schema additions)

All additions are declared once in `crates/oisi/src/schema.rs` (SSoT), golden
`docs/oisi.schema.json` regenerated, both writers contract-tested, guarded by the
bit-identical equivalence gate. Names follow the existing convention (snake_case,
unit suffixes).

**A. Sync provenance — `/acquisition/sync` (group + attrs).** The file's
self-declaration of *how it was timed*: `sync_tier` per signal (`ttl` |
`vsync_fallback`), the clock domain of each, `ttl_present` (bool), DAQ model +
clock rate (if any), the **DAQ↔QPC mapping** + residual, and a top-level
`timing_uncertainty_sec` budget. This is what makes a run trustable from the data
alone (P5). → NWB `/general` + `TimingForensics`.

**B. TTL edge channels — `/acquisition/ttl/<channel>`.** Per channel
(`camera_shutter`, `monitor_scanout`, `light_source`): edge timestamps (raw DAQ +
unified seconds), polarity, and channel semantics. Drop/gap detection per channel.
→ NWB `ndx-events` `EventsTable` (one per channel; `timestamp` in s; edge/polarity
as `CategoricalVectorData`).

**C. Per-frame stimulus angle — `/acquisition/stimulus/{azi,alt}_angle_deg`
(or a unified `stimulus_position_deg`).** The **commanded stimulus angle/position
per stimulus frame** — the exact ground truth the analysis needs, *stored, not
interpolated from `progress`* (P1, the SNLC-trap closure). `progress`/`state_ids`/
`sweep_indices` remain for context. This is the single highest-value addition.

**D. Light-source timing — `/acquisition/light` (or TTL channel + metadata).**
On/off edges (TTL or commanded), wavelength + intensity per epoch, and the
relationship to camera exposure (ISI signal ∝ illumination, so this is
load-bearing data, not metadata). → `ndx-events` + `ndx-openisi`.

**E. Camera exposure window — `/acquisition/camera/exposure_{start,end}_us`**
(or center + width). For fast-moving stimuli the sensor integrates over a window
that may span multiple monitor scanouts; storing the window (vs only a point
timestamp + a scalar `camera_exposure_us`) removes an analysis assumption (P1).

**F. Monitor-light-latency calibration — `/calibration/display` (or a versioned
sidecar referenced by the run).** The photodiode's role, captured once: onset
latency (commanded→emitted), scanline dependence (top/center/bottom), rise
profile / pixel-response, the panel + driver identity it applies to, *how/when it
was measured*, and its uncertainty. Analysis applies it to lift commanded
timestamps to emitted-light time (P4). Versioned + provenance-stamped so a run
records which calibration it used.

**G. Timing forensics — extend the existing `/acquisition/{timing,clock_sync,
quality}`.** Keep the regime/beat-period/drift/drop model; make its uncertainty
**tier-aware** (a TTL run and a vsync-fallback run carry different uncertainty
budgets), and fold the DAQ↔QPC residual + calibration uncertainty into the
top-level budget.

---

## 7. The round-trip guarantee (the acceptance test for this design)

After these additions, an analysis can, **for every camera frame**, obtain:

1. the **emitted-light stimulus angle** that was on screen during that frame —
   *read from stored per-frame angle (C)*, *corrected by the stored monitor
   calibration (F)*, *in one clock domain (5.1)*, *with a stated uncertainty (G)*;
2. without re-deriving anything from assumptions (no "assume linear motion," no
   "assume vsync = photons," no "assume uniform frame timing").

When that holds, downstream quantities — the hemodynamic delay, the phase maps,
the VFS — are grounded in *recorded measurements*, not reverse-engineered. That is
the concrete, testable definition of "we did not fall into the SNLC trap," and it
is the doc's success criterion.

---

## 8. NWB / standards grounding (export mapping)

| `.oisi` | NWB target | Standard |
|---|---|---|
| TTL edge channels (B) | `ndx-events` `EventsTable` per channel, `timestamp` (s) + `CategoricalVectorData` (edge/polarity) | NWBEP001 (core 2025) |
| Per-frame stimulus angle (C) | stimulus `TimeSeries` / `ndx-openisi` stimulus table | NWB stimulus + extension |
| Light-source timing (D) | `ndx-events` + `ndx-openisi` | — |
| Sweep schedule (existing) | `TimeIntervals` | NWB core |
| Sync provenance + forensics + calibration (A,F,G) | `ndx-openisi` `TimingForensics` + `/general` | extension pattern |
| Camera exposure window (E) | `OnePhotonSeries` timestamps + metadata | NWB ophys |

Export stays transform-only / lossless (per `docs/INTEROP_NWB.md`).

---

## 9. Phased implementation plan (design-only here; each phase its own effort)

- **Phase 1 — software-tier completeness (no new hardware).** Store the **per-frame
  stimulus angle (C)**, **light-source timing (D)** and **camera exposure window
  (E)** from what we *already* measure, plus the **sync-provenance record (A)** in
  its `vsync_fallback` form. *This closes the SNLC trap immediately, before any
  TTL/DAQ hardware exists* — it is the highest value-per-effort phase.
- **Phase 2 — TTL/DAQ capture (B) + clock-domain mapping (P3).** The hardware
  Tier-1 path; DAQ↔QPC sync + residual.
- **Phase 3 — monitor-latency calibration (F)** protocol + stored calibration +
  analysis application.
- **Phase 4 — NWB export mapping (§8)** + DANDI.

Each schema change: SSoT edit → golden regen → contract test → bit-identical
equivalence gate.

---

## 10. Open questions for you (these shape the design; please resolve before build)

1. **What *is* the "monitor TTL" source?** Monitors rarely emit a native TTL. Is
   it (a) a GPU genlock/sync output (e.g. Quadro Sync) — which is still *commanded*
   refresh time and **requires the latency calibration (F)**; (b) a tap on the
   display signal; or (c) a *commissioning-only* photodiode→TTL that measures
   emitted light directly (so latency is per-run, not calibration)? This decides
   whether (F) is mandatory and whether emitted-light time is ever measured live.
2. **DAQ hardware + clock.** Which DAQ captures TTL, at what rate, and how is its
   clock related to QPC (shared edge? barcode? hardware-triggered co-sample)?
3. **Light source.** TTL-strobed per camera frame, continuously on, or wavelength-
   switched per epoch? (Determines whether D is per-frame edges or per-epoch.)
4. **Calibration cadence.** Photodiode characterization at commissioning only, or
   periodically (panels drift with temperature/age)?
5. **Per-frame angle representation.** Single `stimulus_position_deg` (the bar's
   1-D sweep coordinate) vs explicit `{azi,alt}_angle_deg` — and do we also store
   the rendered phase, to be fully render-independent?

---

## 11. Summary verdict

Our compute kernel is a verified-faithful port of the Allen method, and the
existing timing model is genuinely strong (shared single clock, sub-frame regime
analysis, triple timestamps, drift + drop forensics). But **source-verification
revealed we are *in* the SNLC trap on the one axis that matters most**: we feed the
faithful algorithm a *software-commanded* sweep onset where the canonical method
feeds a *photodiode-measured* display onset — a systematic, uncorrected
monitor-latency phase offset in every map, indistinguishable from a hemodynamic
delay. Closing that — the photodiode-grade onset via **TTL + a monitor-latency
calibration**, with **sync provenance + uncertainty made first-class** — is the
point of this design. The secondary forensics-rigor gaps (cam-jitter in the
uncertainty RSS, midnight-wrap guard, grounding the magic-number thresholds,
warmup-robust `t0`, clock-domain documentation) are real, bounded, and fixed
immediately. The thesis throughout: *measure-and-store what hardware can give,
calibrate-once what only a photodiode could, declare the provenance and
uncertainty, derive nothing* — grounded in the ISI literature and the NWB standard.
Phase 1 (per-frame angle, light-source timing, exposure window, sync-provenance
record) closes the storage side with zero new hardware and goes first.
