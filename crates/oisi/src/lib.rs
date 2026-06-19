//! The `.oisi` recording file format.
//!
//! `.oisi` is OpenISI's single persisted format (HDF5 internally, NWB/DANDI-aligned
//! via the export bridge). This crate owns the format end-to-end — schema, HDF5
//! I/O, contract validation, foreign-format import — so any producer or consumer
//! can read or write `.oisi` **without** depending on the analysis compute.
//!
//! The dividing line: this crate knows HDF5 structure, the raw-acquisition
//! payload, and the schema (names-as-strings). It does **not** know what the
//! analysis result names *mean* — that vocabulary (VFS, magnification, retinotopy,
//! params, the incremental cache) lives in `isi-analysis`, which composes this
//! crate's primitives.

pub mod error;
mod import;
pub mod io;
pub mod mat5;
pub mod schema;
pub mod types;

pub use error::OisiError;
pub use import::import_snlc_directory;
pub use types::*;
