//! Unified (wire, label) projection for every config enum.
//!
//! Every config enum in this crate (and the re-exported
//! [`openisi_stimulus`] enums) derives `strum::Display` (display label)
//! and `strum::EnumIter` (variant iteration), and is serde-tagged with
//! `#[serde(rename_all = "snake_case")]` (wire format). [`enum_options`]
//! is the one place that combines those three derives into the
//! `(wire, label)` pairs the descriptor layer hands to the UI — so
//! there is no parallel string registry to drift.

use serde::Serialize;
use strum::IntoEnumIterator;

/// One enum variant projected into both the wire string the config
/// persists and the human-facing label the UI shows.
#[derive(Debug, Clone, Serialize)]
pub struct EnumOption {
    /// snake_case serde representation — byte-identical to what TOML
    /// and `.oisi` provenance store.
    pub value: String,
    /// Human-readable label from the variant's `strum(to_string = …)`
    /// attribute. What the UI puts inside `<option>…</option>`.
    pub label: String,
}

/// All variants of `T`, paired with their wire string and display
/// label, in declaration order.
///
/// `T` must derive `serde::Serialize` with the snake_case rename rule
/// (for the wire string), `strum::Display` (for the label, via the
/// per-variant `to_string` attribute), and `strum::EnumIter` (for
/// `T::iter`). Every config enum in this crate does.
pub fn enum_options<T>() -> Vec<EnumOption>
where
    T: IntoEnumIterator + std::fmt::Display + Serialize,
{
    T::iter()
        .map(|v| {
            let wire = serde_json::to_value(&v)
                .ok()
                .and_then(|j| j.as_str().map(String::from))
                .unwrap_or_default();
            EnumOption {
                value: wire,
                label: v.to_string(),
            }
        })
        .collect()
}
