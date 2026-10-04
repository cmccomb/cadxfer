//! Validate a projected dataset and show its counts and source omissions.

use caexfer::{Result, validate};

use super::args::Args;
use super::common::{conversion_options, emit, path_json};
use super::json::{array, object, quote};

/// Validate a source through the format reader and report projected counts.
/// Strict validation fails when the supported projection reports omissions.
pub(super) fn run_validate(args: &Args) -> Result<u8> {
    let report = validate(&args.paths[0], &conversion_options(args)?)?;
    let format = report.format.name();
    if args.json {
        emit(&object([
            ("schema_version", "1".into()),
            ("format", quote(format)),
            ("path", path_json(&args.paths[0])),
            ("passed", report.passed.to_string()),
            ("strict", report.strict.to_string()),
            ("scope", quote("supported-mesh-and-fields-subset")),
            ("points", report.points.to_string()),
            ("cells", report.cells.to_string()),
            ("fields", report.fields.to_string()),
            (
                "omissions",
                array(report.omissions.iter().map(|item| quote(&item.detail))),
            ),
        ]))?;
    } else {
        emit(&format!(
            "{} supported-subset checks {}: {} points, {} cells, {} fields, {} omission(s)",
            format.to_uppercase(),
            if report.passed { "passed" } else { "failed" },
            report.points,
            report.cells,
            report.fields,
            report.omissions.len()
        ))?;
        for omission in &report.omissions {
            emit(&format!("Omission: {}", omission.detail))?;
        }
    }
    Ok(u8::from(!report.passed))
}
