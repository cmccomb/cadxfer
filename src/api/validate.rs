//! Supported-subset validation without file output.

use std::path::{Path, PathBuf};

use crate::conversion::{self, Format, Omission, Options};
use crate::core::Result;

/// Supported-subset validation counts, omissions, and assumptions.
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
    /// Information omitted or assumed while reading the source.
    pub omissions: Vec<Omission>,
}

/// Validate one file against caexfer's supported mesh-and-fields subset.
///
/// Like `caexfer validate`, ordinary mode reports source omissions and
/// unverified assumptions. Strict mode marks the report as failed when any
/// notice is present. A non-BDF OP2/PCH companion is read under a reported
/// basic-frame assumption; validation does not accept it for conversion.
/// Parse and projection errors return [`crate::Error`] instead of a report.
///
/// # Errors
///
/// Returns a source format, I/O, size-limit, or projection error.
pub fn validate(path: impl AsRef<Path>, options: &Options) -> Result<ValidationReport> {
    let path = path.as_ref();
    let format = options
        .input_format
        .map_or_else(|| Format::from_input_path(path), Ok)?;
    let mut read_options = options.clone();
    if matches!(format, Format::Op2 | Format::Pch)
        && options.mesh.as_ref().is_some_and(|mesh| {
            Format::from_input_path(mesh).is_ok_and(|mesh_format| mesh_format != Format::Bdf)
        })
    {
        read_options.assume_basic_frame = true;
    }
    let read = conversion::read_path(path, &read_options)?;
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
