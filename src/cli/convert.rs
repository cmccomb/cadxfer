//! Convert through the public file API, then present the approved receipt.

use std::io::Write;

use caexfer::{ConversionReport, Error, Omission, Result, convert};

use super::approval;
use super::args::Args;
use super::common::{conversion_options, emit, path_json};
use super::json::{array, object, quote};
use super::output::{self, PendingOutput};

/// Convert into private paths so the CLI can confirm losses before install.
pub(super) fn run_convert(args: &Args) -> Result<u8> {
    if args.mesh_out.as_deref() == Some(args.paths[1].as_path()) {
        return Err(Error::new("E_USAGE", "paired outputs need distinct paths"));
    }
    let primary = PendingOutput::new(&args.paths[1], &args.paths[0], args.overwrite)?;
    let companion = args
        .mesh_out
        .as_deref()
        .map(|path| PendingOutput::new(path, &args.paths[0], args.overwrite))
        .transpose()?;
    let mut options = conversion_options(args)?;
    // The library may write every proposed projection into private paths.
    // Only the CLI approval step can install those bytes at requested names.
    options.accept_all = true;
    options.mesh_output = companion
        .as_ref()
        .map(|pending| pending.temporary().to_path_buf());
    let mut report = convert(&args.paths[0], primary.temporary(), &options)?;
    if companion.is_some() && report.mesh_output.is_none() {
        return Err(Error::new(
            "E_OUTPUT",
            "conversion produced no companion report",
        ));
    }
    approval::confirm(
        args,
        &report,
        report
            .mesh_output
            .as_ref()
            .map(|item| item.omissions.as_slice()),
    )?;
    if let Some(companion) = &companion {
        output::install_pair(&primary, companion)?;
        if let Some(receipt) = &mut report.mesh_output {
            receipt.path.clone_from(
                args.mesh_out
                    .as_ref()
                    .ok_or_else(|| Error::new("E_OUTPUT", "missing companion path"))?,
            );
        }
    } else {
        primary.install()?;
    }
    present(args, &report)?;
    Ok(0)
}

/// Render the installed conversion receipt in the selected CLI format.
fn present(args: &Args, report: &ConversionReport) -> Result<()> {
    if args.json {
        let mut fields = vec![
            ("schema_version", "1".into()),
            ("operation", quote("mesh-and-fields-projection")),
            ("output", path_json(&args.paths[1])),
            ("points", report.points.to_string()),
            ("cells", report.cells.to_string()),
            ("fields", report.fields.to_string()),
            ("units", quote("unspecified")),
            ("omissions", omissions_json(&report.omissions)),
        ];
        if let Some(companion) = &report.mesh_output {
            fields.push((
                "mesh_output",
                object([
                    ("path", path_json(&companion.path)),
                    ("format", quote(companion.format.name())),
                    ("omissions", omissions_json(&companion.omissions)),
                ]),
            ));
        }
        emit(&object(fields))?;
    } else {
        emit(&format!(
            "Wrote {}: {} points, {} cells, {} source field(s). Units unspecified.",
            args.paths[1].display(),
            report.points,
            report.cells,
            report.fields
        ))?;
        let mut stderr = std::io::stderr().lock();
        for omission in &report.omissions {
            writeln!(stderr, "Omission: {}", omission.detail)?;
        }
        if let Some(companion) = &report.mesh_output {
            emit(&format!(
                "Wrote companion mesh {}.",
                companion.path.display()
            ))?;
            for omission in &companion.omissions {
                writeln!(stderr, "Companion omission: {}", omission.detail)?;
            }
        }
    }
    Ok(())
}

/// Encode omission stages and details for the versioned CLI JSON schema.
fn omissions_json(omissions: &[Omission]) -> String {
    array(omissions.iter().map(|omission| {
        object([
            ("stage", quote(omission.stage.name())),
            ("acceptance_flag", quote(approval::flag_for(omission))),
            ("detail", quote(&omission.detail)),
        ])
    }))
}
