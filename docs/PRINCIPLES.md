# OpenISI Principles & Definition of Done

This is the codebase's architectural contract — what OpenISI *is*, the invariants
every change is held to, and the objective gates that define "done." It is not
aspirational prose: the [Definition of Done](#definition-of-done) is a checklist of
commands and checks that each pass or fail. The goal is reached exactly when every
gate is green — no judgment call.

`docs/TOOL_LEDGER.md` is this document's enforcement appendix (the tool-vs-domain
record). When the two disagree, fix the code, not the doc.

## Thesis

OpenISI is **a self-contained scientific instrument for ISI retinotopy, built to
become the field's reference tool** — software another lab installs and runs with
no setup, trusts the numbers from, extends, and exchanges data with through the
standards the field already speaks. Spotless, pristine, modern, validated. Held to
a single bar: **100% correct and complete — exactly the right size and scope.**
Never "good enough," never a throwaway, never enterprise-bloat. Both
under-engineering (hand-rolling a standard) and over-engineering (ceremony the
domain doesn't need) are failures against the same bar.

It is an **instrument, not a library.** It runs on a rig, drives a camera and a
stimulus display in real time, and captures experimental data that cannot be
re-taken. Every decision is therefore simultaneously a *data-integrity*, an
*adoption*, and a *trust* decision.

## Scientific motivation

*Why the invariants below are non-negotiable rather than aspirational.*

**The measurement.** OpenISI computes phase-encoded retinotopy from
intrinsic-signal movies: Fourier phase at the stimulus frequency encodes
visual-field position, forward/reverse sweeps cancel the hemodynamic delay, and
the azimuth × altitude maps yield the **visual field sign** whose reversals
parcellate visual cortex into distinct areas (V1, LM, AL, …). These areal maps are
the coordinate system the rest of mouse-vision neuroscience is done in — targeting
recordings to identified areas, and comparing areal organization across animals,
development, and disease.

**The problem it exists to fix.** Today this foundational measurement is not
trustworthy *as a measurement*. Each lab runs its own inherited scripts
(Garrett/SNLC MATLAB, Allen/Zhuang Python, home-brew) — un-interoperable,
unvalidated against one another, full of undocumented magic numbers and silent
conventions. Two labs analyzing the *same* recording can draw *different* area
boundaries, with no principled way to tell a real methodological disagreement from
a toolchain artifact.

**The vision.** Make areal analysis a **calibrated, shared, auditable
instrument**: a result is a property of *(the data, an explicitly-specified,
faithfully-implemented method)* and nothing else — not the operator, not the
machine, not the library version. That is what "the field's reference tool" means
here, and it is *why* faithfulness (Inv. 1), oracle validation (Inv. 3), and
reproducibility-by-record (Inv. 5) are correctness requirements, not features.

**The load-bearing principle — a grounded tolerance is the operational definition
of "a real difference."** An instrument's defining capability is to separate
signal from its own noise. Here that line is numerical: the floating-point floor
(`K·ε`, grounded in IEEE-754 machine epsilon — never an eyeballed literal) is
exactly the boundary between *"these two results are numerically identical"* and
*"they differ by orders of magnitude above the floor → a genuine, attributable
effect."* This is what lets the tool **adjudicate** — validate against the field's
oracles, compare method forks side-by-side (DoD cond. 1), and certify that a
difference is biology/method, not silicon. A loose magic-number tolerance
*dissolves that boundary* and turns validation into theater — a test that cannot
distinguish a correct implementation from a subtly-broken one is not validating
anything. This is why every numerical comparison is grounded in a typed,
self-describing tolerance (`crates/agreement`), and why grounding is a correctness
invariant (Inv. 2/3), not a matter of taste.

## Architecture: a self-contained core, a standard at the boundary

The system has a **hot core that must be bulletproof and self-contained** and a
**cold interoperability boundary that may be heavier.** Conflating the two is the
root architectural error; keeping them separate resolves the format, runtime, and
dependency questions.

- **Core (pure Rust, zero external runtime).** Acquisition → a lean, Rust-native,
  mutable **working format** (`.oisi`, HDF5 underneath) → the incremental Rust
  analysis pipeline → visualization. The daily loop needs nothing installed beyond
  the application itself: the vendor camera DLLs are **bundled**, no interpreter
  sits between captured frames and bytes on disk. This is why the native format is
  *not* NWB and is *not* written by an external tool — the capture-and-persist path
  of irreplaceable data must be the simplest, most robust thing in the system.
- **Boundary (the standard, via its reference tool).** Interoperability is achieved
  by *emitting the field standard* — **NWB / DANDI**, produced and validated by the
  **reference implementation** (pynwb / nwbinspector / dandi), the only way to
  guarantee a conformant file without re-implementing a standard. This is a
  deliberate, off-hot-path export; the machinery (including any Python) is
  **bundled and invisible** to the scientist. The native format is *not* the
  standard; it *exports to* the standard.
- **One schema source.** The `.oisi` layout is defined once, in Rust, and that
  single source feeds the Rust I/O layer, the schema contract doc, *and* the NWB
  exporter — no schema knowledge is hand-copied across modules or languages.
- **Cache is not science.** Tool-internal incremental state (Merkle fingerprints,
  stage intermediates) is separated from the canonical scientific record; ephemeral
  derived state never lives commingled with the data of record.

The successful standards in the field (SpikeGLX, Open Ephys, Suite2p, …) take this
exact shape — an excellent self-contained native pipeline that *exports* to NWB.
OpenISI is built to that pattern, not to "NWB as the native format."

### Platform, compute & reproducibility

- **CPU is the canonical backend.** The compute substrate (Burn) is runtime-dispatched
  over one `Backend` alias; the CPU (`ndarray`) backend is always compiled in and
  needs no GPU. **Published / reproducible results are *defined on the CPU backend*** —
  bit-identical on any machine, so any lab reproduces any result without special
  hardware. GPU (CUDA) is **optional acceleration** whose output must agree with the
  CPU result within a *characterized, gate-tested* bound; it is never required for
  correctness or reproducibility. Cross-vendor GPU (wgpu / Metal, for Mac/AMD
  acceleration) is a **trajectory**, not a gate.
- **Reach: analysis everywhere, acquisition where the hardware is.** Analysis and
  visualization are platform-neutral (pure Rust + static HDF5 + CPU) and **run on
  Windows / macOS / Linux**, so every lab can analyze, visualize, and export.
  *Acquisition* binds to Windows hardware APIs (DXGI vsync, QPC timing, the PCO SDK)
  and is **Windows-only today**; cross-platform acquisition is a **trajectory** pursued
  behind the hardware/platform abstraction (Invariant 18), not a current gate — a
  Mac/Linux lab can fully analyze and share, and acquisition portability follows the
  abstraction.

## Invariants

Grouped by concern; each is non-negotiable.

### Scientific correctness

1. **Faithfulness is inviolable.** For a fixed (config, input, backend), output is
   **bit-identical, forever.** Every change is a serialization/format/structure
   change that passes the equivalence gate and never moves the science. This is the
   backstop every other invariant defers to.
2. **Determinism is explicit; CPU is canonical.** Bit-identity is guaranteed *per
   backend*, and the **CPU (`ndarray`) backend is the canonical reference** for
   published/reproducible results — identical on any machine, no GPU required.
   Cross-backend (CPU↔GPU) floating-point ordering differences are **characterized
   and bounded** by the device-stability gate, never silently assumed zero. The
   contract states where identity holds and where drift is merely bounded.
3. **Validated against the field's oracles.** Correctness is proven against the
   field's own reference implementations (Allen `corticalmapping`, SNLC) and against
   synthetic ground truth — permanently, in CI. Trust is *earned by gate*, not
   asserted.
4. **Forward-compatible forever.** A standard's data outlives its software. The
   format carries a version; files written by any past version **migrate forward**
   to the current schema, always — proven by an end-to-end migration test, not just
   the presence of migration code.
5. **Reproducible by record.** Every analyzed file records exactly what produced it
   (typed analysis config, software version, device, acquisition provenance), so any
   result can be reproduced bit-identically from the file alone.

### The instrument

6. **Self-contained.** The core (acquisition + analysis) runs with **zero external
   runtime** — installer-bundled vendor DLLs, no `pip`, no PATH surgery, no venv.
   Any heavier dependency (the NWB export runtime) is bundled and confined to the
   boundary, invisible to the user.
7. **Data integrity is sacred.** Irreplaceable captured data is never lost or
   corrupted: writes are atomic (write-to-temp + rename), a crash mid-acquisition
   leaves a recoverable partial, and out-of-space / permission / disconnect failures
   are surfaced as typed errors, never silent loss.
8. **Real-time guarantees.** Acquisition meets its hard timing budget (frame
   delivery within spec; drops detected, characterized, and reported, never hidden);
   re-analysis is fast by construction (the demand-driven incremental cache).
9. **Concurrent by design, correct under load.** The real-time threads (camera,
   stimulus, analysis worker, UI) are race-free and bounded-latency, and cancel /
   shut down cleanly; a stalled or dead consumer never corrupts capture or hangs the
   instrument.

### Platform & scale

10. **Within every lab's reach.** CPU-only is fully supported — no GPU is ever
    required for correctness or reproducibility — and analysis + visualization run on
    Windows / macOS / Linux. (The full policy and the acquisition-portability
    trajectory are under *Architecture → Platform, compute & reproducibility*.)
11. **Scales to real data.** Multi-gigabyte camera movies are read and processed
    without exhausting memory (streamed / chunked I/O) on a commodity lab workstation.

### Interoperability

12. **By standard, not by adapter.** Data leaves the tool in a form the field already
    reads (NWB / DANDI), via the recognized standard *and its reference validator* —
    not a bespoke format with a hand-written checker. Config and provenance are
    self-describing, schema-bearing JSON.

### Engineering discipline

13. **Tool for infrastructure; hand-roll only the domain.** For any infrastructure
    responsibility — serialization, schema, validation-dispatch, reactivity,
    hashing, I/O, parsing, error machinery — use the established tool. Hand-roll
    *only* the logic that encodes this project's specific science/hardware (the
    predicate body, not the dispatch). Cargo-culting a mismatched tool is as much a
    failure as hand-rolling. The custom surface must equal the domain surface.
