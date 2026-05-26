//! Tag-only method-choice enums for each analysis stage.
//!
//! These are the Registry-side representation of "which method variant
//! is selected for this stage." They mirror the tagged-enum variant
//! names of the corresponding method enums in
//! `crates/isi-analysis/src/methods/*.rs`, but carry no tunable data —
//! the per-variant tunables are separate Registry params with
//! `active_when` predicates keyed to these choice values.
//!
//! Naming + `serde(rename_all = "snake_case")` produces the same wire
//! strings the analysis crate's tagged enums use (`"gaussian"`,
//! `"snlc_garrett2014_im_bound"`, etc.), so the registry tree written
//! to `.oisi /analysis_params` is consistent with the legacy method
//! tag values.

use serde::{Deserialize, Serialize};

macro_rules! kind_enum {
    ($name:ident { $($variant:ident),+ $(,)? } default = $default_variant:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            $( $variant, )+
        }

        impl Default for $name {
            fn default() -> Self { Self::$default_variant }
        }
    };
}

kind_enum!(CycleCombineKind {
    MarshelGarrett2011DelaySubtraction,
    KalatskyStryker2003RawAverage,
} default = MarshelGarrett2011DelaySubtraction);

kind_enum!(PhaseSmoothingKind {
    OpenIsiAmpWeightedPhasor,
} default = OpenIsiAmpWeightedPhasor);

kind_enum!(VfsComputationKind {
    OpenIsiChainRulePhasorGradient,
} default = OpenIsiChainRulePhasorGradient);

kind_enum!(SignMapSmoothingKind {
    Gaussian,
} default = Gaussian);

kind_enum!(CortexSourceKind {
    Reliability,
    UserPolygon,
    SnlcGarrett2014ImBound,
    AllenZhuang2017FullFrame,
} default = SnlcGarrett2014ImBound);

kind_enum!(PatchThresholdKind {
    AllenZhuang2017FixedSignMapThr,
    Garrett2014SigmaScaled,
} default = Garrett2014SigmaScaled);

kind_enum!(PatchExtractionKind {
    AllenZhuang2017LabelOpenCloseDilate,
} default = AllenZhuang2017LabelOpenCloseDilate);

kind_enum!(PatchRefinementKind {
    None,
    AllenZhuang2017SplitMerge,
} default = AllenZhuang2017SplitMerge);

kind_enum!(QualityGateKind {
    None,
} default = None);

kind_enum!(EccentricityKind {
    Garrett2014WholeCortexV1,
} default = Garrett2014WholeCortexV1);
