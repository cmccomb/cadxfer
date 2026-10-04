//! Convert datasets and report installed output and omitted data.

use std::fs::File;
use std::io::Write;

use caexfer::conversion::{self, Format, Omission, ReadResult, Stage};
use caexfer::core::{Error, Field, FieldLocation, Result};
use caexfer::formats::bdf;

use super::args::{Args, usage};
use super::common::{conversion_options, emit, path_json};
use super::json::{array, object, quote};
use super::output;

/// Add an explicitly synthetic six-component zero field to a geometry source.
/// Preserve its assumption in the conversion report and OP2 provenance title.
fn assume_zero_displacement(args: &Args, source: &mut ReadResult, max_bytes: usize) -> Result<()> {
    // Synthetic displacement is an explicit geometry-to-result assumption,
    // limited to sources that carry no numeric results of their own.
    if !matches!(source.format, Format::Bdf | Format::Inp) {
        return Err(usage(
            "--assume-zero-displacement currently accepts BDF or INP input",
        ));
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
    source.omissions.push(Omission {
        stage: Stage::Assumption,
        detail: "SYNTHETIC ASSUMPTION: all six displacement components set to float 0.0 for every node; no solver analysis was performed".into(),
    });
    source.assumed_zero = true;
    Ok(())
}

/// Convert a source into a staged destination and optional companion mesh.
/// The library reports omissions; this layer owns no-clobber file installation
/// and human or JSON presentation of the result.
pub(super) fn run_convert(args: &Args) -> Result<u8> {
    // Read and project before staging output, so source failures leave no
    // destination path behind.
    let target = Format::from_output_path(&args.paths[1])?;
    let options = conversion_options(args)?;
    let mut source = conversion::read_path(&args.paths[0], &options)?;
    if args.assume_zero_displacement {
        assume_zero_displacement(args, &mut source, options.max_bytes)?;
    }
    let mut report = None;
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
                detail: format!("{excluded} result field(s) excluded from companion mesh"),
            });
        }
        let mut mesh_report = None;
        output::create_pair(
            &args.paths[1],
            |writer| {
                report = Some(conversion::convert(source, target, &options, writer)?);
                Ok(())
            },
            path,
            |writer| {
                mesh_report = Some(conversion::convert(mesh_source, format, &options, writer)?);
                Ok(())
            },
        )?;
        Some((
            path,
            format,
            mesh_report
                .ok_or_else(|| Error::new("E_OUTPUT", "companion mesh produced no report"))?,
        ))
    } else {
        output::create_new(&args.paths[1], |writer| {
            report = Some(conversion::convert(source, target, &options, writer)?);
            Ok(())
        })?;
        None
    };

    // Present the same source/destination omission report in either output
    // mode after successful file installation.
    let report = report.ok_or_else(|| Error::new("E_OUTPUT", "conversion produced no report"))?;
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
        if let Some((path, format, mesh_report)) = &mesh_output {
            fields.push((
                "mesh_output",
                object([
                    ("path", path_json(path)),
                    ("format", quote(format.name())),
                    ("omissions", omissions_json(&mesh_report.omissions)),
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
        if let Some((path, _, mesh_report)) = mesh_output {
            emit(&format!("Wrote companion mesh {}.", path.display()))?;
            for omission in mesh_report.omissions {
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
            ("category", quote("source-or-destination")),
            ("stage", quote(omission.stage.name())),
            ("count", "1".into()),
            ("detail", quote(&omission.detail)),
        ])
    }))
}