14. **Make invalid states unrepresentable where types can; validate where they
    can't.** The template is the tagged enum collapsing `active_when` into the
    variant structure. Prefer it wherever a type *correctly* expresses the invariant;
    use runtime validation (garde) only for what types genuinely cannot (dynamic
    hardware constraints). Sized by correctness, not by code cost in either
    direction.
15. **One source, one boundary — no divergent parallel systems.** Each concern has a
    single authority: one config/param system, one error framework, one schema
    source, one I/O boundary. Duplicated or partially-migrated systems are debt and
    are not allowed to persist. Every remaining custom island is named and justified
    in `TOOL_LEDGER.md`; zero tool-able code is the target, zero custom code is not.
16. **Pristine at every step.** No commit until pristine; commits land on `main` when
    asked; never push without asking. Each landed change is tested and clippy-clean
    workspace-wide.

### Stewardship & openness

17. **Legible to scientists, not just developers.** Errors carry stable
    machine-readable codes *and* messages a non-coder can act on; the UI is
    discoverable; failure is explained, not dumped. The tool assumes its user is a
    scientist.
18. **Extensible without forking; rig-agnostic by construction.** New analysis
    methods plug in through the typed stage enums; **every piece of rig hardware —
    camera, stimulus monitor, light source, DAQ/TTL, photodiode, any peripheral —
    plugs in through a platform/hardware abstraction.** OpenISI must run on **any
    lab's rig, on any OS, with any vendor's hardware** — not just the one it was
    developed on. The rig it is built against (e.g. a pco.panda camera, an Excelitas
    X-Cite light, an Arduino DAQ) is a **reference instance behind the abstraction,
    never an assumption baked into the format, analysis, or schema**: device
    identity is recorded as generic, self-describing metadata, not a vendor-specific
    layout. The community extends OpenISI in-tree, not by maintaining a fork.
