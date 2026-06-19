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
   ⇒ **It is a one-time calibration, not a per-run measurement** — characterize the
   monitor's latency once at commissioning, then run with that stored value. The
   characterization *method* is pluggable (§5.4): the rig's own camera, datasheet
   specs, a scanout model, a lag tester, or a photodiode — the photodiode is the
   most precise option, not a requirement.

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

- **P0 — Best-available cascade, honest at every rung (the GOVERNING principle; the
  rest are corollaries).** For *each signal*, use the **best source available** and
  **record which one** — never silently degrade, never overstate. The fidelity
  cascade: **photodiode** (emitted light) → **TTL** (hardware edge, commanded) →
  **GPU vsync** (software interrupt, commanded) → **camera-hardware-timestamp +
  commanded software timing** (best-effort), with **EDID** feeding the monitor-
  latency model at whatever rung is reached. Whatever rung a run lands on, the file
  states the **source + value + uncertainty** and flags each quantity as
  **measured / assumed / unknown**, so a consumer or reviewer always knows exactly
  what is and isn't grounded. A photodiode run and a no-EDID-no-TTL run are *both
  valid* — they carry honestly different uncertainties. This is the **same
  no-silent-fallback philosophy the codebase already enforces** for acquisition
  geometry (`ProvenanceLevel`: `Full` / `Partial{missing}` / `Defaulted` /
  `Synthetic`), applied to timing. P1–P7 below are how P0 is realized.
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
- **P7 — Scaffold for every source now; implement the software tier first.** The
  data model + provenance enumerate **all** timing sources (photodiode, TTL,
  vsync-fallback) and the schema reserves the optional (`When`) hardware-channel
  slots from day one — so TTL and photodiode capture become a *populated variant*
  later, never a schema migration. Phase 1 implements only the vsync-fallback
  (software) tier; the rest slot in. *(This is the governing decision for the
  current work: pure-software correctness now, but the right scaffolding so TTL —
  and the photodiode — drop in cleanly.)*

---

## 5. The clock & synchronization model

### 5.1 Clock domains
- **QPC domain** (today): camera system timestamps + stimulus vsync
  (`qpcVBlank`) + sweep schedule. The unified timeline `t₀` lives here.
- **Camera hardware domain**: the camera's internal clock (drift-tracked vs QPC).
- **DAQ domain** (new, optional): if a TTL DAQ is used, its sample clock. **Must
  be related to QPC by a stored mapping** (a co-recorded shared edge, or
  barcode-style alignment), with the residual error stored (P3).

### 5.2 Timing sources, ranked by fidelity (recorded per signal, per run)
| Source | What it measures | Signals | Fidelity | Status |
|---|---|---|---|---|
| **Photodiode** | *actual emitted light* (threshold-crossing) | display onset; optional per-run validation | **highest — the only emitted-light measurement** | scaffolded (optional); the most precise calibration method + per-run validator (§5.4) |
| **TTL** | hardware edge, but *commanded* | camera **shutter**, monitor **scanout/sync**, **light source** on/off | hardware-edge; needs the monitor-latency calibration | scaffolded (optional); implemented Phase 2 |
| **vsync-fallback** | software at the vblank interrupt, *commanded* | camera frame, stimulus refresh | software, commanded | **implemented today** |

The file records, **per signal**, which source produced its timing, the clock
domain, and the uncertainty (P5). Sources coexist (e.g. camera on TTL, light on
vsync, a photodiode validating the display) — provenance is **per-signal**, not
per-file. Note the ordering: a **photodiode is *higher* fidelity than TTL** — TTL
on a GPU/genlock refresh is still *commanded* time, whereas the photodiode is the
emitted photons themselves.

### 5.3 The irreducible gap both tiers share — and how we close it
Neither a vsync timestamp **nor a TTL on the GPU refresh** is the emitted-photon
time; both are *commanded/refresh* time, offset from light by the monitor's
latency + scanline position (§2.2). We close this with the **monitor-latency
calibration** (§6.F, P4): a one-time photodiode characterization stored in the
file, which analysis applies to convert commanded→emitted time with a stated
uncertainty. *(The one exception: if the "monitor TTL" is itself a photodiode-
derived edge, it measures emitted light directly per-run — see open question
§10.1.)*

### 5.4 The monitor-latency calibration is REQUIRED; the photodiode is NOT
The commanded→emitted monitor latency (§2.2, §5.3) must be **characterized** — that
value is a required input. But it does **not** have to be characterized *by a
photodiode*; conflating "the latency must be measured" with "by a photodiode" would
smuggle a photodiode dependency back into a photodiode-free design. The
characterization **method is pluggable**, and several are genuinely photodiode-free:

