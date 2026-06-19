//! Import of foreign SNLC/Callaway MATLAB `.mat` retinotopy data into a fresh
//! `.oisi` file. A distinct concern from reading/writing our own `.oisi` format
//! (it ingests an external format via the `mat5` parser), split out of `io.rs`.
//! Reuses the parent module's `.oisi` writers (`create`, `write_complex_maps`,
//! `write_anatomical`) via `super::`.

use std::path::Path;

use crate::io::{create, write_anatomical, write_complex_maps};
use crate::{mat5, ComplexMaps, OisiError};

pub fn import_snlc_directory(dir_path: &Path, output_path: &Path) -> Result<(), OisiError> {
    // Find .mat files in the directory
    let entries = std::fs::read_dir(dir_path).map_err(OisiError::Io)?;

    let mut data_mats: Vec<std::path::PathBuf> = Vec::new();
    let mut grab_mat: Option<std::path::PathBuf> = None;

    for entry in entries {
        let entry = entry.map_err(OisiError::Io)?;
        let path = entry.path();
        if let Some(ext) = path.extension() {
            if ext.eq_ignore_ascii_case("mat") {
                let Some(file_name) = path.file_name() else {
                    continue;
                };
                let name = file_name.to_string_lossy().to_lowercase();
                if name.starts_with("grab") || name.starts_with("grab_") {
                    grab_mat = Some(path);
                } else if !name.contains("analyzer") {
                    data_mats.push(path);
                }
            }
        }
    }

    // Sort data .mat files by name so lower number = horizontal (azimuth)
    data_mats.sort();

    if data_mats.len() < 2 {
        return Err(OisiError::MissingData(format!(
            "need at least 2 .mat data files in {}, found {}",
            dir_path.display(),
            data_mats.len()
        )));
    }

    // Read complex maps from the paired .mat files
    // Convention: first file (lower number) = horizontal = azimuth
    //             second file (higher number) = vertical = altitude
    let azi_cells = mat5::read_snlc_f1m(&data_mats[0])?;
    if azi_cells.len() < 2 {
        return Err(OisiError::InvalidPackage(format!(
            "{}: f1m has {} cells, expected 2",
            data_mats[0].display(),
            azi_cells.len()
        )));
    }

    let alt_cells = mat5::read_snlc_f1m(&data_mats[1])?;
    if alt_cells.len() < 2 {
        return Err(OisiError::InvalidPackage(format!(
            "{}: f1m has {} cells, expected 2",
            data_mats[1].display(),
            alt_cells.len()
        )));
    }

    // The `< 2` length checks above guarantee these `next()` calls
    // succeed, but unwrap would panic on a stale .mat file with an
    // unexpected layout. Use explicit ok_or_else so any mismatch
    // surfaces as a clean `InvalidPackage` instead of a backtrace.
    let mut azi_iter = azi_cells.into_iter();
    let azi_fwd = azi_iter
        .next()
        .ok_or_else(|| {
            OisiError::InvalidPackage(format!(
                "{}: f1m missing azi_fwd cell after length check",
                data_mats[0].display()
            ))
        })?
        .data;
    let azi_rev = azi_iter
        .next()
        .ok_or_else(|| {
            OisiError::InvalidPackage(format!(
                "{}: f1m missing azi_rev cell after length check",
                data_mats[0].display()
            ))
        })?
        .data;

    let mut alt_iter = alt_cells.into_iter();
    let alt_fwd = alt_iter
        .next()
        .ok_or_else(|| {
            OisiError::InvalidPackage(format!(
                "{}: f1m missing alt_fwd cell after length check",
                data_mats[1].display()
            ))
        })?
        .data;
    let alt_rev = alt_iter
        .next()
        .ok_or_else(|| {
            OisiError::InvalidPackage(format!(
                "{}: f1m missing alt_rev cell after length check",
                data_mats[1].display()
            ))
        })?
        .data;

    let complex_maps = ComplexMaps {
        azi_fwd,
        azi_rev,
        alt_fwd,
        alt_rev,
    };

    // Create the .oisi file
    create(output_path, "complex_maps_import")?;
    write_complex_maps(output_path, &complex_maps)?;

    // Import anatomical if present
    if let Some(grab_path) = &grab_mat {
        tracing::info!(path = %grab_path.display(), "found anatomical");
        match mat5::read_snlc_anatomical(grab_path) {
            Ok(anat) => {
                let (h, w) = anat.dim();
                tracing::info!(width = w, height = h, "anatomical imported");
                write_anatomical(output_path, &anat)?;
            }
            Err(e) => {
                tracing::warn!(path = %grab_path.display(), error = %e, "could not read anatomical");
            }
        }
    } else {
        tracing::warn!("no grab_*.mat found in directory");
    }

    Ok(())
}
