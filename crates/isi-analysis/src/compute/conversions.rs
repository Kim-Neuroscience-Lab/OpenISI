//! Conversions between host `ndarray` and on-device `tch::Tensor`.
//!
//! The analysis pipeline keeps raw frames on the host (as `Array3<u16>` from
//! HDF5) and only uploads the data it actually needs each step — per
//! `docs/ANALYSIS_COMPUTE.md` Principle 4 (bounded device memory). These
//! helpers are the only place ndarray↔tensor conversion happens.
//!
//! All on-device tensors use `f32` (Kind::Float) for real values and
//! `Kind::ComplexFloat` for complex values.

use ndarray::{Array2, Array3};
use num_complex::Complex64;
use tch::{Device, Kind, Tensor};

use super::device;
use crate::{AnalysisError, Result};

/// Upload a subset of u16 camera frames as an `f32` tensor on the active
/// device, in the order given by `indices`. The full frame stack stays in
/// host memory; only the indexed subset is uploaded.
///
/// Output shape: `[indices.len(), H, W]`, `Kind::Float`, on `device()`.
pub fn frames_u16_subset_to_tensor_f32(
    frames: &Array3<u16>,
    indices: &[usize],
) -> Tensor {
    let (_, h, w) = frames.dim();
    let n = indices.len();
    let plane = h * w;

    let mut flat = vec![0.0f32; n * plane];
    for (out_i, &src_i) in indices.iter().enumerate() {
        let src_plane = frames.slice(ndarray::s![src_i, .., ..]);
        let dst_start = out_i * plane;
        for (px_i, &v) in src_plane.iter().enumerate() {
            flat[dst_start + px_i] = v as f32;
        }
    }

    Tensor::from_slice(&flat)
        .reshape([n as i64, h as i64, w as i64])
        .to(device())
}

/// Download a 2D tensor back to a host `Array2<f64>`. Works regardless of
/// the tensor's device or kind — internally moves to CPU and promotes to
/// `f64` so downstream f64-based ndarray code (segmentation, derived maps,
/// HDF5 write) sees uniform precision.
///
/// Returns `AnalysisError::Compute` if the input isn't 2D or the tch /
/// ndarray conversion fails. Both are "shouldn't happen by construction"
/// upstream invariant violations, but expressing them as errors lets the
/// caller surface a clean message to the scientist instead of a panic.
pub fn tensor_to_array2_f64(t: &Tensor) -> Result<Array2<f64>> {
    let t_cpu = t.to_device(Device::Cpu).to_kind(Kind::Double);
    let size = t_cpu.size();
    if size.len() != 2 {
        return Err(AnalysisError::Compute(format!(
            "tensor_to_array2_f64 expects a 2D tensor, got shape {size:?}"
        )));
    }
    let (h, w) = (size[0] as usize, size[1] as usize);
    // `Vec::try_from(&Tensor)` only accepts 1D tensors; flatten first.
    let flat = t_cpu.flatten(0, -1);
    let data: Vec<f64> = Vec::try_from(&flat)
        .map_err(|e| AnalysisError::Compute(format!("tensor → Vec<f64>: {e}")))?;
    Array2::from_shape_vec((h, w), data)
        .map_err(|e| AnalysisError::Compute(format!("shape mismatch in tensor_to_array2_f64: {e}")))
}

/// Download a 2D complex tensor back to a host `Array2<Complex64>`. Accepts
/// `Kind::ComplexFloat` or `Kind::ComplexDouble`; both are extracted via
/// `.real()` / `.imag()` and promoted to `f64` for the host representation.
pub fn complex_tensor_to_array2(t: &Tensor) -> Result<Array2<Complex64>> {
    let kind = t.kind();
    if !matches!(kind, Kind::ComplexFloat | Kind::ComplexDouble) {
        return Err(AnalysisError::Compute(format!(
            "complex_tensor_to_array2 expects a complex tensor, got {kind:?}"
        )));
    }
    let re = tensor_to_array2_f64(&t.real())?;
    let im = tensor_to_array2_f64(&t.imag())?;
    let (h, w) = re.dim();
    Ok(Array2::from_shape_fn((h, w), |(r, c)| Complex64::new(re[[r, c]], im[[r, c]])))
}

/// Upload an `Array2<Complex64>` to the active device as a
/// `Kind::ComplexFloat` tensor. This is the canonical representation for
/// complex maps in the on-device pipeline; see `docs/ANALYSIS_COMPUTE.md`
/// Principle 7.
pub fn array2_complex_to_complex_tensor(arr: &Array2<Complex64>) -> Tensor {
    let (h, w) = arr.dim();
    let re: Vec<f32> = arr.iter().map(|z| z.re as f32).collect();
    let im: Vec<f32> = arr.iter().map(|z| z.im as f32).collect();
    let shape = [h as i64, w as i64];
    let re_t = Tensor::from_slice(&re).reshape(shape).to(super::device());
    let im_t = Tensor::from_slice(&im).reshape(shape).to(super::device());
    Tensor::complex(&re_t, &im_t)
}
