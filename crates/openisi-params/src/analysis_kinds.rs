//! Tag-only method-choice enums for each analysis stage.
//!
//! These are the **UI-dropdown** representation of "which method variant is
//! selected for this stage": the descriptor layer projects them (serde wire
//! string + strum label + `EnumIter`) into the `<option>` lists. They mirror the
//! variant names of the tagged method enums in `config::analysis` (which ARE the
//! `crates/isi-analysis` method types), but carry no tunable data — the per-variant
//! tunables live on the tagged enum variants themselves.
//!
//! ## Three things derive automatically per variant
//!
//! 1. **Wire format** — `#[serde(rename_all = "snake_case")]` on the enum
//!    yields the snake_case string written to TOML and `.oisi` provenance.
//! 2. **Display label** — `#[strum(to_string = "…")]` on each variant
//!    produces the human-facing label used in UI dropdowns. Both come
//!    from the same single declaration here, so the wire string and the
//!    UI label can never drift apart.
//! 3. **Variant iteration** — `strum::EnumIter` lets the descriptor
//!    layer enumerate every variant of a given enum to populate
//!    `<option>` lists without a parallel string registry.

use serde::{Deserialize, Serialize};

macro_rules! kind_enum {
    ($name:ident {
        $( $(#[$variant_attr:meta])* $variant:ident => $label:literal ),+
        $(,)?
    } default = $default_variant:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, strum::Display, strum::EnumIter)]
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            $(
                $(#[$variant_attr])*
                #[strum(to_string = $label)]
                $variant,
            )+
        }

        impl Default for $name {
            fn default() -> Self { Self::$default_variant }
        }
    };
}

kind_enum!(BaselineKind {
    // Allen `ImageAnalysis.normalizeMovie` baseline: per-pixel temporal
    // mean/median over ALL frames (the stimulus sweeps included). The faithful
    // oracle path. `mean` is `np.mean(movie, axis=0)`; `median` is
    // `np.median(movie, axis=0)`.
    AllenAllFrameMean => "All-frame mean (Allen normalizeMovie)",
    AllenAllFrameMedian => "All-frame median (Allen normalizeMovie)",
    // OpenISI inter-sweep baseline: F0 from only the rest frames (before the
    // first sweep + the inter-sweep gaps), so stimulus-driven activity does not
    // contaminate the ΔF/F denominator. The more principled resting F0; falls
    // back to the all-frame mean when a schedule has no rest gaps.
    OpenIsiInterSweepMean => "Inter-sweep rest mean (OpenISI)",
    OpenIsiInterSweepMedian => "Inter-sweep rest median (OpenISI)",
} default = OpenIsiInterSweepMean);

kind_enum!(ResponseNormalizationKind {
    // Fractional ΔF/F: (F−F0)/max(F0,floor). OpenISI default; F1 magnitude
    // carries a per-pixel 1/F0 weighting the oracles don't apply.
    OpenIsiFractionalDff => "Fractional ΔF/F (OpenISI)",
    // Absolute response F−F0, no division — the oracle-faithful F1 amplitude
    // (SNLC Gf1image.m / Allen generatePhaseMap2). Phase identical to ΔF/F.
    OracleAbsoluteDeltaF => "Absolute ΔF (SNLC / Allen F1)",
} default = OpenIsiFractionalDff);

kind_enum!(CycleAverageKind {
    // Plain complex average of the per-cycle maps — faithful to Allen
    // `get_average_movie` + single DFT (the per-cycle DFT kernel is identical
    // across cycles, so averaging the complex maps equals DFT-ing the averaged
    // movie) and SNLC `Gf1image` accumulation. The validated default.
    SimpleComplexAverage => "Simple complex average (Allen / SNLC)",
    // OpenISI deviation no oracle performs: phase-lock each cycle to the consensus
    // global phase before averaging. Kept as an explicit option.
    PhaseLockedAverage => "Phase-locked average (OpenISI)",
} default = SimpleComplexAverage);

kind_enum!(RectificationKind {
    // No rectification — Allen isRectify=False (validated default).
    None => "None (Allen isRectify=False)",
    // Half-wave rectify: clip negatives to zero before the DFT —
    // Allen isRectify=True (HighLevel.getMappingMovies).
    AllenZhuang2017ClipNegative => "Clip-negative rectify (Allen isRectify=True)",
} default = None);

