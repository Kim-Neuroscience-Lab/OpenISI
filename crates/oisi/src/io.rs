//! `.oisi` format I/O — the HDF5 primitives + the format-pure read/write
//! functions. This is the format layer: it knows the HDF5 structure, the
//! raw-acquisition payload, and the schema (names-as-strings). It does **not**
//! know what analysis result names *mean* — the analysis-semantic readers
//! (retinotopy/responsiveness/reliability restore, `inspect`, the stage-cache
//! and fingerprint helpers) live in `isi-analysis`'s `io` module, which calls
//! these primitives.
//!
//! HDF5 layout:
//!
//! ```text
//!   /version                     attr: "1.0"
//!   /created_at                  attr: ISO-8601 string
//!   /source_type                 attr: "raw_acquisition" | "complex_maps_import" | ...
//!   /analysis_params             attr: JSON string (serialized AnalysisParams)
//!
//!   /anatomical                  dataset: u8 (H, W)  — optional
//!
//!   /acquisition/                group — optional, only from raw acquisition
//!     frames/<name>              dataset: f32 (T, H, W) chunked+gzip
//!     timestamps/<name>          dataset: f64 (T,)
//!
//!   /complex_maps/               group — present after DFT or import
//!     azi_fwd                    dataset: f64 (H, W, 2) where [:,:,0]=re, [:,:,1]=im
//!     azi_rev, alt_fwd, alt_rev
//!
//!   /results/                    group — present after retinotopy computation
//!     azi_phase                  dataset: f64 (H, W)
//!     alt_phase, azi_phase_degrees, alt_phase_degrees
//!     azi_amplitude, alt_amplitude, vfs
//! ```

use hdf5::File as H5File;
use ndarray::{Array2, Array3};
use num_complex::Complex64;
use std::path::Path;

use crate::schema::name;
use crate::{AcquisitionIdentity, ComplexMaps, OisiError, RawAcquisition};

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

/// Read the four complex maps.
pub fn read_complex_maps(path: &Path) -> Result<ComplexMaps, OisiError> {
    let file = open_read(path)?;

    let read_complex = |name: &str| -> Result<Array2<Complex64>, OisiError> {
        let ds_path = format!("complex_maps/{name}");
        let ds = file
            .dataset(&ds_path)
            .map_err(|e| OisiError::MissingData(format!("{ds_path}: {e}")))?;
        let raw: Array3<f64> = ds
            .read()
            .map_err(|e| OisiError::hdf5(format!("reading {ds_path}"), e))?;
        let (h, w, c) = raw.dim();
        if c != 2 {
            return Err(OisiError::InvalidPackage(format!(
                "{ds_path}: expected shape (H,W,2), got dim 2 = {c}"
            )));
        }
        let mut result = Array2::<Complex64>::zeros((h, w));
        for r in 0..h {
            for col in 0..w {
                result[[r, col]] = Complex64::new(raw[[r, col, 0]], raw[[r, col, 1]]);
            }
        }
        Ok(result)
    };

    Ok(ComplexMaps {
        azi_fwd: read_complex("azi_fwd")?,
        azi_rev: read_complex("azi_rev")?,
        alt_fwd: read_complex("alt_fwd")?,
        alt_rev: read_complex("alt_rev")?,
    })
}

/// Read the acquisition identity attributes from a `.oisi` file.
///
/// A **missing** attribute is legitimate (imports carry no `animal_id`) and reads
/// as an empty string. An attribute that **exists but cannot be read** is
/// corruption, and is surfaced as an error rather than silently defaulted — a
/// silent default would let a damaged recording masquerade as one with empty
/// identity (and that identity keys the incremental cache).
pub fn read_acquisition_identity(path: &Path) -> Result<AcquisitionIdentity, OisiError> {
    let file = open_read(path)?;
    let read = |name: &str| -> Result<String, OisiError> {
        match file.attr(name) {
            // Present but unreadable → corruption → fail loud (never default).
            Ok(attr) => attr
                .read_scalar::<hdf5::types::VarLenUnicode>()
                .map(|s| s.as_str().to_string())
                .map_err(|e| OisiError::hdf5(format!("reading {name} attribute"), e)),
            // Absent → legitimate (e.g. imported files) → empty.
            Err(_) => Ok(String::new()),
        }
    };
    Ok(AcquisitionIdentity {
        animal_id: read("animal_id")?,
        created_at: read("created_at")?,
    })
}

