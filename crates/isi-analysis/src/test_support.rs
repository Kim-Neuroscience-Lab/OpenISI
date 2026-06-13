//! Shared helpers for the in-crate golden tests. Golden fixtures are raw
//! little-endian binary blobs (`*.bin`); this is the ONE place that decodes
//! them and the one comparison helper, so the byte layout and the
//! disagreement-count logic aren't re-derived in every `mod golden`.
//!
//! Gated `#[cfg(test)]` at the `mod` declaration in `lib.rs`, so none of this
//! ships in a release build.

use ndarray::Array2;

/// Decode a little-endian `f64` blob (row-major) into a flat `Vec`.
pub(crate) fn load_f64(bytes: &[u8]) -> Vec<f64> {
    bytes
        .chunks_exact(8)
        .map(|c| f64::from_le_bytes(c.try_into().unwrap()))
        .collect()
}

/// Decode a little-endian `f32` blob (row-major) into a flat `Vec`.
pub(crate) fn load_f32(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
        .collect()
}

/// Decode a little-endian `i32` blob (row-major) into a flat `Vec`.
pub(crate) fn load_i32(bytes: &[u8]) -> Vec<i32> {
    bytes
        .chunks_exact(4)
        .map(|c| i32::from_le_bytes(c.try_into().unwrap()))
        .collect()
}

/// Count pixels where a bool result array disagrees with a `u8` golden mask
/// (row-major, `0`/`1`). Uses the array's own dims — no stride argument.
pub(crate) fn count_differing(ours: &Array2<bool>, golden: &[u8]) -> usize {
    let (h, w) = ours.dim();
    let mut d = 0usize;
    for r in 0..h {
        for c in 0..w {
            if (ours[[r, c]] as u8) != golden[r * w + c] {
                d += 1;
            }
        }
    }
    d
}
