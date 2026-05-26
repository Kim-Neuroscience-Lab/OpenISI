//! Thin Tauri-side wrapper around the canonical params crate.
//!
//! The params SSoT lives in `crates/openisi-params/`. This module exists
//! purely to (a) re-export it for the rest of `src-tauri` to import as
//! `crate::params::*` (unchanged from before the crate split), (b) host
//! the Tauri-IPC command surface in `commands.rs`, and (c) provide the
//! Tauri-Emitter implementation of `ParamChangeObserver`.

pub use openisi_params::*;

pub mod commands;
pub mod observer;
