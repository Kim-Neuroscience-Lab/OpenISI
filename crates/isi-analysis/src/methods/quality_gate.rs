//! Stage 9 — Per-pixel quality gate.
//!
//! Optional per-pixel quality mask. Currently only `None`. Allen
//! `retinotopic_mapping` does not apply a per-pixel gate in its
//! published segmentation (`RetinotopicMapping.py` L1076-1078).

use ndarray::Array2;

/// Method choice for the per-pixel quality gate.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum QualityGateMethod {
    /// No per-pixel quality gate. Matches Allen `retinotopic_mapping`'s
    /// actual published behavior — the sign-map threshold + cortex
    /// envelope do all the gating.
    None,
}

impl QualityGateMethod {
    pub fn none() -> Self {
        Self::None
    }

    /// Apply the gate. Returns an all-true mask for `None`.
    /// The orchestrator ANDs this mask into the threshold mask.
    pub fn apply(&self, shape: (usize, usize)) -> Array2<bool> {
        match self {
            Self::None => Array2::from_elem(shape, true),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_returns_all_true() {
        let mask = QualityGateMethod::None.apply((4, 4));
        assert_eq!(mask.dim(), (4, 4));
        assert!(mask.iter().all(|&b| b));
    }
}
