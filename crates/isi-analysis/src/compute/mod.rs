//! GPU-accelerated (or CPU-optimized) compute backend using libtorch.
//!
//! When the `gpu` feature is enabled and CUDA is available, all heavy
//! operations run on GPU. Otherwise, libtorch's CPU backend provides
//! MKL/OpenBLAS-accelerated multi-threaded computation.
//!
//! When the `gpu` feature is disabled, falls back to the original
//! ndarray-based sequential implementation.

#[cfg(feature = "gpu")]
mod torch_ops;

#[cfg(feature = "gpu")]
pub use torch_ops::*;

// When gpu feature is disabled, provide stub functions that call the
// original ndarray implementations.
#[cfg(not(feature = "gpu"))]
mod fallback;

#[cfg(not(feature = "gpu"))]
pub use fallback::*;