/// Read the `rig_params` JSON attribute from a `.oisi` file, if present.
/// Captured at acquisition time (`src-tauri/src/export.rs::write_oisi`).
/// Returns an opaque `serde_json::Value` because the analysis crate
/// doesn't have a typed `RigParams` struct — the rig config is
/// provenance, not analysis input. Returns `None` for files captured
/// before `/rig_params` was written.
pub fn read_rig_params(path: &Path) -> Result<Option<serde_json::Value>, OisiError> {
    read_root_json_attr(path, "rig_params")
}

/// Read the `experiment_params` JSON attribute from a `.oisi` file, if
/// present. Same provenance role as `read_rig_params`. Returns `None`
/// for files captured before `/experiment_params` was written.
pub fn read_experiment_params(path: &Path) -> Result<Option<serde_json::Value>, OisiError> {
    read_root_json_attr(path, "experiment_params")
}

/// Helper for reading a JSON-encoded root HDF5 attribute that may be
/// absent on older files. Used by `read_rig_params` and
/// `read_experiment_params`.
fn read_root_json_attr(path: &Path, name: &str) -> Result<Option<serde_json::Value>, OisiError> {
    let file = open_read(path)?;
    let attr_names = file
        .attr_names()
        .map_err(|e| OisiError::hdf5("listing root attrs", e))?;
    if !attr_names.iter().any(|n| n == name) {
        return Ok(None);
    }
    let attr = file
        .attr(name)
        .map_err(|e| OisiError::hdf5(format!("opening {name} attr"), e))?;
    let json_vlu: hdf5::types::VarLenUnicode = attr
        .read_scalar()
        .map_err(|e| OisiError::hdf5(format!("reading {name} attr"), e))?;
    let value: serde_json::Value = serde_json::from_str(json_vlu.as_str())
        .map_err(|e| OisiError::InvalidPackage(format!("parsing {name}: {e}")))?;
    Ok(Some(value))
}

/// Read the user-drawn cortex ROI from `/anatomical/cortex_roi`, if
/// present. Returns `Ok(None)` when the dataset is absent (no user
/// override for this file). Returns `Err` only on I/O / parse failure.
///
/// The dataset is stored as `u8` (0/1) for HDF5 compatibility; this
/// helper converts to `Array2<bool>`. Source-of-truth path is
/// `/anatomical/cortex_roi`; consumers (analyze orchestrator, future
/// UI) write to that path when the user provides an explicit ROI.
pub fn read_cortex_roi(path: &Path) -> Result<Option<Array2<bool>>, OisiError> {
    let file = open_read(path)?;
    if !file.link_exists("anatomical/cortex_roi") {
        return Ok(None);
    }
    let ds = file
        .dataset("anatomical/cortex_roi")
        .map_err(|e| OisiError::hdf5("opening anatomical/cortex_roi", e))?;
    let data: Array2<u8> = ds
        .read()
        .map_err(|e| OisiError::hdf5("reading anatomical/cortex_roi", e))?;
    Ok(Some(data.mapv(|v| v != 0)))
}

/// Read the anatomical image as u8 grayscale.
pub fn read_anatomical(path: &Path) -> Result<Array2<u8>, OisiError> {
    let file = open_read(path)?;
    let ds = file
        .dataset("anatomical")
        .map_err(|e| OisiError::MissingData(format!("anatomical: {e}")))?;
    let data: Array2<u8> = ds
        .read()
        .map_err(|e| OisiError::hdf5("reading anatomical", e))?;
    Ok(data)
}

