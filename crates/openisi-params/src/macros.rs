//! The `define_params!` macro — generates ParamId, PARAM_DEFS, and typed accessors.

/// Define all parameters in a single invocation.
///
/// Syntax:
/// ```ignore
/// define_params! {
///     VariantName: type = default, "toml.path", PersistTarget, GroupId, "Label", "unit", Constraint;
///     ...
/// }
/// ```
///
/// Generates:
/// - `ParamId` enum with one variant per parameter
/// - `PARAM_DEFS` static array of `ParamDef`
/// - Typed getter/setter methods on `Registry`
/// - `ParamId::count()` method
macro_rules! define_params {
    (
        $(
            $variant:ident : $ty:ident = $default:expr,
            $toml_path:expr, $persist:ident, $group:ident,
            $label:expr, $unit:expr, $constraint:expr
            $(, active_when = $active_when:expr )?
        );+ $(;)?
    ) => {
        // ── ParamId enum ──────────────────────────────────────────────
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[repr(usize)]
        pub enum ParamId {
            $( $variant, )+
        }

        impl ParamId {
            pub const fn count() -> usize {
                // Count variants by listing them
                [$( ParamId::$variant, )+].len()
            }

            /// All param IDs in definition order.
            pub fn all() -> &'static [ParamId] {
                static ALL: &[ParamId] = &[
                    $( ParamId::$variant, )+
                ];
                ALL
            }
        }

        // ── PARAM_DEFS table (lazy-initialized because some defaults allocate) ─
        pub static PARAM_DEFS: std::sync::LazyLock<Vec<ParamDef>> = std::sync::LazyLock::new(|| {
            vec![
                $(
                    ParamDef {
                        id: ParamId::$variant,
                        label: $label,
                        unit: $unit,
                        group: GroupId::$group,
                        toml_path: $toml_path,
                        persist: PersistTarget::$persist,
                        default: define_params!(@default $ty $default),
                        constraint: $constraint,
                        active_when: define_params!(@active_when $($active_when)?),
                    },
                )+
            ]
        });

        // ── Typed getters on Registry ─────────────────────────────────
        impl Registry {
            $(
                define_params!(@getter $variant, $ty);
            )+

            $(
                define_params!(@setter $variant, $ty);
            )+
        }

        // ── Marker types + RegistryParam impls ────────────────────────
        // One zero-sized marker struct per entry, sealed via
        // `registry_param::__internal::Sealed`. The bridge in
        // `crates/isi-analysis/src/bridge.rs` names these markers in
        // method-enum constructor argument types so that bare literals
        // structurally cannot enter the pipeline.
        $(
            #[allow(non_camel_case_types)]
            pub struct $variant;
            impl $crate::registry_param::__internal::Sealed for $variant {}
            impl $crate::registry_param::RegistryParam for $variant {
                type Value = define_params!(@rust_type $ty);
                const ID: ParamId = ParamId::$variant;
                const TOML_PATH: &'static str = $toml_path;
                fn extract(value: &ParamValue) -> Self::Value {
                    define_params!(@extract $ty, value)
                }
            }
        )+
    };

    // ── Extract from ParamValue → concrete Rust type ──────────────
    (@extract Bool, $v:expr) => {
        match $v { ParamValue::Bool(x) => *x, _ => unreachable!() }
    };
    (@extract U16, $v:expr) => {
        match $v { ParamValue::U16(x) => *x, _ => unreachable!() }
    };
    (@extract U32, $v:expr) => {
        match $v { ParamValue::U32(x) => *x, _ => unreachable!() }
    };
    (@extract I32, $v:expr) => {
        match $v { ParamValue::I32(x) => *x, _ => unreachable!() }
    };
    (@extract Usize, $v:expr) => {
        match $v { ParamValue::Usize(x) => *x, _ => unreachable!() }
    };
    (@extract F64, $v:expr) => {
        match $v { ParamValue::F64(x) => *x, _ => unreachable!() }
    };
    (@extract String, $v:expr) => {
        match $v { ParamValue::String(x) => x.clone(), _ => unreachable!() }
    };
    (@extract StringVec, $v:expr) => {
        match $v { ParamValue::StringVec(x) => x.clone(), _ => unreachable!() }
    };
    (@extract Envelope, $v:expr) => {
        match $v { ParamValue::Envelope(x) => *x, _ => unreachable!() }
    };
    (@extract Carrier, $v:expr) => {
        match $v { ParamValue::Carrier(x) => *x, _ => unreachable!() }
    };
    (@extract Projection, $v:expr) => {
        match $v { ParamValue::Projection(x) => *x, _ => unreachable!() }
    };
    (@extract Structure, $v:expr) => {
        match $v { ParamValue::Structure(x) => *x, _ => unreachable!() }
    };
    (@extract Order, $v:expr) => {
        match $v { ParamValue::Order(x) => *x, _ => unreachable!() }
    };
    (@extract CycleCombineKind, $v:expr) => {
        match $v { ParamValue::CycleCombine(x) => *x, _ => unreachable!() }
    };
    (@extract PhaseSmoothingKind, $v:expr) => {
        match $v { ParamValue::PhaseSmoothing(x) => *x, _ => unreachable!() }
    };
    (@extract VfsComputationKind, $v:expr) => {
        match $v { ParamValue::VfsComputation(x) => *x, _ => unreachable!() }
    };
    (@extract SignMapSmoothingKind, $v:expr) => {
        match $v { ParamValue::SignMapSmoothing(x) => *x, _ => unreachable!() }
    };
    (@extract CortexSourceKind, $v:expr) => {
        match $v { ParamValue::CortexSource(x) => *x, _ => unreachable!() }
    };
    (@extract PatchThresholdKind, $v:expr) => {
        match $v { ParamValue::PatchThreshold(x) => *x, _ => unreachable!() }
    };
    (@extract PatchExtractionKind, $v:expr) => {
        match $v { ParamValue::PatchExtraction(x) => *x, _ => unreachable!() }
    };
    (@extract PatchRefinementKind, $v:expr) => {
        match $v { ParamValue::PatchRefinement(x) => *x, _ => unreachable!() }
    };
    (@extract QualityGateKind, $v:expr) => {
        match $v { ParamValue::QualityGate(x) => *x, _ => unreachable!() }
    };
    (@extract EccentricityKind, $v:expr) => {
        match $v { ParamValue::Eccentricity(x) => *x, _ => unreachable!() }
    };

    // ── Rust-type lookup (used to set `RegistryParam::Value`) ────
    (@rust_type Bool) => { bool };
    (@rust_type U16) => { u16 };
    (@rust_type U32) => { u32 };
    (@rust_type I32) => { i32 };
    (@rust_type Usize) => { usize };
    (@rust_type F64) => { f64 };
    (@rust_type String) => { String };
    (@rust_type StringVec) => { Vec<String> };
    (@rust_type Envelope) => { Envelope };
    (@rust_type Carrier) => { Carrier };
    (@rust_type Projection) => { Projection };
    (@rust_type Structure) => { Structure };
    (@rust_type Order) => { Order };
    (@rust_type CycleCombineKind) => { CycleCombineKind };
    (@rust_type PhaseSmoothingKind) => { PhaseSmoothingKind };
    (@rust_type VfsComputationKind) => { VfsComputationKind };
    (@rust_type SignMapSmoothingKind) => { SignMapSmoothingKind };
    (@rust_type CortexSourceKind) => { CortexSourceKind };
    (@rust_type PatchThresholdKind) => { PatchThresholdKind };
    (@rust_type PatchExtractionKind) => { PatchExtractionKind };
    (@rust_type PatchRefinementKind) => { PatchRefinementKind };
    (@rust_type QualityGateKind) => { QualityGateKind };
    (@rust_type EccentricityKind) => { EccentricityKind };

    // ── active_when arm ──────────────────────────────────────────
    // Empty (not provided) → None; provided → Some(fn pointer).
    (@active_when) => { None };
    (@active_when $f:expr) => { Some($f as fn(&Registry) -> bool) };

    // ── Default value constructors ────────────────────────────────
    (@default Bool $val:expr) => { ParamValue::Bool($val) };
    (@default U16 $val:expr) => { ParamValue::U16($val) };
    (@default U32 $val:expr) => { ParamValue::U32($val) };
    (@default I32 $val:expr) => { ParamValue::I32($val) };
    (@default Usize $val:expr) => { ParamValue::Usize($val) };
    (@default F64 $val:expr) => { ParamValue::F64($val) };
    (@default String $val:expr) => { ParamValue::String($val.to_string()) };
    (@default StringVec $val:expr) => { ParamValue::StringVec($val) };
    (@default Envelope $val:expr) => { ParamValue::Envelope($val) };
    (@default Carrier $val:expr) => { ParamValue::Carrier($val) };
    (@default Projection $val:expr) => { ParamValue::Projection($val) };
    (@default Structure $val:expr) => { ParamValue::Structure($val) };
    (@default Order $val:expr) => { ParamValue::Order($val) };
    (@default CycleCombineKind $val:expr) => { ParamValue::CycleCombine($val) };
    (@default PhaseSmoothingKind $val:expr) => { ParamValue::PhaseSmoothing($val) };
    (@default VfsComputationKind $val:expr) => { ParamValue::VfsComputation($val) };
    (@default SignMapSmoothingKind $val:expr) => { ParamValue::SignMapSmoothing($val) };
    (@default CortexSourceKind $val:expr) => { ParamValue::CortexSource($val) };
    (@default PatchThresholdKind $val:expr) => { ParamValue::PatchThreshold($val) };
    (@default PatchExtractionKind $val:expr) => { ParamValue::PatchExtraction($val) };
    (@default PatchRefinementKind $val:expr) => { ParamValue::PatchRefinement($val) };
    (@default QualityGateKind $val:expr) => { ParamValue::QualityGate($val) };
    (@default EccentricityKind $val:expr) => { ParamValue::Eccentricity($val) };

    // ── Typed getter arms ────────────────────────────────────────
    (@getter $variant:ident, Bool) => {
        paste::paste! {
            pub fn [<$variant:snake>](&self) -> bool {
                match &self.values[ParamId::$variant as usize] {
                    ParamValue::Bool(v) => *v,
                    _ => unreachable!(),
                }
            }
        }
    };
    (@getter $variant:ident, U16) => {
        paste::paste! {
            pub fn [<$variant:snake>](&self) -> u16 {
                match &self.values[ParamId::$variant as usize] {
                    ParamValue::U16(v) => *v,
                    _ => unreachable!(),
                }
            }
        }
    };
    (@getter $variant:ident, U32) => {
        paste::paste! {
            pub fn [<$variant:snake>](&self) -> u32 {
                match &self.values[ParamId::$variant as usize] {
                    ParamValue::U32(v) => *v,
                    _ => unreachable!(),
                }
            }
        }
    };
    (@getter $variant:ident, I32) => {
        paste::paste! {
            pub fn [<$variant:snake>](&self) -> i32 {
                match &self.values[ParamId::$variant as usize] {
                    ParamValue::I32(v) => *v,
                    _ => unreachable!(),
                }
            }
        }
    };
    (@getter $variant:ident, Usize) => {
        paste::paste! {
            pub fn [<$variant:snake>](&self) -> usize {
                match &self.values[ParamId::$variant as usize] {
                    ParamValue::Usize(v) => *v,
                    _ => unreachable!(),
                }
            }
        }
    };
    (@getter $variant:ident, F64) => {
        paste::paste! {
            pub fn [<$variant:snake>](&self) -> f64 {
                match &self.values[ParamId::$variant as usize] {
                    ParamValue::F64(v) => *v,
                    _ => unreachable!(),
                }
            }
        }
    };
    (@getter $variant:ident, String) => {
        paste::paste! {
            pub fn [<$variant:snake>](&self) -> &str {
                match &self.values[ParamId::$variant as usize] {
                    ParamValue::String(v) => v.as_str(),
                    _ => unreachable!(),
                }
            }
        }
    };
    (@getter $variant:ident, StringVec) => {
        paste::paste! {
            pub fn [<$variant:snake>](&self) -> &[String] {
                match &self.values[ParamId::$variant as usize] {
                    ParamValue::StringVec(v) => v.as_slice(),
                    _ => unreachable!(),
                }
            }
        }
    };
    (@getter $variant:ident, Envelope) => {
        paste::paste! {
            pub fn [<$variant:snake>](&self) -> Envelope {
                match &self.values[ParamId::$variant as usize] {
                    ParamValue::Envelope(v) => *v,
                    _ => unreachable!(),
                }
            }
        }
    };
    (@getter $variant:ident, Carrier) => {
        paste::paste! {
            pub fn [<$variant:snake>](&self) -> Carrier {
                match &self.values[ParamId::$variant as usize] {
                    ParamValue::Carrier(v) => *v,
                    _ => unreachable!(),
                }
            }
        }
    };
    (@getter $variant:ident, Projection) => {
        paste::paste! {
            pub fn [<$variant:snake>](&self) -> Projection {
                match &self.values[ParamId::$variant as usize] {
                    ParamValue::Projection(v) => *v,
                    _ => unreachable!(),
                }
            }
        }
    };
    (@getter $variant:ident, Structure) => {
        paste::paste! {
            pub fn [<$variant:snake>](&self) -> Structure {
                match &self.values[ParamId::$variant as usize] {
                    ParamValue::Structure(v) => *v,
                    _ => unreachable!(),
                }
            }
        }
    };
    (@getter $variant:ident, Order) => {
        paste::paste! {
            pub fn [<$variant:snake>](&self) -> Order {
                match &self.values[ParamId::$variant as usize] {
                    ParamValue::Order(v) => *v,
                    _ => unreachable!(),
                }
            }
        }
    };
    (@getter $variant:ident, CycleCombineKind) => {
        paste::paste! {
            pub fn [<$variant:snake>](&self) -> CycleCombineKind {
                match &self.values[ParamId::$variant as usize] {
                    ParamValue::CycleCombine(v) => *v,
                    _ => unreachable!(),
                }
            }
        }
    };
    (@getter $variant:ident, PhaseSmoothingKind) => {
        paste::paste! {
            pub fn [<$variant:snake>](&self) -> PhaseSmoothingKind {
                match &self.values[ParamId::$variant as usize] {
                    ParamValue::PhaseSmoothing(v) => *v,
                    _ => unreachable!(),
                }
            }
        }
    };
    (@getter $variant:ident, VfsComputationKind) => {
        paste::paste! {
            pub fn [<$variant:snake>](&self) -> VfsComputationKind {
                match &self.values[ParamId::$variant as usize] {
                    ParamValue::VfsComputation(v) => *v,
                    _ => unreachable!(),
                }
            }
        }
    };
    (@getter $variant:ident, SignMapSmoothingKind) => {
        paste::paste! {
            pub fn [<$variant:snake>](&self) -> SignMapSmoothingKind {
                match &self.values[ParamId::$variant as usize] {
                    ParamValue::SignMapSmoothing(v) => *v,
                    _ => unreachable!(),
                }
            }
        }
    };
    (@getter $variant:ident, CortexSourceKind) => {
        paste::paste! {
            pub fn [<$variant:snake>](&self) -> CortexSourceKind {
                match &self.values[ParamId::$variant as usize] {
                    ParamValue::CortexSource(v) => *v,
                    _ => unreachable!(),
                }
            }
        }
    };
    (@getter $variant:ident, PatchThresholdKind) => {
        paste::paste! {
            pub fn [<$variant:snake>](&self) -> PatchThresholdKind {
                match &self.values[ParamId::$variant as usize] {
                    ParamValue::PatchThreshold(v) => *v,
                    _ => unreachable!(),
                }
            }
        }
    };
    (@getter $variant:ident, PatchExtractionKind) => {
        paste::paste! {
            pub fn [<$variant:snake>](&self) -> PatchExtractionKind {
                match &self.values[ParamId::$variant as usize] {
                    ParamValue::PatchExtraction(v) => *v,
                    _ => unreachable!(),
                }
            }
        }
    };
    (@getter $variant:ident, PatchRefinementKind) => {
        paste::paste! {
            pub fn [<$variant:snake>](&self) -> PatchRefinementKind {
                match &self.values[ParamId::$variant as usize] {
                    ParamValue::PatchRefinement(v) => *v,
                    _ => unreachable!(),
                }
            }
        }
    };
    (@getter $variant:ident, QualityGateKind) => {
        paste::paste! {
            pub fn [<$variant:snake>](&self) -> QualityGateKind {
                match &self.values[ParamId::$variant as usize] {
                    ParamValue::QualityGate(v) => *v,
                    _ => unreachable!(),
                }
            }
        }
    };
    (@getter $variant:ident, EccentricityKind) => {
        paste::paste! {
            pub fn [<$variant:snake>](&self) -> EccentricityKind {
                match &self.values[ParamId::$variant as usize] {
                    ParamValue::Eccentricity(v) => *v,
                    _ => unreachable!(),
                }
            }
        }
    };

    // ── Typed setter arms ────────────────────────────────────────
    (@setter $variant:ident, Bool) => {
        paste::paste! {
            pub fn [<set_ $variant:snake>](&mut self, v: bool) -> crate::error::ParamsResult<()> {
                self.set(ParamId::$variant, ParamValue::Bool(v))
            }
        }
    };
    (@setter $variant:ident, U16) => {
        paste::paste! {
            pub fn [<set_ $variant:snake>](&mut self, v: u16) -> crate::error::ParamsResult<()> {
                self.set(ParamId::$variant, ParamValue::U16(v))
            }
        }
    };
    (@setter $variant:ident, U32) => {
        paste::paste! {
            pub fn [<set_ $variant:snake>](&mut self, v: u32) -> crate::error::ParamsResult<()> {
                self.set(ParamId::$variant, ParamValue::U32(v))
            }
        }
    };
    (@setter $variant:ident, I32) => {
        paste::paste! {
            pub fn [<set_ $variant:snake>](&mut self, v: i32) -> crate::error::ParamsResult<()> {
                self.set(ParamId::$variant, ParamValue::I32(v))
            }
        }
    };
    (@setter $variant:ident, Usize) => {
        paste::paste! {
            pub fn [<set_ $variant:snake>](&mut self, v: usize) -> crate::error::ParamsResult<()> {
                self.set(ParamId::$variant, ParamValue::Usize(v))
            }
        }
    };
    (@setter $variant:ident, F64) => {
        paste::paste! {
            pub fn [<set_ $variant:snake>](&mut self, v: f64) -> crate::error::ParamsResult<()> {
                self.set(ParamId::$variant, ParamValue::F64(v))
            }
        }
    };
    (@setter $variant:ident, String) => {
        paste::paste! {
            pub fn [<set_ $variant:snake>](&mut self, v: String) -> crate::error::ParamsResult<()> {
                self.set(ParamId::$variant, ParamValue::String(v))
            }
        }
    };
    (@setter $variant:ident, StringVec) => {
        paste::paste! {
            pub fn [<set_ $variant:snake>](&mut self, v: Vec<String>) -> crate::error::ParamsResult<()> {
                self.set(ParamId::$variant, ParamValue::StringVec(v))
            }
        }
    };
    (@setter $variant:ident, Envelope) => {
        paste::paste! {
            pub fn [<set_ $variant:snake>](&mut self, v: Envelope) -> crate::error::ParamsResult<()> {
                self.set(ParamId::$variant, ParamValue::Envelope(v))
            }
        }
    };
    (@setter $variant:ident, Carrier) => {
        paste::paste! {
            pub fn [<set_ $variant:snake>](&mut self, v: Carrier) -> crate::error::ParamsResult<()> {
                self.set(ParamId::$variant, ParamValue::Carrier(v))
            }
        }
    };
    (@setter $variant:ident, Projection) => {
        paste::paste! {
            pub fn [<set_ $variant:snake>](&mut self, v: Projection) -> crate::error::ParamsResult<()> {
                self.set(ParamId::$variant, ParamValue::Projection(v))
            }
        }
    };
    (@setter $variant:ident, Structure) => {
        paste::paste! {
            pub fn [<set_ $variant:snake>](&mut self, v: Structure) -> crate::error::ParamsResult<()> {
                self.set(ParamId::$variant, ParamValue::Structure(v))
            }
        }
    };
    (@setter $variant:ident, Order) => {
        paste::paste! {
            pub fn [<set_ $variant:snake>](&mut self, v: Order) -> crate::error::ParamsResult<()> {
                self.set(ParamId::$variant, ParamValue::Order(v))
            }
        }
    };
    (@setter $variant:ident, CycleCombineKind) => {
        paste::paste! {
            pub fn [<set_ $variant:snake>](&mut self, v: CycleCombineKind) -> crate::error::ParamsResult<()> {
                self.set(ParamId::$variant, ParamValue::CycleCombine(v))
            }
        }
    };
    (@setter $variant:ident, PhaseSmoothingKind) => {
        paste::paste! {
            pub fn [<set_ $variant:snake>](&mut self, v: PhaseSmoothingKind) -> crate::error::ParamsResult<()> {
                self.set(ParamId::$variant, ParamValue::PhaseSmoothing(v))
            }
        }
    };
    (@setter $variant:ident, VfsComputationKind) => {
        paste::paste! {
            pub fn [<set_ $variant:snake>](&mut self, v: VfsComputationKind) -> crate::error::ParamsResult<()> {
                self.set(ParamId::$variant, ParamValue::VfsComputation(v))
            }
        }
    };
    (@setter $variant:ident, SignMapSmoothingKind) => {
        paste::paste! {
            pub fn [<set_ $variant:snake>](&mut self, v: SignMapSmoothingKind) -> crate::error::ParamsResult<()> {
                self.set(ParamId::$variant, ParamValue::SignMapSmoothing(v))
            }
        }
    };
    (@setter $variant:ident, CortexSourceKind) => {
        paste::paste! {
            pub fn [<set_ $variant:snake>](&mut self, v: CortexSourceKind) -> crate::error::ParamsResult<()> {
                self.set(ParamId::$variant, ParamValue::CortexSource(v))
            }
        }
    };
    (@setter $variant:ident, PatchThresholdKind) => {
        paste::paste! {
            pub fn [<set_ $variant:snake>](&mut self, v: PatchThresholdKind) -> crate::error::ParamsResult<()> {
                self.set(ParamId::$variant, ParamValue::PatchThreshold(v))
            }
        }
    };
    (@setter $variant:ident, PatchExtractionKind) => {
        paste::paste! {
            pub fn [<set_ $variant:snake>](&mut self, v: PatchExtractionKind) -> crate::error::ParamsResult<()> {
                self.set(ParamId::$variant, ParamValue::PatchExtraction(v))
            }
        }
    };
    (@setter $variant:ident, PatchRefinementKind) => {
        paste::paste! {
            pub fn [<set_ $variant:snake>](&mut self, v: PatchRefinementKind) -> crate::error::ParamsResult<()> {
                self.set(ParamId::$variant, ParamValue::PatchRefinement(v))
            }
        }
    };
    (@setter $variant:ident, QualityGateKind) => {
        paste::paste! {
            pub fn [<set_ $variant:snake>](&mut self, v: QualityGateKind) -> crate::error::ParamsResult<()> {
                self.set(ParamId::$variant, ParamValue::QualityGate(v))
            }
        }
    };
    (@setter $variant:ident, EccentricityKind) => {
        paste::paste! {
            pub fn [<set_ $variant:snake>](&mut self, v: EccentricityKind) -> crate::error::ParamsResult<()> {
                self.set(ParamId::$variant, ParamValue::Eccentricity(v))
            }
        }
    };
}

// The macro is used via #[macro_use] on this module in mod.rs.