| Method | Photodiode-free? | Precision | Notes |
|---|---|---|---|
| **Rig's own camera images the monitor** | ✅ (uses existing hardware) | ~one camera frame (10–33 ms) | A camera is a slow photodiode array; capture the luminance rise of a commanded flash, hardware-timestamped, same clock domain. Bounds the offset; not sub-frame. |
| **Datasheet / independent-review specs** (input lag + g2g response) | ✅ | ~ms | No measurement; per-unit + settings-dependent. |
| **EDID scanout model + panel-response measurement** | ✅ (EDID is free) | scanline-exact + residual | EDID's Detailed Timing (pixel clock + h/v active/blank/sync) gives the scanline-dependent scanout geometry *deterministically* — only the panel-response + input-lag residual needs measuring. We already parse EDID for size (`monitor.rs`) but discard the timing; extracting it makes this method largely free. (EDID's CEA/HDMI VSDB *may* also self-report a coarse `video_latency`.) |
| **Commercial lag tester** (Bodnar, …) | ✅ (rig stays free) | sub-ms | Is itself a photodiode device, used once externally. |
| **Photodiode** | ✗ (adds the sensor) | ~0.1 ms | Most precise, direct, field-standard; *and* validates per-run. The best, but **optional**. |

So the `/calibration/display` slot stores the **latency value + the method that
produced it + its uncertainty** (P5). The deterministic scanline-geometry term is
**EDID-derived** regardless of which method supplies the panel-response/input-lag
residual. A camera-self-calibrated run and a
photodiode-calibrated run are both valid — they carry different declared
uncertainties. The system is **photodiode-free-capable** (camera self-cal /
datasheet, coarser) *and* **photodiode-ready** (precise, optional).

**Why we still scaffold the photodiode (decision: yes):** it is the *most precise*
calibration method **and** the field-standard *per-run validation* channel (a
photodiode in a screen corner measures the emitted light TTL/vsync cannot, closing
§5.3's gap per-run), for a scaffolding cost of ~zero (an optional `When` signal slot
+ one provenance variant) versus a format migration if retrofitted. It is the best
*option*, not a *requirement*.

So the data model reserves (Phase 1, unwritten): a method-tagged
`/calibration/display` slot (populated by *any* of the methods above — camera
self-cal is the default photodiode-free path) and an optional
`/acquisition/photodiode` signal + a `photodiode` provenance source, alongside the
TTL scaffolding.

---

## 6. Proposed data model (schema additions)

All additions are declared once in `crates/oisi/src/schema.rs` (SSoT), golden
`docs/oisi.schema.json` regenerated, both writers contract-tested, guarded by the
bit-identical equivalence gate. Names follow the existing convention (snake_case,
unit suffixes). Each item is marked **[POPULATE]** (written in Phase 1, software
tier) or **[SCAFFOLD]** (declared as optional `When` now, written when the hardware
arrives — the format is ready, per P7).

**A. Sync provenance — `/acquisition/sync` (group + attrs). [POPULATE]** The
machine-readable form of P0: the file's self-declaration of *how it was timed*, so
a consumer never has to guess. Per signal: the `source` it landed on in the
cascade ∈ {`photodiode` | `ttl` | `gpu_vsync` | `camera_hw_best_effort`} (the full
enum exists from day one; Phase 1 writes `gpu_vsync`/`camera_hw_best_effort`), its
clock domain, and a `quality` tag ∈ {`measured` | `assumed` | `unknown`} with a
per-signal `uncertainty_sec`. Plus `ttl_present`/`photodiode_present`/`edid_present`
(bool), DAQ model + clock rate (if any), the **DAQ↔QPC mapping** + residual, and a
top-level `timing_uncertainty_sec` budget. A run is trustable from this alone:
nothing is silently degraded, nothing is overstated. → NWB `/general` +
`TimingForensics`.

**B. TTL edge channels — `/acquisition/ttl/<channel>`. [SCAFFOLD]** Per channel
(`camera_shutter`, `monitor_scanout`, `light_source`): edge timestamps (raw DAQ +
unified seconds), polarity, channel semantics; drop/gap detection per channel.
Optional (`When` TTL DAQ present). → NWB `ndx-events` `EventsTable` (one per
channel; `timestamp` in s; edge/polarity as `CategoricalVectorData`).

**B′. Photodiode signal — `/acquisition/photodiode`. [SCAFFOLD]** Optional analog
trace + derived threshold-crossing onsets (the *emitted-light* measurement, §5.4):
samples or onset timestamps (unified seconds), threshold, channel placement.
Optional (`When` a photodiode is connected). → NWB `ndx-events` `EventsTable` +
analog `TimeSeries`. Doubles as the per-run validation of §5.3's gap.

**C. Per-frame stimulus angle — `/acquisition/stimulus/{azi,alt}_angle_deg`
(or a unified `stimulus_position_deg`). [POPULATE]** The **commanded stimulus angle/position
per stimulus frame** — the exact ground truth the analysis needs, *stored, not
interpolated from `progress`* (P1, the SNLC-trap closure). `progress`/`state_ids`/
`sweep_indices` remain for context. This is the single highest-value addition.