19. **Documented as a deliverable.** A user guide, the `.oisi` + NWB format spec,
    install instructions, and API docs ship with the tool and stay current — a
    standard nobody can learn to use is not a standard.
20. **Observable in the field.** Structured logging, the multi-clock timing
    forensics, and clear failure reporting are sufficient to diagnose a problem in a
    remote lab you cannot reach directly.
21. **Versioned and supply-chain-hygienic.** Software and on-disk format follow
    semantic versioning with a deprecation/migration path; dependencies are audited
    (`cargo-audit`); the provenance of every bundled binary (the vendor DLLs) is
    documented and reproducible.
22. **Open by construction.** A permissive open-source license, a public contribution
    path, and transparent provenance/governance — the social preconditions for a tool
    to become *the* standard, not merely a good one.

## Definition of Done

"Done" is derived from the instrument's **purpose** — *a mouse-vision lab acquires
trustworthy ISI data and gets correct retinotopic maps it can reproduce and share, and
a second lab can do the same without the people who built it.* It is **not**
feature-completeness (an adoptable platform is never feature-complete) and **not**
conformance to any sub-community's checklist (NWB-standardization, no-hand-rolling
craft, SSoT purism). Those are means or capabilities; each earns a place only by
**tracing to the purpose**.

There are **three tiers**, because the binding constraint is not the feature set — it
is that this work passes from its original engineer to a **less-experienced
successor**. That splits the remaining work by *who can do it*:

