//! SNLC sample-data bundle: download from GitHub, extract, and import each
//! subject directory into a `.oisi` file. Shared between the Tauri command
//! `commands::library::import_snlc_sample_data` and the headless CLI
//! subcommand `import-samples` so both surfaces use the same fetch path.
//!
//! Contract: either every discovered subject imports successfully and the
//! function returns the list of created `.oisi` paths, or the first failure
//! aborts the bundle with a specific `AppError`. There is no "partial
//! success" return shape — the type system makes a `(imported, errors)`
//! mix inexpressible.
//!
//! Bundles produced on macOS embed AppleDouble resource forks under
//! `__MACOSX/` and dot-prefixed `._*.mat` shadow files. These are not
//! subject directories; `find_subject_dirs` rejects them at the discovery
//! boundary so they never reach the import step. Anything that survives
//! discovery is treated as a real subject and *must* import successfully.

use std::io;
use std::path::{Path, PathBuf};

use crate::error::AppError;

const SNLC_SAMPLE_DATA_URL: &str =
    "https://github.com/SNLC/ISI/raw/master/Sample%20Data.zip";

/// Download the SNLC sample zip, extract it, discover subject directories,
/// and import each into `out_dir`. Returns one `.oisi` per discovered
/// subject in the order they were discovered (alphabetical by full path).
///
/// All-or-nothing: any failure during download, extraction, discovery, or
/// per-subject import aborts the bundle. The temp directory is cleaned up
/// before the function returns on every code path.
pub fn import_snlc_sample_bundle(out_dir: &Path) -> Result<Vec<PathBuf>, AppError> {
    create_dir(out_dir)?;

    let temp_dir = std::env::temp_dir().join("openisi_sample_data");
    let _ = std::fs::remove_dir_all(&temp_dir);
    create_dir(&temp_dir)?;

    // Guard: clean up temp_dir on every return path, including panics.
    struct CleanupGuard(PathBuf);
    impl Drop for CleanupGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = CleanupGuard(temp_dir.clone());

    let zip_path = temp_dir.join("sample_data.zip");
    download_zip(SNLC_SAMPLE_DATA_URL, &zip_path)?;

    let extract_dir = temp_dir.join("extracted");
    extract_zip(&zip_path, &extract_dir)?;
    let _ = std::fs::remove_file(&zip_path);

    let subject_dirs = find_subject_dirs(&extract_dir);
    if subject_dirs.is_empty() {
        return Err(AppError::NotAvailable(format!(
            "bundle contains no subject directories (extracted to {})",
            extract_dir.display(),
        )));
    }
    eprintln!("[sample_data] found {} subject directories", subject_dirs.len());

    let mut imported = Vec::with_capacity(subject_dirs.len());
    for dir in &subject_dirs {
        let name = dir.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "<unnamed>".into());
        let output_path = out_dir.join(format!("{name}.oisi"));

        // Per-subject import failures surface as `AppError::Analysis` with
        // the AnalysisError chained. Context (which subject, which source
        // dir) goes into a single io::Error wrap so the message stays
        // structurally a domain error with annotation, not a string-only
        // top-level error.
        isi_analysis::io::import_snlc_directory(dir, &output_path)
            .map_err(|source| AppError::Analysis(annotate_subject_error(&name, dir, source)))?;
        eprintln!("[sample_data] imported {name} → {}", output_path.display());
        imported.push(output_path);
    }
    eprintln!("[sample_data] complete: {} imported", imported.len());
    Ok(imported)
}

fn create_dir(path: &Path) -> Result<(), AppError> {
    std::fs::create_dir_all(path).map_err(|e| io_with_context(format!(
        "create directory {}", path.display(),
    ), e))
}

fn download_zip(url: &str, zip_path: &Path) -> Result<(), AppError> {
    eprintln!("[sample_data] downloading {url}");
    let response = ureq::get(url).call().map_err(|e| io_with_context(
        format!("download {url}"), other_io(e.to_string()),
    ))?;
    let mut zip_file = std::fs::File::create(zip_path)
        .map_err(|e| io_with_context(format!("create {}", zip_path.display()), e))?;
    std::io::copy(&mut response.into_body().as_reader(), &mut zip_file)
        .map_err(|e| io_with_context(format!("write {}", zip_path.display()), e))?;
    eprintln!("[sample_data] download complete");
    Ok(())
}

fn extract_zip(zip_path: &Path, extract_dir: &Path) -> Result<(), AppError> {
    eprintln!("[sample_data] extracting to {}", extract_dir.display());
    let file = std::fs::File::open(zip_path)
        .map_err(|e| io_with_context(format!("open {}", zip_path.display()), e))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| io_with_context(
        format!("read zip {}", zip_path.display()), other_io(e.to_string()),
    ))?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| io_with_context(
            format!("zip entry #{i} in {}", zip_path.display()), other_io(e.to_string()),
        ))?;
        let entry_path = match entry.enclosed_name() {
            Some(p) => extract_dir.join(p),
            None => continue,
        };
        if entry.is_dir() {
            create_dir(&entry_path)?;
        } else {
            if let Some(parent) = entry_path.parent() {
                create_dir(parent)?;
            }
            let mut out = std::fs::File::create(&entry_path).map_err(|e| io_with_context(
                format!("create {}", entry_path.display()), e,
            ))?;
            std::io::copy(&mut entry, &mut out).map_err(|e| io_with_context(
                format!("write {}", entry_path.display()), e,
            ))?;
        }
    }
    Ok(())
}

/// Wrap an `AnalysisError` from a per-subject import with the subject name
/// and source directory in a `MissingData` envelope. The underlying error
/// stays in the message; nothing is dropped.
fn annotate_subject_error(
    name: &str,
    dir: &Path,
    source: isi_analysis::AnalysisError,
) -> isi_analysis::AnalysisError {
    isi_analysis::AnalysisError::MissingData(format!(
        "subject '{name}' at {}: {source}", dir.display(),
    ))
}

fn io_with_context(context: String, source: io::Error) -> AppError {
    AppError::Io(io::Error::new(source.kind(), format!("{context}: {source}")))
}

fn other_io(message: String) -> io::Error {
    io::Error::other(message)
}

/// Walk `root` and return every directory that looks like an SNLC subject:
/// it contains at least one `.mat` file whose name does not start with a
/// dot (excludes AppleDouble `._*` resource forks) and is not located
/// inside an excluded ancestor (`__MACOSX`, dot-prefixed directories).
///
/// The filter is structural: garbage is rejected *here*, never silently
/// "imported and fails downstream". Discovery output is the contract for
/// the import loop — every element must be importable.
fn find_subject_dirs(root: &Path) -> Vec<PathBuf> {
    let mut results = Vec::new();
    walk(root, &mut results);
    results.sort();
    return results;

    fn walk(dir: &Path, results: &mut Vec<PathBuf>) {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        let mut has_data_mat = false;
        let mut subdirs = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => continue,
            };
            if is_excluded(name) { continue; }
            if path.is_dir() {
                subdirs.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("mat") {
                has_data_mat = true;
            }
        }
        if has_data_mat {
            results.push(dir.to_path_buf());
        }
        for sub in subdirs {
            walk(&sub, results);
        }
    }

    /// Names that never belong to a real SNLC subject directory or data file.
    fn is_excluded(name: &str) -> bool {
        // macOS resource fork tree and AppleDouble shadow files
        if name == "__MACOSX" { return true; }
        if name.starts_with("._") { return true; }
        // Hidden files / dotdirs (`.DS_Store`, `.git`, etc.)
        if name.starts_with('.') { return true; }
        false
    }
}
