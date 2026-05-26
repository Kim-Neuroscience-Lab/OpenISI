//! Tensor-based analysis compute backend.
//!
//! Runs on whichever hardware libtorch makes available — CUDA on NVIDIA,
//! Metal on Apple Silicon, otherwise CPU. There is one implementation of
//! every analysis operation; hardware dispatch is libtorch's job.
//!
//! See `docs/ANALYSIS_COMPUTE.md`.

mod ops;
mod conversions;
mod accumulator;

pub use ops::*;
pub use conversions::*;
pub use accumulator::{CycleAccumulator, Direction};
