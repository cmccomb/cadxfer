//! Convert datasets and report installed output and omitted data.

use std::cell::OnceCell;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

use caexfer::conversion::{
    self, AssumptionKind, ConversionReport, Format, Omission, Options, ReadResult, Stage,
};
use caexfer::core::{Error, Field, FieldLocation, Result};
use caexfer::formats::bdf;

use super::approval;
use super::args::{Args, usage};
use super::common::{conversion_options, emit, path_json};
use super::json::{array, object, quote};
use super::output;

/// Add a synthetic six-component zero field to a geometry source.
/// Preserve its assumption in the conversion report and OP2 provenance title.
fn assume_zero_displacement(args: &Args, source: &mut ReadResult, max_bytes: usize) -> Result<()> {
    // Synthetic displacement is an explicit geometry-to-result assumption,
    // limited to sources that carry no numeric results of their own.
    if !matches!(source.format, Format::Bdf | Format::Inp) {
        return Err(usage("synthetic zero OP2 output requires BDF or INP input"));
    }
    if !source.dataset.fields.is_empty() {
        return Err(Error::new(
            "E_OP2",
            "zero-displacement assumption requires a result-free input",
        ));
    }
    if source.format == Format::Bdf {
        // OP2 rereads against the BDF mesh, so its GRID output frame must be
        // basic even though the synthetic displacement values are all zero.
        if bdf::mesh::read_from(File::open(&args.paths[0])?, max_bytes)?.has_nonbasic_output_frame {
            return Err(Error::new(
                "E_OP2",
                "synthetic OP2 output requires basic-frame GRID CD=0 for matching BDF reread",
            ));
        }
    }
    let count = source
        .dataset
        .mesh
        .points
        .len()
        .checked_mul(6)
        .ok_or_else(|| Error::new("E_LIMIT", "too many nodes for synthetic displacement"))?;

    // Record provenance alongside the values for both report and OP2 title.
    source.dataset.fields.push(Field {
        name: "DISP".into(),
        location: FieldLocation::Point,
        components: ["T1", "T2", "T3", "R1", "R2", "R3"]
            .map(str::to_owned)
            .to_vec(),
        values: vec![0.0; count],
        step: None,
        time: None,
    });
    source.omissions.push(Omission::assumed(
        AssumptionKind::SyntheticZero,
        "SYNTHETIC ASSUMPTION: all six displacement components set to float 0.0 for every node; no solver analysis was performed",
    ));
    source.assumed_zero = true;
    Ok(())
}

/// Companion mesh output and its separate conversion receipt.
struct CompanionReceipt {
    path: PathBuf,
    format: Format,
    report: ConversionReport,
}

/// Receipts for output installed after its limitations were accepted.
struct Installed {
    report: ConversionReport,
    companion: Option<CompanionReceipt>,
}