kind_enum!(DirectionSmoothingKind {
    // No pre-combine smoothing — OpenISI smooths the combined phasor post-combine
    // (PhaseSmoothing). The validated default.
    None => "None (post-combine smoothing)",
    // SNLC adaptiveSmoother.m (Wiener-type), per-direction pre-combine.
    SnlcAdaptiveSmoother => "Adaptive smoother (SNLC Gprocesskret)",
} default = None);

kind_enum!(CycleCombineKind {
    // Per-cycle delay subtraction `(φ_fwd − φ_rev) / 2` is Kalatsky &
    // Stryker 2003's original contribution, designed to cancel
    // hemodynamic delay. Marshel 2011 / Garrett 2014 inherit and use it
    // — they do not introduce it. Naming attributes the technique to
    // its actual author.
    KalatskyStryker2003DelaySubtraction => "Delay subtraction (Kalatsky & Stryker 2003)",
    // No delay correction — direction maps are computed independently
    // per direction without phase cancellation. Not a published method
    // (Kalatsky's whole point was that raw averaging is wrong); kept
    // as a fallback for debugging / when delay subtraction is somehow
    // unwanted.
    UnweightedCycleAverage => "Unweighted cycle average (no delay correction)",
} default = KalatskyStryker2003DelaySubtraction);

kind_enum!(PhaseSmoothingKind {
    // Amplitude-weighted complex-phasor smoothing — phase-equivalent to SNLC
    // `Gprocesskret.m`, which smooths the complex F1 map (`amp·e^{iφ}`) directly.
    // (Our normalized-convolution magnitude is an OpenISI refinement that does not
    // change the phase.)
    SnlcAmpWeightedPhasor => "Amplitude-weighted phasor (SNLC)",
    // Unweighted scalar Gaussian on the position/phase map — Allen
    // `RetinotopicMapping.py::_getSignMap` (`gaussian_filter(positionMap, sigma)`).
    AllenZhuang2017PositionGaussian => "Position Gaussian (Allen / Zhuang 2017)",
} default = SnlcAmpWeightedPhasor);

kind_enum!(VfsComputationKind {
    OpenIsiChainRulePhasorGradient => "Chain-rule phasor gradient (OpenISI)",
} default = OpenIsiChainRulePhasorGradient);

kind_enum!(SignMapSmoothingKind {
    Gaussian => "Gaussian",
} default = Gaussian);

kind_enum!(CortexSourceKind {
    Reliability => "Reliability mask",
    UserPolygon => "User polygon",
    SnlcGarrett2014ImBound => "SNLC ImBound (Garrett 2014)",
    // SNLC response-magnitude ROI gate (overlaymaps.m): normalized mag^1.1 ≥ thr.
    SnlcMagThreshold => "SNLC magnitude threshold (overlaymaps)",
    // No cortex restriction — analysis runs over the full frame.
    // Allen/Zhuang did not introduce a "full-frame" cortex source as a
    // distinct method; their pipeline just omitted the restriction.
    // Named for what it does rather than misattributed to a paper.
    NoRestriction => "No restriction (full frame)",
} default = SnlcGarrett2014ImBound);

kind_enum!(PatchThresholdKind {
    AllenZhuang2017FixedSignMapThr => "Fixed sign-map threshold (Allen / Zhuang 2017)",
    Garrett2014SigmaScaled => "Sigma-scaled (Garrett 2014)",
} default = Garrett2014SigmaScaled);

kind_enum!(PatchExtractionKind {
    AllenZhuang2017LabelOpenCloseDilate => "Label + open/close/dilate (Allen / Zhuang 2017)",
} default = AllenZhuang2017LabelOpenCloseDilate);

kind_enum!(PatchRefinementKind {
    None => "None",
    AllenZhuang2017SplitMerge => "Split/merge refinement (Allen / Zhuang 2017)",
} default = AllenZhuang2017SplitMerge);

kind_enum!(EccentricityKind {
    OpenIsiWholeCortexV1 => "Whole-cortex V1, mean center (OpenISI)",
    SnlcGetAreaBordersV1Center => "V1 center, pixel-CoM sample (SNLC / getAreaBorders)",
} default = OpenIsiWholeCortexV1);
