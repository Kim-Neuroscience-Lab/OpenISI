//! Stage 3 — Visual Field Sign (VFS) computation.
//!
//! Computes the per-pixel sign map `s(x,y) ∈ [-1, +1]` from the smoothed
//! per-orientation position phasors. The sign is the local handedness of
//! the retinotopic map (V1+V2+... alternate sign at each border).

use crate::compute;

/// Method choice for computing the visual field sign.
///
/// Canonical type: [`openisi_params::config::analysis::VfsComputation`] (UNIFY);
/// compute behavior is attached via [`VfsComputationExt`].
pub use openisi_params::config::analysis::VfsComputation as VfsComputationMethod;

/// Compute behavior for the VFS-computation stage (extension trait).
pub trait VfsComputationExt {
    /// Compute VFS and the four phase gradients (magnification consumes them).
    fn apply(
        &self,
        azi_z_smoothed: &compute::Complex2,
        alt_z_smoothed: &compute::Complex2,
    ) -> (BurnTensor2, BurnTensor2, BurnTensor2, BurnTensor2, BurnTensor2);
}

impl VfsComputationExt for VfsComputationMethod {
    fn apply(
        &self,
        azi_z_smoothed: &compute::Complex2,
        alt_z_smoothed: &compute::Complex2,
    ) -> (
        BurnTensor2,
        BurnTensor2,
        BurnTensor2,
        BurnTensor2,
        BurnTensor2,
    ) {
        match self {
            Self::OpenIsiChainRulePhasorGradient => {
                let (d_azi_dx, d_azi_dy) = compute::phase_gradients(azi_z_smoothed);
                let (d_alt_dx, d_alt_dy) = compute::phase_gradients(alt_z_smoothed);
                let vfs = compute::compute_vfs(
                    d_azi_dx.clone(),
                    d_azi_dy.clone(),
                    d_alt_dx.clone(),
                    d_alt_dy.clone(),
                );
                (vfs, d_azi_dx, d_azi_dy, d_alt_dx, d_alt_dy)
            }
        }
    }
}

/// Shorthand for the Burn 2D tensor type used in this module's Burn port.
type BurnTensor2 = burn_tensor::Tensor<compute::Backend, 2>;
