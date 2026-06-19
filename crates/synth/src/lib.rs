//! Synthetic ground-truth retinotopy generator (**dev-only**).
//!
//! Generates `.oisi`-shaped recordings from a *known* retinotopy so the pipeline
//! can be validated for **correctness** (does it recover the truth?) rather than
//! only faithfulness-to-an-oracle. The forward model and its citations are in
//! [`docs/SYNTHETIC_VALIDATION.md`](../../../docs/SYNTHETIC_VALIDATION.md).
//!
//! Built bedrock-up (Phase A built):
//! 1. [`map`] — the analytic ground-truth map (complex-log / wedge-dipole).
//! 2. [`encode`] — the Kalatsky–Stryker forward encoder (truth → clean movie).
//! 3. [`realism`] — the realism layer (Phase A: hemodynamic HRF delay + sensor
//!    noise; Phase B: PSF, physiological lines, drift, vasculature). Richer than
//!    the pipeline's assumptions, so recovery tests measure robustness to model
//!    mismatch, not assumptions against themselves.
//! 4. [`acquire`] — recording assembly: four sweep epochs → a pipeline-ingestible
//!    [`acquire::Synthetic`]. The conversion to `isi_analysis::RawAcquisition` +
//!    the recover-and-compare correctness test live in `isi-analysis`'s dev tests
//!    (this crate stays an independent leaf). Determinism via [`rng`].
//!
//! Deferred (Phase B/C): the remaining realism knobs + the oracle-handoff adapters
//! + the stress battery (see `docs/SYNTHETIC_VALIDATION.md`).
//!
//! Nothing depends on this crate, so the app/release build never compiles it.

pub mod acquire;
pub mod encode;
pub mod map;
pub mod realism;
pub mod rng;
