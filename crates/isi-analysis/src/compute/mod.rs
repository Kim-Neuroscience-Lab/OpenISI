//! Tensor-based analysis compute backend.
//!
//! A single tensor substrate: **Burn**. Hardware dispatch is Burn's; the
//! backend is the single alias [`Backend`] (see `backend.rs`), which
//! is the one place the backend (ndarray → CUDA → WGPU) is chosen. Every
//! op is written generically over `Tensor<Backend, D>`, so switching the
//! backend changes no downstream code.
//!
//! The analysis ops (`compute_vfs`, `phase_gradients`, `gaussian_smooth`,
//! the DFT/SNR/reliability ops, …), the [`Complex2`] complex pair, the
//! ndarray↔tensor conversions, and the [`CycleAccumulator`] are all
//! re-exported flat from this module.

mod accumulator;
mod backend;
mod complex;
mod conversions;
#[cfg(test)]
mod golden_vfs;
mod ops;
pub mod projection;
pub mod responsiveness;

pub use accumulator::{CycleAccumulator, Direction};
pub use backend::{backend_info, device, device_tag, Backend};
pub use complex::Complex2;
pub use conversions::*;
pub use ops::*;