1. **Ready to hand off** — everything that requires the original engineer is finished
   and locked behind the safety net; everything else is legible and standard-tooled
   enough for someone who isn't them to continue. *(The nearest target.)*
2. **Ready to publish** — the tool/methods paper's claims are correct, defensible to
   reviewers, and **reviewer-runnable**.
3. **Ready to be adopted by the field** — the full six conditions, as a *living,
   extensible, open* state, not a fixed endpoint.

### The six conditions  *(Tier 3 — field adoption)*

Each holds only at the *real-thing* level. Each names **who catches the cheat** and
**the bar that beats the incumbent assembly** (vendor capture + PsychoPy/PTB stimulus
+ inherited Allen/Garrett/Kalatsky analysis scripts), so it cannot retreat to a proxy.

1. **Correct science.** Maps faithful to each cited source, *and* recover known
   synthetic ground truth, *and* expert-confirmed on real data, *and* error
   characterized; assumptions + limits stated; defaults principled and sourced.
   *Sentinels:* physicist, both reviewers. *Rejected proxy:* "matches the oracle /
   internally consistent." *Beats the field by:* making the method forks explicit,
   cited, and validated side-by-side.
2. **Safe with irreplaceable data.** Nothing silent, nothing lost mid-session, the
   cache cannot corrupt the canonical record, every fault a surfaced typed error,
   atomic + crash-recoverable. *Sentinel:* experimentalist. *Proxy:* "the final write
   is atomic." *Beats:* capture-plus-glue that drops frames silently.
3. **Runs the lab's real workflow.** A multi-hour real experiment on a real rig,
   *repeatedly*; a non-developer at a second lab installs it self-contained and reaches
   a correct map; hardware-abstracted; a headless door beside the GUI. *Sentinels:*
   experimentalist, engineer, data scientist. *Proxy:* "it ran once." *Beats:* the
   three-tool assembly — owns the whole chain with **recorded** (not assumed) timing.
4. **Reproducible from the record.** The `.oisi` reproduces the result (config +
   version + calibration + realized schedule travel with it); determinism asserted at
   the scientifically-meaningful level; no hidden state; no stale-cache lies; whole-file
   version + forward migration. *Sentinels:* science reviewer, data scientist, PI.
   *Proxy:* "the equivalence test is green." *Beats:* scripts that carry no provenance.
