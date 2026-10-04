//! Validate a projected dataset and show its counts and source omissions.

use caexfer::conversion;
use caexfer::core::Result;

use super::args::Args;
use super::common::{conversion_options, emit, path_json};
use super::json::{array, object, quote};

/// Validate a source through the format reader and report projected counts.
/// Strict validation fails when the supported projection reports omissions.
pub(super) fn run_validate(args: &Args) -> Result<u8> {
    // Readers return projected datasets and explicit source losses.
    let read = conversion::read_path(&args.paths[0], &conversion_options(args)?)?;
    let format = read.format.name();
    let dataset = read.dataset;
    let omissions = read.omissions;
    // Strict mode treats any documented omission as a failed validation.
    let passed = !args.strict || omissions.is_empty();
    if args.json {
        emit(&object([
            ("schema_version", "1".into()),
            ("format", quote(format)),
            ("path", path_json(&args.paths[0])),
            ("passed", passed.to_string()),
            ("strict", args.strict.to_string()),
            ("scope", quote("supported-mesh-and-fields-subset")),
            ("points", dataset.mesh.points.len().to_string()),
            ("cells", dataset.mesh.cells.len().to_string()),
            ("fields", dataset.fields.len().to_string()),
            (
                "omissions",
                array(omissions.iter().map(|item| quote(&item.detail))),
            ),
        ]))?;
    } else {
        emit(&format!(
            "{} supported-subset checks {}: {} points, {} cells, {} fields, {} omission(s)",
            format.to_uppercase(),
            if passed { "passed" } else { "failed" },
            dataset.mesh.points.len(),
            dataset.mesh.cells.len(),
            dataset.fields.len(),
            omissions.len()
        ))?;
        for omission in omissions {
            emit(&format!("Omission: {}", omission.detail))?;
        }
    }
    Ok(u8::from(!passed))
}
