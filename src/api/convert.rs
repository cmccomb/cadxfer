//! Staged file conversion with explicit acceptance of projected changes.

use std::fs::File;
use std::path::Path;

use crate::conversion::{
    self, AssumptionKind, CompanionReport, ConversionReport, Format, Omission, Options, ReadResult,
    Stage,
};
use crate::core::{Error, Field, FieldLocation, Result};
use crate::file_output;
use crate::formats::bdf;

/// Add an explicitly proposed all-zero displacement table to a result-free deck.
fn propose_synthetic_zero(input: &Path, source: &mut ReadResult, max_bytes: usize) -> Result<()> {
    if !matches!(source.format, Format::Bdf | Format::Inp) || !source.dataset.fields.is_empty() {
        return Err(Error::new(
            "E_OP2",
            "synthetic OP2 output requires a result-free BDF or INP",
        ));
    }
    if source.format == Format::Bdf
        && bdf::mesh::read_from(File::open(input)?, max_bytes)?.has_nonbasic_output_frame
    {
        return Err(Error::new(
            "E_OP2",
            "synthetic OP2 output requires basic-frame GRID CD=0",
        ));
    }
    let count = source
        .dataset
        .mesh
        .points
        .len()
        .checked_mul(6)
        .ok_or_else(|| Error::new("E_LIMIT", "too many nodes for synthetic displacement"))?;
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

/// Refuse to install staged output until every reported change was accepted.
fn check_acceptance(report: &ConversionReport, options: &Options) -> Result<()> {
    let pending = report
        .omissions
        .iter()
        .filter_map(|item| {
            let (accepted, option) = match item.stage {
                Stage::Source | Stage::Destination => {
                    (options.accept_omissions, "accept_omissions")
                }
                Stage::Assumption => match item.assumption {
                    Some(AssumptionKind::BasicFrame) => {
                        (options.assume_basic_frame, "assume_basic_frame")
                    }
                    Some(AssumptionKind::ZeroRotations) => {
                        (options.zero_missing_rotations, "zero_missing_rotations")
                    }
                    Some(AssumptionKind::SyntheticZero) => {
                        (options.accept_synthetic_zero, "accept_synthetic_zero")
                    }
                    None => (false, "accept_all"),
                },
            };
            (!options.accept_all && !accepted).then(|| format!("{} [{}]", item.detail, option))
        })
        .collect::<Vec<_>>();
    if pending.is_empty() {
        Ok(())
    } else {
        Err(Error::new(
            "E_USAGE",
            format!(
                "conversion needs acceptance before output installation:\n{}",
                pending.join("\n")
            ),
        ))
    }
}

/// Convert one source file into a new destination file.
///
/// Input and output formats are inferred from extensions unless `input_format`
/// overrides the source. Files are staged and synced before an atomic
/// no-clobber install. Reported losses and assumptions need explicit acceptance
/// through [`Options`], just as unattended CLI conversions do. OP2 may also
/// write a staged companion mesh through `options.mesh_output`.
///
/// # Errors
///
/// Returns a source, projection, acceptance, or output error. No destination
/// is installed when conversion or acceptance fails. Paired installation can
/// leave the first installed file if installing the second fails.
pub fn convert(
    input: impl AsRef<Path>,
    output: impl AsRef<Path>,
    options: &Options,
) -> Result<ConversionReport> {
    let input = input.as_ref();
    let output = output.as_ref();
    let target = Format::from_output_path(output)?;
    if options.mesh_output.is_some() && target != Format::Op2 {
        return Err(Error::new(
            "E_USAGE",
            "mesh_output applies only to OP2 output",
        ));
    }
    let mut engine_options = options.clone();
    let source_format = engine_options
        .input_format
        .map_or_else(|| Format::from_input_path(input), Ok)?;
    if matches!(source_format, Format::Op2 | Format::Pch)
        && options.mesh.as_ref().is_some_and(|path| {
            Format::from_input_path(path).is_ok_and(|format| format != Format::Bdf)
        })
    {
        engine_options.assume_basic_frame = true;
    }
    if target == Format::Op2 {
        engine_options.zero_missing_rotations = true;
    }
    let mut source = conversion::read_path(input, &engine_options)?;
    if target == Format::Op2
        && matches!(source.format, Format::Bdf | Format::Inp)
        && source.dataset.fields.is_empty()
    {
        propose_synthetic_zero(input, &mut source, options.max_bytes)?;
    }

    let mut report = None;
    if let Some(companion_path) = &options.mesh_output {
        let companion_format = Format::from_output_path(companion_path)?;
        if companion_format == Format::Op2 {
            return Err(Error::new(
                "E_USAGE",
                "mesh_output needs a mesh-bearing format",
            ));
        }
        let mut companion_source = source.clone();
        let excluded = companion_source.dataset.fields.len();
        companion_source.dataset.fields.clear();
        if excluded > 0 {
            companion_source.omissions.push(Omission {
                stage: Stage::Destination,
                assumption: None,
                detail: format!("{excluded} result field(s) excluded from companion mesh"),
            });
        }
        let mut companion_report = None;
        file_output::create_pair(
            output,
            |writer| {
                let produced = conversion::convert(source, target, &engine_options, writer)?;
                check_acceptance(&produced, options)?;
                report = Some(produced);
                Ok(())
            },
            companion_path,
            |writer| {
                let produced = conversion::convert(
                    companion_source,
                    companion_format,
                    &engine_options,
                    writer,
                )?;
                check_acceptance(&produced, options)?;
                companion_report = Some(produced);
                Ok(())
            },
        )?;
        let companion =
            companion_report.ok_or_else(|| Error::new("E_OUTPUT", "no companion report"))?;
        report
            .as_mut()
            .ok_or_else(|| Error::new("E_OUTPUT", "no conversion report"))?
            .mesh_output = Some(CompanionReport {
            path: companion_path.clone(),
            format: companion_format,
            omissions: companion.omissions,
        });
    } else {
        file_output::create_new(output, |writer| {
            let produced = conversion::convert(source, target, &engine_options, writer)?;
            check_acceptance(&produced, options)?;
            report = Some(produced);
            Ok(())
        })?;
    }
    report.ok_or_else(|| Error::new("E_OUTPUT", "no conversion report"))
}

#[cfg(test)]
mod tests {
    //! Guard the assumption boundary before any file is installed.

    use super::*;
    use crate::core::Dataset;

    fn empty_source(format: Format) -> ReadResult {
        ReadResult {
            format,
            dataset: Dataset::default(),
            voxel_grid: None,
            omissions: Vec::new(),
            generated_point_ids: false,
            assumed_zero: false,
        }
    }

    #[test]
    fn synthetic_zero_requires_a_result_free_deck() {
        let mut non_deck = empty_source(Format::Vtu);
        let before = non_deck.clone();
        assert_eq!(
            propose_synthetic_zero(Path::new("unused"), &mut non_deck, 1)
                .unwrap_err()
                .code,
            "E_OP2"
        );
        assert_eq!(non_deck, before);

        let mut with_results = empty_source(Format::Bdf);
        with_results.dataset.fields.push(Field {
            name: "DISP".into(),
            location: FieldLocation::Point,
            components: vec!["T1".into()],
            values: Vec::new(),
            step: None,
            time: None,
        });
        let before = with_results.clone();
        assert_eq!(
            propose_synthetic_zero(Path::new("unused"), &mut with_results, 1)
                .unwrap_err()
                .code,
            "E_OP2"
        );
        assert_eq!(with_results, before);
    }

    #[test]
    fn unclassified_assumption_requires_explicit_catchall() {
        let report = ConversionReport {
            omissions: vec![Omission {
                stage: Stage::Assumption,
                assumption: None,
                detail: "unclassified assumption".into(),
            }],
            ..ConversionReport::default()
        };
        let error = check_acceptance(&report, &Options::default()).unwrap_err();
        assert_eq!(error.code, "E_USAGE");
        assert!(error.message.contains("accept_all"));
        assert!(
            check_acceptance(
                &report,
                &Options {
                    accept_all: true,
                    ..Options::default()
                }
            )
            .is_ok()
        );
    }
}