5. **Trustworthy to the field.** Legible, organized, SSoT, right-tool-not-hand-rolled,
   DRY; validation visible and runnable; extensible through abstractions; shareable
   losslessly (NWB/DANDI); observable for remote diagnosis; open license + contribution
   path; **not bus-factor-1**. *(Every code-quality property lives here — earned,
   because each traces to "the field can trust and extend it.")* *Sentinels:*
   developer, PI, data scientist, tool reviewer. *Proxy:* "it works / passes
   nwbinspector." *Beats:* per-lab scripts + closed vendor systems on auditability and
   openness.
6. **Stays live across real use.** Re-entrant (no required restarts), responsive (no UI
   freeze, no work-in-locks), recoverable from transient faults, no resource creep,
   UI/backend never desynced. *Sentinel:* the experimentalist, all day. *Proxy:* "a
   single run succeeded." *Beats:* the assembly that needs babysitting between stages.

**Two spines run through all six:** **no silent failure** (the through-line of 1/2/4/6
— swallowed errors, undetected drops, stale hits, unflagged garbage-in break
correctness *and* integrity *and* reproducibility at once); and **trace-to-outcome** (a
property is a gate *iff* its failure traces to one of the six — this *includes*
SSoT/legibility/right-tool via condition 5 and *excludes* orthodoxy pursued for its own
sake).

**Self-check.** *All six met ⟺ all eight persona-sentinels satisfied ⟺ all field gaps
closed ⟺ every incumbent tool matched-or-superseded on its own strength.* If these four
ever disagree, the contract is wrong, not done.

### Tier 1 — Ready to hand off  *(the binding constraint; nearest target)*

The successor is less experienced; the ordering principle is **finish what requires
you, convert your judgment into the safety net, make everything else legible for
someone who isn't you.** The more senior-expertise something needs, the earlier it must
be done — the worst outcome is handing off a dangerous, half-finished core.

Must be **done by the original engineer before handoff** (cannot be delegated):
- [x] **Science core (cond. 1):** correctness of the existing methods + the machinery
  that proves it — `regression_oisi` bit-identical (`cargo test -p isi-analysis --test
  regression_oisi -- --include-ignored`), per-stage goldens, synthetic-ground-truth
  pipeline tests, the faithful-to-source golden net (`tests/golden/*`), pre-2026
  migration end-to-end tested.
- [ ] **Data-integrity core (cond. 2):** acquisition write path **atomic +
  crash-recoverable**; disk-full/disconnect surfaced as typed errors; tool-internal
  cache **separated** from the canonical record so it cannot corrupt it.
- [ ] **Lifecycle/concurrency core (cond. 6):** **re-entrant acquisition (no required
  restart between runs)**; race-free threads with graceful cancel/shutdown (a dead
  consumer never corrupts capture); no work-in-locks UI freeze.
- [ ] **I/O + error architecture (cond. 2/5):** one owned HDF5 boundary; HDF5 errors
  born once with `#[source]` preserved, uniform `thiserror`; the `.oisi` schema a single
  Rust source generating the contract doc + feeding the exporter's paths.
- [ ] **Determinism policy + whole-file version invariant (cond. 4):** documented and
  gate-tested (the *framework + the cross-backend bound asserted*, not the breadth of
  future migrations).

The successor must be able to **lean on these instead of the original engineer**:
- [x] **The safety net as replacement judgment** — bit-identical regression, goldens,
  equivalence, `cargo test --workspace` green, `cargo clippy --workspace --all-targets`
  zero-warnings, types that make illegal states unrepresentable.
- [x] **Standard, recognized tools — not bespoke mazes** (`TOOL_LEDGER.md` reconciled;
  typed serde/schemars/garde config; the `define_params!` DSL deleted) so the successor
  can lean on external docs + community, not on the author's private knowledge.
- [ ] **The *why*, externalized** — `PRINCIPLES.md`, `ARCHITECTURE.md`, method
  citations, the tool-ledger rationale, decision records — complete enough that the
  holder leaving does not erase it.

**Deferred to the successor** (additive, lower-risk, guarded by the net): breadth of
methods/hardware, installers, the NWB breadth, cross-platform, UI polish, adoption work.

**Handoff is done when** everything that requires the original engineer is finished and
locked behind tests, and everything else is legible, standard-tooled, and documented
enough to continue safely.

### Tier 2 — Ready to publish  *(tool / methods paper)*

