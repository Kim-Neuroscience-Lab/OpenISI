//! `RegistryParam` sealed trait + `Tagged<P>` newtype — structural
//! enforcement that every value entering the analysis pipeline came
//! from the registry, not a literal smuggled into code.
//!
//! Each `define_params!` entry emits a zero-sized marker struct and a
//! `RegistryParam` impl for it. The trait is sealed (only the macro
//! can implement it), so no third party can fabricate a marker.
//!
//! `Tagged<P>` is the typed value wrapper. Its constructor is crate-
//! private; the only producer is `RegistrySnapshot::get<P>`, which
//! reads `values[P::ID as usize]` and downcasts to `P::Value`.
//!
//! Method enum constructors in `crates/isi-analysis` take `Tagged<P>`
//! arguments with a specific `P` per slot. A bare literal cannot
//! construct a `Tagged<P>`, so analysis-pipeline values structurally
//! cannot come from anywhere except the registry. See the crate-root
//! doc comment in `lib.rs` for the full chain of guarantees.

use std::marker::PhantomData;

use super::ParamId;

/// Sealed marker — the macro implements it for every parameter; no
/// outside code can.
mod sealed {
    pub trait Sealed {}
}

/// Implemented once per parameter by the `define_params!` macro.
/// `Value` is the Rust type the registry stores for this param;
/// `ID` is its `ParamId` slot.
pub trait RegistryParam: sealed::Sealed + 'static {
    /// The concrete Rust type the registry stores for this parameter
    /// (e.g. `f64`, `i32`, `usize`, `SignMapSmoothingKind`).
    type Value: Clone + 'static;

    /// The `ParamId` slot this marker designates. The macro guarantees
    /// `ID`'s `ParamValue` variant carries a `Value` payload.
    const ID: ParamId;

    /// Dotted TOML path (e.g. `"sign_map_smoothing.gaussian.sigma_um"`).
    /// Mirrors the `toml_path` of the matching `define_params!` entry.
    const TOML_PATH: &'static str;

    /// Downcast a `ParamValue` (from a snapshot or the live Registry)
    /// to this parameter's `Value` type. The macro emits a per-entry
    /// impl that pattern-matches on exactly the `ParamValue` variant
    /// that the entry's `$ty` declares; mismatches are `unreachable!`
    /// — a macro invariant violation, not a runtime data hazard.
    fn extract(value: &super::ParamValue) -> Self::Value;
}

/// A typed parameter value. Constructor is crate-private; the only
/// producer is `RegistrySnapshot::get::<P>` (or its variants). A bare
/// numeric/string literal cannot construct a `Tagged<P>`.
#[repr(transparent)]
pub struct Tagged<P: RegistryParam>(P::Value, PhantomData<P>);

impl<P: RegistryParam> Tagged<P> {
    /// Crate-internal constructor. The only callers live inside
    /// `openisi-params` (`RegistrySnapshot::get` and friends).
    pub(crate) fn new(value: P::Value) -> Self {
        Self(value, PhantomData)
    }

    /// Unwrap to the inner value. The caller (a method enum
    /// constructor) discards the marker after type-checking has
    /// confirmed the value came from the correct registry slot.
    pub fn into_inner(self) -> P::Value {
        self.0
    }
}

impl<P: RegistryParam> std::fmt::Debug for Tagged<P>
where
    P::Value: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Tagged").field(&self.0).finish()
    }
}

/// Macro-only entry point for the sealed trait. The `define_params!`
/// macro invokes this in `@registry_param` arms to emit `Sealed` and
/// `RegistryParam` impls without exposing the sealing module.
#[doc(hidden)]
pub mod __internal {
    pub use super::sealed::Sealed;
}
