//! The `.oisi` file format — the single owner of the on-disk contract.
//!
//! An `.oisi` file is written by acquisition (capture-write) and read and written
//! by analysis. The format is therefore its own bounded concern, depended on by
//! both consumers and depending on neither. This crate owns:
//!
//! - the **data model** — the typed contents of an `.oisi` file (frames, complex
//!   maps, results, render hints, schedule, hardware, timing, provenance);
//! - the **HDF5 read/write boundary** — every `.oisi` I/O operation, in one place,
//!   with source-preserving errors;
//! - the **schema** — generated from these Rust types, so the contract cannot drift;
//! - the **format version + forward migration**.
//!
//! Algorithms (DFT, retinotopy, segmentation) live in `isi-analysis`, a *consumer*
//! of this format — not its owner. See `docs/ARCHITECTURE.md` and `docs/PRINCIPLES.md`.