/// Read the raw acquisition arrays — camera frames + timestamps + sweep
/// schedule — from an `.oisi` file into a [`RawAcquisition`].
///
/// This is the HDF5 half of the raw→complex path; the pure compute half (ΔF/F
/// baseline + the per-cycle DFT) is HDF5-free. The split keeps the pipeline's
/// `Baseline`/`Projection` stages HDF5-free: the boundary calls this, the
/// stages borrow the result.
pub fn read_raw_acquisition(path: &Path) -> Result<RawAcquisition, OisiError> {
    let file = open_read(path)?;

    if file.group("acquisition/camera").is_err() {
        return Err(OisiError::MissingData(
            "Expected acquisition/camera/ group".into(),
        ));
    }

    let frames_ds = file
        .dataset("acquisition/camera/frames")
        .map_err(|e| OisiError::hdf5("opening camera/frames", e))?;
    let frames: Array3<u16> = frames_ds
        .read()
        .map_err(|e| OisiError::hdf5("reading camera/frames", e))?;

    let cam_ts_sec: Vec<f64> = file
        .dataset("acquisition/camera/timestamps_sec")
        .map_err(|e| OisiError::hdf5("opening camera timestamps_sec", e))?
        .read_1d()
        .map_err(|e| OisiError::hdf5("reading camera timestamps_sec", e))?
        .to_vec();

    // Sweep schedule — onset times + per-sweep duration + direction.
    let sweep_start_sec: Vec<f64> = file
        .dataset("acquisition/schedule/sweep_start_sec")
        .map_err(|e| OisiError::hdf5("opening sweep_start_sec", e))?
        .read_1d()
        .map_err(|e| OisiError::hdf5("reading sweep_start_sec", e))?
        .to_vec();
    let sweep_end_sec: Vec<f64> = file
        .dataset("acquisition/schedule/sweep_end_sec")
        .map_err(|e| OisiError::hdf5("opening sweep_end_sec", e))?
        .read_1d()
        .map_err(|e| OisiError::hdf5("reading sweep_end_sec", e))?
        .to_vec();
    // `sweep_sequence` — the per-sweep direction list (SSoT for cycle
    // grouping), read via the shared helper that `inspect` also uses.
    let sweep_sequence = read_sweep_sequence(&file)?;

    Ok(RawAcquisition {
        frames,
        cam_ts_sec,
        sweep_start_sec,
        sweep_end_sec,
        sweep_sequence,
    })
}

