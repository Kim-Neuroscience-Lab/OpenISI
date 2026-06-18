//! Synthetic ground-truth retinotopy generator (**dev-only**).
//!
//! Generates `.oisi`-shaped recordings from a *known* retinotopy so the pipeline
//! can be validated for **correctness** (does it recover the truth?) rather than
//! only faithfulness-to-an-oracle. The forward model and its citations are in
//! [`docs/SYNTHETIC_VALIDATION.md`](../../../docs/SYNTHETIC_VALIDATION.md).
//!
//! Built bedrock-up:
//! 1. [`map`] — the analytic ground-truth map (complex-log / wedge-dipole). The
//!    primitive everything else consumes: cortex pixel → known visual position,
//!    field sign, and magnification.
//! 2. *(next)* the Kalatsky–Stryker forward encoder (truth → periodic movie).
//! 3. *(next)* the realism layer (delay, hemodynamic PSF, noise — richer than the
//!    pipeline's assumptions, to avoid circularity).
//! 4. *(next)* `.oisi` assembly + the recover-and-compare report.
//!
//! Nothing depends on this crate, so the app/release build never compiles it.

pub mod encode;
pub mod map;
