//! Burn compute substrate — one backend type, runtime device selection.
//!
//! The whole analysis pipeline is written against a single backend type,
//! [`Backend`] = `burn_dispatch::Dispatch` — Burn's runtime multi-backend
//! layer (the PyTorch-style device model). Every op is `Tensor<Backend, D>`;
//! the actual compute device is a **runtime value** ([`Device`] =
//! `DispatchDevice`), not a compile-time type. So there is no `<B: Backend>`
//! generic plumbing, no per-device code paths, and no separate binary per
//! backend — one binary picks the device at runtime, like `tensor.to('cuda')`.
//!
//! ## What features control
//!
//! Cargo features choose which backends are *compiled in* (and thus
//! selectable at runtime), not which device runs. The `ndarray` (CPU)
//! backend is always present. `--features cuda` additionally compiles in
//! the CUDA backend, so a cuda-built binary can run on CPU *or* CUDA chosen
//! at runtime. A WGPU/Metal/Vulkan arm slots in the same way (one
//! additional feature + one `DispatchDevice` variant) with zero op changes.
//!
//! ## Device selection policy
//!
//! [`device`] returns the preferred device for this build: CUDA when
//! it was compiled in (this is the production GPU target), otherwise the
//! ndarray CPU device. Per the project's no-silent-fallback rule the choice
//! is explicit, not a "try GPU, swallow errors" probe. A future UI/CLI
//! device picker would pass a chosen `DispatchDevice` straight through —
//! the type already supports it.

use burn_dispatch::{Dispatch, DispatchDevice};

/// The single analysis backend: Burn's runtime dispatch backend. All ops
/// are written against `Tensor<Backend, D>`; the device is chosen at
/// runtime via `Device`.
pub type Backend = Dispatch;

/// The runtime device handle for [`Backend`]. A value, selected at runtime
/// — `DispatchDevice::Cuda(..)`, `DispatchDevice::NdArray(..)`, etc.
pub type Device = DispatchDevice;

/// The preferred compute device for this build (see module docs):
/// CUDA if compiled in, else ndarray CPU.
pub fn device() -> Device {
    #[cfg(feature = "cuda")]
    {
        DispatchDevice::Cuda(burn_cuda::CudaDevice::default())
    }
    #[cfg(not(feature = "cuda"))]
    {
        DispatchDevice::NdArray(burn_ndarray::NdArrayDevice::Cpu)
    }
}

/// Human-readable identifier of the active device, for the UI.
pub fn backend_info() -> String {
    match device() {
        #[cfg(feature = "cuda")]
        DispatchDevice::Cuda(_) => "CUDA (Burn dispatch)".to_string(),
        DispatchDevice::NdArray(_) => "CPU (Burn dispatch, ndarray)".to_string(),
        // `DispatchDevice` is `#[non_exhaustive]` across feature sets; any
        // other compiled-in backend reports generically rather than failing.
        #[allow(unreachable_patterns)]
        _ => "Burn dispatch (other device)".to_string(),
    }
}

/// Short, filename-safe device tag for dev-figure run labels.
pub fn device_tag() -> &'static str {
    match device() {
        #[cfg(feature = "cuda")]
        DispatchDevice::Cuda(_) => "cuda",
        DispatchDevice::NdArray(_) => "cpu",
        #[allow(unreachable_patterns)]
        _ => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use burn_tensor::{Tensor, TensorData};

    // Smoke test: the Dispatch backend creates a tensor on the
    // runtime-selected device, runs a real op, and reads it back. Proves
    // `Tensor<Dispatch, D>` works as the single backend type.
    #[test]
    fn burn_tensor_roundtrips_host_data() {
        let device = device();
        let input: Vec<f32> = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0];

        let t = Tensor::<Backend, 2>::from_data(TensorData::new(input.clone(), [2, 3]), &device);
        let out = t.add_scalar(10.0);
        let recovered: Vec<f32> = out.into_data().to_vec().expect("to_vec");

        let expected: Vec<f32> = input.iter().map(|&v| v + 10.0).collect();
        assert_eq!(
            recovered, expected,
            "Dispatch add_scalar round-trip mismatch"
        );
    }

    #[test]
    fn burn_matmul_produces_correct_result() {
        let device = device();
        let a = Tensor::<Backend, 2>::from_data(
            TensorData::new(vec![1.0f32, 2.0, 3.0, 4.0], [2, 2]),
            &device,
        );
        let id = Tensor::<Backend, 2>::from_data(
            TensorData::new(vec![1.0f32, 0.0, 0.0, 1.0], [2, 2]),
            &device,
        );
        let prod = a.clone().matmul(id);
        let recovered: Vec<f32> = prod.into_data().to_vec().expect("to_vec");
        assert_eq!(recovered, vec![1.0, 2.0, 3.0, 4.0]);
    }
}