/// Read the per-sweep direction sequence from `/acquisition/schedule`'s
/// `sweep_sequence` JSON attribute — the single source of truth for which
/// stimulus direction each sweep belongs to. Used by BOTH the DFT (cycle
/// grouping) and `inspect` (schedule summary), so the two can never disagree
/// on how many cycles a recording has.
pub fn read_sweep_sequence(file: &H5File) -> Result<Vec<String>, OisiError> {
    let schedule_group = file
        .group("acquisition/schedule")
        .map_err(|_| OisiError::MissingData("acquisition/schedule".into()))?;
    let seq_json: hdf5::types::VarLenUnicode = schedule_group
        .attr("sweep_sequence")
        .map_err(|e| OisiError::hdf5("reading sweep_sequence", e))?
        .read_scalar()
        .map_err(|e| OisiError::hdf5("reading sweep_sequence value", e))?;
    serde_json::from_str(seq_json.as_str())
        .map_err(|e| OisiError::InvalidPackage(format!("parsing sweep_sequence: {e}")))
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

/// The `.oisi` format version this build writes and recognizes.
pub const FORMAT_VERSION: &str = "1.0";

/// Verify a file's format version is one this build can read.
///
/// A **missing** version attribute is tolerated (pre-versioning files; their
/// `/analysis_params` schema is brought forward by `isi-analysis`'s migrate). A
/// version that is **present but unrecognized** is rejected rather than silently
/// misread — forward compatibility (PRINCIPLES Invariant 4): never guess at a
/// format written by a newer OpenISI.
pub fn verify_format_version(path: &Path) -> Result<(), OisiError> {
    let file = open_read(path)?;
    match file.attr("version") {
        Ok(attr) => {
            let v = attr
                .read_scalar::<hdf5::types::VarLenUnicode>()
                .map_err(|e| OisiError::hdf5("reading version attribute", e))?;
            if v.as_str() == FORMAT_VERSION {
                Ok(())
            } else {
                Err(OisiError::InvalidPackage(format!(
                    "unrecognized .oisi format version {:?} (this build reads {FORMAT_VERSION:?}); the file may be from a newer OpenISI",
                    v.as_str()
                )))
            }
        }
        // Absent → pre-versioning file; tolerate (schema migration handles it).
        Err(_) => Ok(()),
    }
}

/// Create a new .oisi file with just metadata.
pub fn create(path: &Path, source_type: &str) -> Result<(), OisiError> {
    let file = H5File::create(path)
        .map_err(|e| OisiError::hdf5(format!("creating {}", path.display()), e))?;

    write_str_attr(&file, name::VERSION, FORMAT_VERSION)?;
    write_str_attr(&file, name::SOURCE_TYPE, source_type)?;
    write_str_attr(&file, name::CREATED_AT, &chrono_now())?;

    Ok(())
}

/// Strip derived, recomputable outputs so the next analyze recomputes
/// from the rawest available input: `results`, the `analysis_state` stage
/// fingerprints, and the `/cache` intermediates always, plus `complex_maps` when
/// raw `acquisition` frames are present (for cycle-averaged imports the complex
/// maps ARE the input, so they are kept).
///
/// The retinotopy fingerprint keys on params + data, not the code version, so a
/// stale cache can silently mask a code change. Test/baseline harnesses call
/// this before analyzing so they exercise the compute path unconditionally.
pub fn strip_derived_outputs(path: &Path) -> Result<(), OisiError> {
    let file = open_readwrite(path)?;
    let has_raw = file.group("acquisition").is_ok();
    let _ = file.unlink("results");
    let _ = file.unlink("analysis_state");
    let _ = file.unlink("cache");
    if has_raw {
        let _ = file.unlink("complex_maps");
    }
    Ok(())
}

/// Apply a set of in-place `.oisi` mutations **atomically**: copy the file to a
/// sibling temp, run `mutate` against the temp, fsync it, then atomically
/// `rename` the temp over the original. A crash / disk-full / panic at any point
/// leaves the ORIGINAL file untouched (the temp is removed on error).
///
/// This guards the analysis write path the way `export.rs` already guards
/// acquisition capture: HDF5 B-tree/superblock updates are **not** atomic, so an
/// in-place mid-write crash can corrupt the whole file — including its
/// (irreplaceable) raw `/acquisition` frames. See `docs/FOUNDATION_AUDIT.md` A1.
///
/// Output is byte-identical to the equivalent in-place write: the temp starts as
/// an exact copy of the original, and the `write_*` helpers unlink-then-recreate
/// their groups, so they perform the same HDF5 operations on the same starting
/// bytes. Cost: one full-file copy per call — acceptable because analyses are
/// infrequent and the raw data is irreplaceable; correctness dominates.
pub fn atomic_update<F, E>(path: &Path, mutate: F) -> Result<(), E>
where
    F: FnOnce(&Path) -> Result<(), E>,
    E: From<OisiError>,
{
    let tmp = path.with_extension(match path.extension().and_then(|e| e.to_str()) {
        Some("oisi") => "oisi.analyzing".to_string(),
        _ => "analyzing".to_string(),
    });
    let io_err = |ctx: String| E::from(OisiError::Io(std::io::Error::other(ctx)));

    // Clear any stale temp left by a previously-killed run, then copy.
    let _ = std::fs::remove_file(&tmp);
    std::fs::copy(path, &tmp).map_err(|e| {
        io_err(format!(
            "atomic_update: copy {} -> {}: {e}",
            path.display(),
            tmp.display()
        ))
    })?;

    // Run the mutations on the temp. On ANY error, drop the temp and abort,
    // leaving the original intact.
    if let Err(e) = mutate(&tmp) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }

    // Durably flush the temp to disk before the rename, so a power loss right
    // after the rename can't surface a temp whose bytes never reached storage.
    // The handle must be WRITABLE: Windows `FlushFileBuffers` (sync_all) rejects
    // a read-only handle with "access denied".
    if let Err(e) = std::fs::OpenOptions::new()
        .write(true)
        .open(&tmp)
        .and_then(|f| f.sync_all())
    {
        let _ = std::fs::remove_file(&tmp);
        return Err(io_err(format!("atomic_update: fsync {}: {e}", tmp.display())));
    }

    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        io_err(format!(
            "atomic_update: rename {} -> {}: {e}",
            tmp.display(),
            path.display()
        ))
    })
}