/// Stage converted output, confirm its receipt, then install the destination.
/// Paired OP2 and mesh outputs are both written before confirmation.
fn stage_conversion(
    args: &Args,
    source: ReadResult,
    target: Format,
    options: &Options,
) -> Result<Installed> {
    let report = OnceCell::new();
    let mesh_output = if let Some(path) = &args.mesh_out {
        // OP2 has no embedded mesh; stage the result and its companion
        // together before installing either final path.
        let format = Format::from_output_path(path)?;
        if format == Format::Op2 {
            return Err(usage("--mesh-out requires a mesh-bearing output format"));
        }
        let mut mesh_source = source.clone();
        let excluded = mesh_source.dataset.fields.len();

        // The companion is geometry-only, with excluded fields reported.
        mesh_source.dataset.fields.clear();
        if excluded > 0 {
            mesh_source.omissions.push(Omission {
                stage: Stage::Destination,
                assumption: None,
                detail: format!("{excluded} result field(s) excluded from companion mesh"),
            });
        }
        let mesh_report = OnceCell::new();
        output::create_pair(
            &args.paths[1],
            |writer| {
                report
                    .set(conversion::convert(source, target, options, writer)?)
                    .map_err(|_| Error::new("E_OUTPUT", "duplicate conversion report"))?;
                Ok(())
            },
            path,
            |writer| {
                mesh_report
                    .set(conversion::convert(mesh_source, format, options, writer)?)
                    .map_err(|_| Error::new("E_OUTPUT", "duplicate companion report"))?;
                approval::confirm(
                    args,
                    report
                        .get()
                        .ok_or_else(|| Error::new("E_OUTPUT", "conversion produced no report"))?,
                    mesh_report.get(),
                )
            },
        )?;
        Some(CompanionReceipt {
            path: path.clone(),
            format,
            report: mesh_report
                .into_inner()
                .ok_or_else(|| Error::new("E_OUTPUT", "companion mesh produced no report"))?,
        })
    } else {
        output::create_new(&args.paths[1], |writer| {
            let produced = conversion::convert(source, target, options, writer)?;
            approval::confirm(args, &produced, None)?;
            report
                .set(produced)
                .map_err(|_| Error::new("E_OUTPUT", "duplicate conversion report"))
        })?;
        None
    };

    let report = report
        .into_inner()
        .ok_or_else(|| Error::new("E_OUTPUT", "conversion produced no report"))?;
    Ok(Installed {
        report,
        companion: mesh_output,
    })
}

/// Convert a source into a staged destination and optional companion mesh.
/// The library reports omissions; this layer owns no-clobber file installation
/// and human or JSON presentation of the result.
pub(super) fn run_convert(args: &Args) -> Result<u8> {
    // Read and project before staging output, so source failures leave no
    // destination path behind.
    let target = Format::from_output_path(&args.paths[1])?;
    let mut options = conversion_options(args)?;
    // A non-BDF companion cannot prove the Nastran result frame. Carry the
    // proposed basic-frame assertion into the receipt for confirmation.
    if matches!(options.input_format, Some(Format::Op2 | Format::Pch))
        && args.mesh.as_ref().is_some_and(|path| {
            Format::from_input_path(path).is_ok_and(|format| format != Format::Bdf)
        })
    {
        options.assume_basic_frame = true;
    }
    // The OP2 writer reports a zero-rotation infill only when its selected
    // displacement field actually has three components.
    if target == Format::Op2 {
        options.zero_missing_rotations = true;
    }
    let mut source = conversion::read_path(&args.paths[0], &options)?;
    if target == Format::Op2
        && matches!(source.format, Format::Bdf | Format::Inp)
        && source.dataset.fields.is_empty()
    {
        assume_zero_displacement(args, &mut source, options.max_bytes)?;
    }
    let Installed { report, companion } = stage_conversion(args, source, target, &options)?;
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
        if let Some(CompanionReceipt {
            path,
            format,
            report,
        }) = &companion
        {
            fields.push((
                "mesh_output",
                object([
                    ("path", path_json(path)),
                    ("format", quote(format.name())),
                    ("omissions", omissions_json(&report.omissions)),
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
        for omission in report.omissions {
            writeln!(stderr, "Omission: {}", omission.detail)?;
        }
        if let Some(CompanionReceipt { path, report, .. }) = companion {
            emit(&format!("Wrote companion mesh {}.", path.display()))?;
            for omission in report.omissions {
                writeln!(stderr, "Companion omission: {}", omission.detail)?;
            }
        }
    }
    Ok(0)
}

/// Encode omission stages and details for the versioned CLI JSON schema.
fn omissions_json(omissions: &[Omission]) -> String {
    // Each omission is one report row with its originating conversion stage.
    array(omissions.iter().map(|omission| {
        object([
            ("stage", quote(omission.stage.name())),
            ("acceptance_flag", quote(approval::flag_for(omission))),
            ("detail", quote(&omission.detail)),
        ])
    }))
}
