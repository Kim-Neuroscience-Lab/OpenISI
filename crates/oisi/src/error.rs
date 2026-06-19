//! Error type for the `.oisi` format I/O layer.
//!
//! `OisiError` carries the four format-layer failure modes — I/O, HDF5,
//! malformed package, missing data — with no analysis-vocabulary variants
//! (compute / validation / cancellation live in `isi-analysis`'s
//! `AnalysisError`, which has a `From<OisiError>` that maps these four
//! variant-for-variant, preserving the IPC wire codes).

/// Errors from reading or writing the `.oisi` format.
#[derive(Debug, thiserror::Error)]
pub enum OisiError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("HDF5 error ({context}): {source}")]
    Hdf5 {
        context: String,
        #[source]
        source: hdf5::Error,
    },

    #[error("Invalid .oisi file: {0}")]
    InvalidPackage(String),

    #[error("Missing data: {0}")]
    MissingData(String),
}

impl OisiError {
    /// Construct an HDF5 error that **preserves the underlying `hdf5::Error`** as
    /// its `source` (never stringified), with `context` naming the operation.
    pub fn hdf5(context: impl Into<String>, source: hdf5::Error) -> Self {
        Self::Hdf5 {
            context: context.into(),
            source,
        }
    }
}

impl From<hdf5::Error> for OisiError {
    fn from(source: hdf5::Error) -> Self {
        Self::Hdf5 {
            context: String::new(),
            source,
        }
    }
}