/// Write complex maps to the file.
pub fn write_complex_maps(path: &Path, maps: &ComplexMaps) -> Result<(), OisiError> {
    let file = open_readwrite(path)?;

    // Remove existing group if present, then recreate
    let _ = file.unlink(name::COMPLEX_MAPS);
    let group = file
        .create_group(name::COMPLEX_MAPS)
        .map_err(|e| OisiError::hdf5("creating complex_maps group", e))?;

    let write_complex = |name: &str, data: &Array2<Complex64>| -> Result<(), OisiError> {
        let (h, w) = data.dim();
        let mut raw = Array3::<f64>::zeros((h, w, 2));
        for r in 0..h {
            for c in 0..w {
                raw[[r, c, 0]] = data[[r, c]].re;
                raw[[r, c, 1]] = data[[r, c]].im;
            }
        }
        group
            .new_dataset_builder()
            .with_data(&raw)
            .create(name)
            .map_err(|e| OisiError::hdf5(format!("writing complex_maps/{name}"), e))?;
        Ok(())
    };

    write_complex(name::AZI_FWD, &maps.azi_fwd)?;
    write_complex(name::AZI_REV, &maps.azi_rev)?;
    write_complex(name::ALT_FWD, &maps.alt_fwd)?;
    write_complex(name::ALT_REV, &maps.alt_rev)?;

    Ok(())
}

