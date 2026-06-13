//! Stage 3 — Visual Field Sign (VFS) computation.
//!
//! Computes the per-pixel sign map `s(x,y) ∈ [-1, +1]` from the smoothed
//! per-orientation position phasors. The sign is the local handedness of
//! the retinotopic map (V1+V2+... alternate sign at each border).

use crate::compute;

/// Method choice for computing the visual field sign.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum VfsComputationMethod {
    /// OpenISI chain-rule phasor gradient. Computes phase gradients via
    /// the chain rule on the smoothed phasor `z = c + i·s`:
    /// `∂φ/∂x = (c·∂s/∂x − s·∂c/∂x) / |z|²`,
    /// then VFS = `sin(θ_alt − θ_azi)` where
    /// `θ = atan2(∂φ/∂y, ∂φ/∂x)`. Mathematically equivalent to Allen
    /// `visualSignMap` (`RetinotopicMapping.py` L113–147) but more
    /// numerically stable near phase wraps.
    OpenIsiChainRulePhasorGradient,
}

impl VfsComputationMethod {
    pub fn open_isi_chain_rule_phasor_gradient() -> Self {
        Self::OpenIsiChainRulePhasorGradient
    }

    /// Compute VFS and the four phase gradients (magnification consumes
    /// them).
    pub fn apply(
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
