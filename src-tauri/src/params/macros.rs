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
                        active_when: None,
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
    };

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

    // ── Typed setter arms ────────────────────────────────────────
    (@setter $variant:ident, Bool) => {
        paste::paste! {
            pub fn [<set_ $variant:snake>](&mut self, v: bool) -> Result<(), String> {
                self.set(ParamId::$variant, ParamValue::Bool(v))
            }
        }
    };
    (@setter $variant:ident, U16) => {
        paste::paste! {
            pub fn [<set_ $variant:snake>](&mut self, v: u16) -> Result<(), String> {
                self.set(ParamId::$variant, ParamValue::U16(v))
            }
        }
    };
    (@setter $variant:ident, U32) => {
        paste::paste! {
            pub fn [<set_ $variant:snake>](&mut self, v: u32) -> Result<(), String> {
                self.set(ParamId::$variant, ParamValue::U32(v))
            }
        }
    };
    (@setter $variant:ident, I32) => {
        paste::paste! {
            pub fn [<set_ $variant:snake>](&mut self, v: i32) -> Result<(), String> {
                self.set(ParamId::$variant, ParamValue::I32(v))
            }
        }
    };
    (@setter $variant:ident, Usize) => {
        paste::paste! {
            pub fn [<set_ $variant:snake>](&mut self, v: usize) -> Result<(), String> {
                self.set(ParamId::$variant, ParamValue::Usize(v))
            }
        }
    };
    (@setter $variant:ident, F64) => {
        paste::paste! {
            pub fn [<set_ $variant:snake>](&mut self, v: f64) -> Result<(), String> {
                self.set(ParamId::$variant, ParamValue::F64(v))
            }
        }
    };
    (@setter $variant:ident, String) => {
        paste::paste! {
            pub fn [<set_ $variant:snake>](&mut self, v: String) -> Result<(), String> {
                self.set(ParamId::$variant, ParamValue::String(v))
            }
        }
    };
    (@setter $variant:ident, StringVec) => {
        paste::paste! {
            pub fn [<set_ $variant:snake>](&mut self, v: Vec<String>) -> Result<(), String> {
                self.set(ParamId::$variant, ParamValue::StringVec(v))
            }
        }
    };
    (@setter $variant:ident, Envelope) => {
        paste::paste! {
            pub fn [<set_ $variant:snake>](&mut self, v: Envelope) -> Result<(), String> {
                self.set(ParamId::$variant, ParamValue::Envelope(v))
            }
        }
    };
    (@setter $variant:ident, Carrier) => {
        paste::paste! {
            pub fn [<set_ $variant:snake>](&mut self, v: Carrier) -> Result<(), String> {
                self.set(ParamId::$variant, ParamValue::Carrier(v))
            }
        }
    };
    (@setter $variant:ident, Projection) => {
        paste::paste! {
            pub fn [<set_ $variant:snake>](&mut self, v: Projection) -> Result<(), String> {
                self.set(ParamId::$variant, ParamValue::Projection(v))
            }
        }
    };
    (@setter $variant:ident, Structure) => {
        paste::paste! {
            pub fn [<set_ $variant:snake>](&mut self, v: Structure) -> Result<(), String> {
                self.set(ParamId::$variant, ParamValue::Structure(v))
            }
        }
    };
    (@setter $variant:ident, Order) => {
        paste::paste! {
            pub fn [<set_ $variant:snake>](&mut self, v: Order) -> Result<(), String> {
                self.set(ParamId::$variant, ParamValue::Order(v))
            }
        }
    };
}

// The macro is used via #[macro_use] on this module in mod.rs.
