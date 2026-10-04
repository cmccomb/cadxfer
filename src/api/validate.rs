//! Supported-subset validation without file output.

use std::path::{Path, PathBuf};

use crate::conversion::{self, Format, Omission, Options};
use crate::core::Result;

/// Supported-subset validation counts and source omissions.
/// A passing report does not establish solver-model correctness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationReport {
    /// Validated source path.
    pub path: PathBuf,
    /// Detected or explicitly selected source format.
    pub format: Format,
    /// Whether strict omission checking was requested.
    pub strict: bool,
    /// Whether the supported-subset check passed under the chosen strictness.
    pub passed: bool,
    /// Projected point count.
    pub points: usize,
    /// Projected cell count.
    pub cells: usize,
    /// Projected numeric field count.
    pub fields: usize,
    /// Information omitted while reading the source.
    pub omissions: Vec<Omission>,
}

/// Validate one file against caexfer's supported mesh-and-fields subset.
///
/// Like `caexfer validate`, ordinary mode reports source omissions and strict
/// mode marks the report as failed when any omission is present. Parse and
/// projection errors return [`crate::Error`] instead of a report.
///
/// # Errors
///
/// Returns a source format, I/O, size-limit, or projection error.
pub fn validate(path: impl AsRef<Path>, options: &Options) -> Result<ValidationReport> {
    let path = path.as_ref();
    let read = conversion::read_path(path, options)?;
    let passed = !options.strict || read.omissions.is_empty();
    Ok(ValidationReport {
        path: path.to_path_buf(),
        format: read.format,
        strict: options.strict,
        passed,
        points: read.dataset.mesh.points.len(),
        cells: read.dataset.mesh.cells.len(),
        fields: read.dataset.fields.len(),
        omissions: read.omissions,
    })
}
