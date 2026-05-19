//! Fallback compute path when `gpu` feature is disabled.
//!
//! Reports CPU-only backend. The actual computation stays in the
//! existing io.rs and math.rs ndarray code paths — this module
//! just provides the backend_info() function for consistency.

pub fn backend_info() -> String {
    "CPU (ndarray, no libtorch)".into()
}
