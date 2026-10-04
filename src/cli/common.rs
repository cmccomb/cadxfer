//! Shared CLI output and source-format option helpers.

use std::io::Write;
use std::path::Path;

use caexfer::conversion::{Format, Options};
use caexfer::core::Result;

use super::args::Args;
use super::json::quote;

/// Write one complete output line to locked stdout with I/O error propagation.
pub(super) fn emit(value: &str) -> Result<()> {
    // Lock stdout for the whole line so other writes cannot interleave it.
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{value}")?;
    Ok(())
}

/// Quote an OS path for JSON, using a displayable lossy form when needed.
pub(super) fn path_json(path: &Path) -> String {
    quote(&path.to_string_lossy())
}

/// Prefer an explicit source format; otherwise use the input path extension.
fn format_of(args: &Args) -> Result<Format> {
    args.from
        .as_deref()
        .map_or_else(|| Format::from_input_path(&args.paths[0]), Format::parse)
}

/// Translate validated CLI flags into the library's typed conversion options.
pub(super) fn conversion_options(args: &Args) -> Result<Options> {
    let format = format_of(args)?;
    Ok(Options {
        input_format: Some(format),
        mesh: args.mesh.clone(),
        assume_basic_frame: args.assume_basic_frame,
        subcase: args.subcase,
        step: args.step,
        max_bytes: args.max_bytes.unwrap_or(Options::default().max_bytes),
        msh_version: args.msh_version,
        zero_missing_rotations: args.zero_missing_rotations,
    })
}