**D. Light-source timing — `/acquisition/light` (or TTL channel + metadata).
[POPULATE commanded on/off + wavelength/intensity; SCAFFOLD the TTL edges]**
On/off edges (TTL or commanded), wavelength + intensity per epoch, and the
relationship to camera exposure (ISI signal ∝ illumination, so this is
load-bearing data, not metadata). → `ndx-events` + `ndx-openisi`.

**E. Camera exposure window — `/acquisition/camera/exposure_{start,end}_us`.
[POPULATE]** (or center + width). For fast-moving stimuli the sensor integrates over a window
that may span multiple monitor scanouts; storing the window (vs only a point
timestamp + a scalar `camera_exposure_us`) removes an analysis assumption (P1).

**F. Monitor-light-latency calibration — `/calibration/display` (or a versioned
sidecar referenced by the run). [SCAFFOLD — populated once by *any* method in
§5.4; camera-self-cal is the default photodiode-free path]** Captured once: onset
latency (commanded→emitted), scanline dependence (top/center/bottom), rise
profile / pixel-response, the panel + driver identity it applies to, **the method
that measured it** (`camera_self_cal` | `datasheet` | `scanout_model` |
`lag_tester` | `photodiode`), *how/when it was measured*, and its uncertainty
(method-dependent, P5). Analysis applies it to lift commanded timestamps to
emitted-light time (P4). Versioned + provenance-stamped so a run records which
calibration — and which method — it used.

**H. Monitor descriptor (EDID) — `/hardware/edid` (raw) + parsed fields.
[POPULATE — extend existing]** We already parse EDID for physical size
(`monitor.rs::parse_edid_physical_size`) but discard the rest. Fully extract +
store: identity (mfr/model/serial), physical size, and the **Detailed Timing**
(pixel clock + h/v active/blank/sync) that deterministically yields the
scanline-dependent scanout geometry feeding F's `scanout_model`; plus the CEA/HDMI
VSDB `video_latency` when present (coarse, optional). Zero new hardware — the data
is already on the wire. → NWB `/general/devices` + `ndx-openisi`.

**G. Timing forensics — extend the existing `/acquisition/{timing,clock_sync,
quality}`. [POPULATE]** Keep the regime/beat-period/drift/drop model; make its uncertainty
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

### 7.1 The analysis must *consume* the hierarchy (or it's recorded-but-unused)
A timing hierarchy is only worth building if the analysis uses it. Today the
cycle-combine DFT is the faithful Allen `get_average_movie`: it assumes **uniform**
camera-frame spacing (`mean_frame_dur`) and matches each sweep to the **nearest**
camera frame — it ignores the recorded per-frame timestamps. That is correct and
oracle-validated when jitter is small, but it leaves the recorded timing on the
table. So we add a **selectable** *non-uniform-time* projection method (the
codebase's tagged-enum pattern): a Lomb–Scargle / NUDFT that projects at the
stimulus frequency using each sample's **actual** `cam_ts_sec` and the
**best-available** stimulus onset (P0 cascade) corrected by the monitor-latency
calibration (F) — so camera jitter, non-uniform spacing, and the commanded→emitted
offset are *corrected from measurement* rather than assumed away. The faithful
uniform Allen method stays the oracle-validated **default**; the non-uniform method
is the rigorous path that makes the hierarchy pay off. (This is also what finally
*uses* the per-frame stimulus state arrays, resolving their "recorded-but-unused"
status — they were forensic until an estimator consumed them.)

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

- **Phase 1 — software-tier completeness + full scaffolding (no new hardware).**
  *Populate* the per-frame stimulus angle (C), light-source timing (D), camera
  exposure window (E), and the sync-provenance record (A) in its `vsync_fallback`
  form, from what we *already* measure. *Scaffold* (declare as optional `When`, with
  the full source enum) the TTL channels (B), the photodiode signal (B′), and the
  `/calibration/display` slot (F) — so the format is **TTL- and photodiode-ready the
  day Phase 1 lands**, no later migration (P7). Highest value-per-effort, and the
  prerequisite scaffolding for everything after.
- **Phase 2 — TTL/DAQ capture (B) + clock-domain mapping (P3).** Populate the TTL
  scaffolding; DAQ↔QPC sync + residual.
- **Phase 3 — monitor-latency calibration (F).** The commissioning protocol that
  *populates* the reserved calibration slot via the chosen method (§5.4 — camera
  self-cal is the default photodiode-free path; photodiode optional for precision +
  per-run validation, scaffolded as B′); analysis applies the stored calibration.
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
calibrate-once the monitor latency (by any method in §5.4 — EDID + the rig's own
camera need no photodiode), declare the provenance and uncertainty, derive
nothing* — grounded in the ISI literature and the NWB standard.
Phase 1 (per-frame angle, light-source timing, exposure window, sync-provenance
record) closes the storage side with zero new hardware and goes first.