The debut introduces OpenISI itself, so reviewers will **install and run it** —
self-containment + an install path are pulled *forward* from adoption into the
publication bar.
- [ ] **Cond. 1 at the reviewer's bar:** non-circular validation — faithful-to-source
  **and** synthetic ground-truth recovery **and** expert-confirmed on real maps **and**
  characterized error; methods cited; honest sourced defaults (no
  tune-until-it-looks-right).
- [ ] **Cond. 4:** the paper's results reproduce from the record; code + data available.
- [ ] **Cond. 3 at "reviewer-runnable":** a reviewer installs it self-contained (vendor
  DLLs bundled, **no venv-path dependency**; Python bundled + export-only) and reaches a
  result. *(Pulled forward because it is a tool paper.)*
- [ ] **Cond. 5 at "open + described + runnable":** public, open-licensed; methods,
  install, and the `.oisi`/NWB format documented enough to review and reproduce.
- [x] **Interoperable export proven** (validation + conformance): `ndx-openisi`
  embedded in every exported file; `export_nwb` passes `nwbinspector` clean + strict
  `pynwb.validate`; lossless round-trip vs **real** `write_oisi` output (`cargo test -p
  openisi --test nwb_export_e2e`); `dandi validate` local (upload out of scope).

**Not required to publish:** condition-6 polish beyond what a reviewer hits,
cross-platform breadth, hardware abstraction, signed multi-OS installers, governance.

### Tier 3 — Ready to be adopted by the field  *(the living standard)*

The six conditions in full, as a *living, extensible, open* state — second-lab
self-contained install, a **rig-hardware abstraction** for camera, display, light
source, and DAQ/TTL (not PCO/X-Cite/Arduino-hardcoded; Invariant 18), GPU-CPU
agreement gate-tested, multi-GB datasets streamed without OOM, a cross-platform analysis
CI matrix, **docs that ship and stay current**, **observability** sufficient to diagnose
a remote field failure, **semantic versioning** (software + on-disk format with a
migration path), a clean supply chain (`cargo-audit`), **signed self-contained per-OS
installers**, and license + contribution + governance. The successor and community drive
this; it is the threshold of a self-sustaining standard, not a completion.

### Continuously held (not finish-line boxes)
- [x] `TOOL_LEDGER.md` reconciles: every custom island named + justified; no tool-able
  code in the custom surface.
- [x] `cargo test --workspace` green; `cargo clippy --workspace --all-targets` **zero
  warnings**.
- [x] Typed serde/schemars/garde config; JSON config files; `define_params!` deleted;
  descriptor-driven UI golden-locked. *(These are how conditions 1 and 5 stay true over
  time, not gates we cross once.)*

## Status

The typed config/param system and the NWB export path (validation + conformance
scope) are built and validated; the science-correctness net (bit-identical regression,
goldens, synthetic ground truth) holds. This contract spans the **full field-standard
scope** — scientific correctness, the instrument, platform/compute/reproducibility,
interoperability, engineering discipline, and stewardship.

Recorded platform decisions: **CPU is canonical** for reproducible results (GPU is
optional acceleration within a bound); **analysis is cross-platform, acquisition is
Windows-only today** with cross-platform acquisition pursued behind the hardware
abstraction (trajectory, not a gate); **cross-vendor GPU (wgpu/Metal) is trajectory.**

**The active target is Tier 1 (Ready to hand off).** Because the work passes to a
less-experienced successor, the prioritization is governed by *who can do it*: the
senior-expertise core must be finished and locked behind the safety net before handoff
— the **data-integrity core** (atomic/crash-safe write, cache/record separation), the
**lifecycle core** (re-entrant acquisition — today a second run requires restarting the
program — and race-free cancel/shutdown), the **one owned I/O boundary +
source-preserved errors + Rust schema-SSoT**, and the **determinism policy + whole-file
version invariant**. Adoption-tier breadth (camera/display abstraction, bundled
installers, cross-platform, NWB breadth, observability, governance) is deferred to the
successor on top of that net. Pristine-only; nothing lands until its gates are green.