/// Write an anatomical image.
pub fn write_anatomical(path: &Path, image: &Array2<u8>) -> Result<(), OisiError> {
    let file = open_readwrite(path)?;
    let _ = file.unlink(name::ANATOMICAL);
    file.new_dataset_builder()
        .with_data(image)
        .create(name::ANATOMICAL)
        .map_err(|e| OisiError::hdf5("writing anatomical", e))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Low-level HDF5 primitives — shared by this module, by `isi-analysis`'s
// analysis-semantic I/O, and by the capture-write path (`src-tauri::export`).
// ---------------------------------------------------------------------------

pub fn open_read(path: &Path) -> Result<H5File, OisiError> {
    H5File::open(path).map_err(|e| OisiError::hdf5(format!("opening {}", path.display()), e))
}

pub fn open_readwrite(path: &Path) -> Result<H5File, OisiError> {
    H5File::open_rw(path).map_err(|e| OisiError::hdf5(format!("opening {}", path.display()), e))
}

// String / scalar attribute writers — the `.oisi` HDF5 attribute primitives,
// owned here (the single I/O boundary) and used by both the analysis-write path
// and the capture-write path (`src-tauri::export`). Each takes `&hdf5::Location`,
// the base that both `File` and `Group` coerce to, so one writer serves root and
// group attributes alike.

/// Write (replacing) a string attribute on `location` (a file or a group).
pub fn write_str_attr(location: &hdf5::Location, name: &str, value: &str) -> Result<(), OisiError> {
    // Remove existing attribute if present.
    let _ = location.delete_attr(name);
    let attr = location
        .new_attr::<hdf5::types::VarLenUnicode>()
        .create(name)
        .map_err(|e| OisiError::hdf5(format!("creating attr {name}"), e))?;
    let val: hdf5::types::VarLenUnicode = value
        .parse()
        .map_err(|e| OisiError::InvalidPackage(format!("invalid UTF-8 attr {name}: {e}")))?;
    attr.write_scalar(&val)
        .map_err(|e| OisiError::hdf5(format!("writing attr {name}"), e))?;
    Ok(())
}

/// Write (replacing) an `f64` attribute on `location` (a file or a group).
pub fn write_f64_attr(location: &hdf5::Location, name: &str, value: f64) -> Result<(), OisiError> {
    let _ = location.delete_attr(name);
    let attr = location
        .new_attr::<f64>()
        .create(name)
        .map_err(|e| OisiError::hdf5(format!("creating attr {name}"), e))?;
    attr.write_scalar(&value)
        .map_err(|e| OisiError::hdf5(format!("writing attr {name}"), e))?;
    Ok(())
}

/// Write (replacing) a `u32` attribute on `location` (a file or a group).
pub fn write_u32_attr(location: &hdf5::Location, name: &str, value: u32) -> Result<(), OisiError> {
    let _ = location.delete_attr(name);
    let attr = location
        .new_attr::<u32>()
        .create(name)
        .map_err(|e| OisiError::hdf5(format!("creating attr {name}"), e))?;
    attr.write_scalar(&value)
        .map_err(|e| OisiError::hdf5(format!("writing attr {name}"), e))?;
    Ok(())
}

/// Write a 1-D array as a dataset under `group`, with a Fletcher32 integrity
/// checksum (which requires chunking). An empty array is written unchunked
/// (HDF5 rejects a zero-length chunk). The `.oisi` 1-D dataset primitive,
/// shared by the capture-write and analysis-write paths.
pub fn write_checked_1d<T: hdf5::H5Type + Clone>(
    group: &hdf5::Group,
    name: &str,
    data: Vec<T>,
) -> Result<(), OisiError> {
    if data.is_empty() {
        group
            .new_dataset_builder()
            .with_data(&ndarray::Array1::<T>::from(data))
            .create(name)
            .map_err(|e| OisiError::hdf5(format!("writing {name}"), e))?;
    } else {
        let len = data.len();
        group
            .new_dataset_builder()
            .fletcher32()
            .chunk((len,))
            .with_data(&ndarray::Array1::from(data))
            .create(name)
            .map_err(|e| OisiError::hdf5(format!("writing {name}"), e))?;
    }
    Ok(())
}

/// List the member (link) names of an HDF5 group.
pub fn list_group_members_from_group(group: &hdf5::Group) -> Result<Vec<String>, OisiError> {
    group
        .member_names()
        .map_err(|e| OisiError::hdf5("listing HDF5 group members", e))
}

/// Read a string attribute on `location` (a file or a group), or `None`.
pub fn read_str_attr(location: &hdf5::Location, name: &str) -> Option<String> {
    let attr = location.attr(name).ok()?;
    let v: hdf5::types::VarLenUnicode = attr.read_scalar().ok()?;
    Some(v.to_string())
}

/// Read an `f64` attribute on `location` (a file or a group), or `None`.
pub fn read_f64_attr(location: &hdf5::Location, name: &str) -> Option<f64> {
    let attr = location.attr(name).ok()?;
    attr.read_scalar::<f64>().ok()
}

fn chrono_now() -> String {
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", duration.as_secs())
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::Array2;
    use num_complex::Complex64;
    use std::path::PathBuf;

    /// Helper: create a unique temp file path and ensure cleanup on drop.
    struct TempFile(PathBuf);

    impl TempFile {
        fn new(name: &str) -> Self {
            let mut path = std::env::temp_dir();
            path.push(format!("openisi_oisi_test_{}_{}", name, std::process::id()));
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    /// Build synthetic ComplexMaps of the given size.
    fn make_complex_maps(h: usize, w: usize) -> ComplexMaps {
        let make = |scale: f64| -> Array2<Complex64> {
            Array2::from_shape_fn((h, w), |(r, c)| {
                Complex64::new((r as f64 + 1.0) * scale, (c as f64 + 1.0) * scale * 0.5)
            })
        };
        ComplexMaps {
            azi_fwd: make(1.0),
            azi_rev: make(2.0),
            alt_fwd: make(3.0),
            alt_rev: make(4.0),
        }
    }

    #[test]
    fn complex_maps_round_trip() {
        let tmp = TempFile::new("complex_rt");
        let maps = make_complex_maps(8, 8);

        create(tmp.path(), "test").unwrap();
        write_complex_maps(tmp.path(), &maps).unwrap();

        let loaded = read_complex_maps(tmp.path()).unwrap();

        assert_eq!(loaded.azi_fwd.dim(), (8, 8));
        assert_eq!(loaded.azi_fwd, maps.azi_fwd);
        assert_eq!(loaded.azi_rev, maps.azi_rev);
        assert_eq!(loaded.alt_fwd, maps.alt_fwd);
        assert_eq!(loaded.alt_rev, maps.alt_rev);
    }

    #[test]
    fn anatomical_round_trip() {
        let tmp = TempFile::new("anat_rt");
        let (h, w) = (16, 16);
        let image = Array2::from_shape_fn((h, w), |(r, c)| ((r * w + c) % 256) as u8);

        create(tmp.path(), "test").unwrap();
        write_anatomical(tmp.path(), &image).unwrap();

        let loaded = read_anatomical(tmp.path()).unwrap();
        assert_eq!(loaded, image);
    }

    #[test]
    fn complex_maps_overwrite() {
        let tmp = TempFile::new("complex_overwrite");
        create(tmp.path(), "test").unwrap();

        let maps_v1 = make_complex_maps(8, 8);
        write_complex_maps(tmp.path(), &maps_v1).unwrap();

        // Write different maps.
        let maps_v2 = ComplexMaps {
            azi_fwd: Array2::from_elem((8, 8), Complex64::new(99.0, 99.0)),
            azi_rev: Array2::from_elem((8, 8), Complex64::new(88.0, 88.0)),
            alt_fwd: Array2::from_elem((8, 8), Complex64::new(77.0, 77.0)),
            alt_rev: Array2::from_elem((8, 8), Complex64::new(66.0, 66.0)),
        };
        write_complex_maps(tmp.path(), &maps_v2).unwrap();

        let loaded = read_complex_maps(tmp.path()).unwrap();
        assert_eq!(loaded.azi_fwd[[0, 0]], Complex64::new(99.0, 99.0));
        assert_eq!(loaded.alt_rev[[0, 0]], Complex64::new(66.0, 66.0));
    }

    #[test]
    fn verify_format_version_accepts_current_and_rejects_unknown() {
        let tmp = TempFile::new("format_version");
        create(tmp.path(), "test").unwrap();

        // A freshly created file carries FORMAT_VERSION and is accepted.
        verify_format_version(tmp.path()).unwrap();

        // Stamp an unrecognized (e.g. newer) version → rejected, never misread.
        {
            let file = open_readwrite(tmp.path()).unwrap();
            write_str_attr(&file, "version", "99.0").unwrap();
        }
        let err = verify_format_version(tmp.path()).unwrap_err();
        assert!(
            matches!(err, OisiError::InvalidPackage(_)),
            "expected InvalidPackage, got {err:?}"
        );
    }

    /// A failing mutation must leave the ORIGINAL file byte-for-byte intact and
    /// remove the temp — the whole point of A1 (a crash/disk-full mid-write
    /// cannot corrupt the live `.oisi`).
    #[test]
    fn atomic_update_leaves_original_intact_on_mutate_error() {
        let tmp = TempFile::new("atomic_err");
        std::fs::write(tmp.path(), b"ORIGINAL-BYTES").unwrap();

        let result = atomic_update(tmp.path(), |scratch| {
            // Simulate a partial write that then fails (disk-full / crash).
            std::fs::write(scratch, b"HALF-WRITTEN-GARBAGE").unwrap();
            Err(OisiError::InvalidPackage("simulated mid-write failure".into()))
        });

        assert!(result.is_err(), "atomic_update should surface the error");
        assert_eq!(
            std::fs::read(tmp.path()).unwrap(),
            b"ORIGINAL-BYTES",
            "original must be untouched after a failed mutation"
        );
        assert!(
            !tmp.path().with_extension("analyzing").exists(),
            "the temp must be cleaned up on failure"
        );
        let _ = std::fs::remove_file(tmp.path());
    }

    /// A successful mutation publishes the temp over the original (atomic
    /// rename) and leaves no temp behind.
    #[test]
    fn atomic_update_publishes_on_success() {
        let tmp = TempFile::new("atomic_ok");
        std::fs::write(tmp.path(), b"ORIGINAL").unwrap();

        atomic_update(tmp.path(), |scratch| {
            std::fs::write(scratch, b"NEW-CONTENTS").unwrap();
            Ok::<(), OisiError>(())
        })
        .unwrap();

        assert_eq!(std::fs::read(tmp.path()).unwrap(), b"NEW-CONTENTS");
        assert!(!tmp.path().with_extension("analyzing").exists());
        let _ = std::fs::remove_file(tmp.path());
    }
}
