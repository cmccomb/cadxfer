//! Confirm reported conversion losses and specific inferred values before install.

use std::collections::BTreeSet;
use std::io::{BufRead, IsTerminal, Write};

use caexfer::conversion::{AssumptionKind, ConversionReport, Omission, Stage};
use caexfer::core::{Error, Result};

use super::args::Args;

/// Select the explicit acceptance flag for a report notice.
pub(super) fn flag_for(notice: &Omission) -> &'static str {
    match notice.stage {
        Stage::Source | Stage::Destination => "--accept-omissions",
        Stage::Assumption => match notice.assumption {
            Some(AssumptionKind::BasicFrame) => "--accept-basic-frame",
            Some(AssumptionKind::ZeroRotations) => "--accept-zero-rotations",
            Some(AssumptionKind::SyntheticZero) => "--accept-synthetic-zero",
            None => "--accept-all-approximations-and-infill",
        },
    }
}

/// Check whether the command already accepts one class of reported change.
fn accepted(args: &Args, flag: &str) -> bool {
    args.accept_all_approximations_and_infill
        || match flag {
            "--accept-omissions" => args.accept_omissions,
            "--accept-basic-frame" => args.accept_basic_frame,
            "--accept-zero-rotations" => args.accept_zero_rotations,
            "--accept-synthetic-zero" => args.accept_synthetic_zero,
            _ => false,
        }
}

/// Prompt for the remaining changes after both output receipts are complete.
/// A noninteractive caller receives the same numbered reasons and needed flags.
fn confirm_with(
    args: &Args,
    report: &ConversionReport,
    companion: Option<&ConversionReport>,
    interactive: bool,
    input: &mut impl BufRead,
    warning: &mut impl Write,
) -> Result<()> {
    let mut notices: Vec<(&Omission, bool)> =
        report.omissions.iter().map(|item| (item, false)).collect();
    if let Some(companion) = companion {
        for item in &companion.omissions {
            if !notices.iter().any(|(previous, _)| *previous == item) {
                notices.push((item, true));
            }
        }
    }
    let pending: Vec<_> = notices
        .into_iter()
        .filter(|(item, _)| !accepted(args, flag_for(item)))
        .collect();
    if pending.is_empty() {
        return Ok(());
    }
    let flags: BTreeSet<_> = pending.iter().map(|(item, _)| flag_for(item)).collect();
    let reasons = pending
        .iter()
        .enumerate()
        .map(|(index, (item, companion))| {
            format!(
                "{}. {}{} [{}]",
                index + 1,
                if *companion { "Companion: " } else { "" },
                item.detail,
                flag_for(item)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let flags = flags.into_iter().collect::<Vec<_>>().join(", ");
    let guidance = format!(
        "use {flags} (or --accept-all-approximations-and-infill) to accept these changes without a prompt"
    );
    if !args.json {
        writeln!(warning, "Conversion limitations:\n{reasons}\n{guidance}")?;
    }
    if !interactive || args.json {
        return Err(Error::new(
            "E_USAGE",
            if args.json {
                format!("conversion needs confirmation:\n{reasons}\n{guidance}")
            } else {
                guidance
            },
        ));
    }
    write!(warning, "Continue with these changes? [y/N] ")?;
    warning.flush()?;
    let mut answer = String::new();
    input.read_line(&mut answer)?;
    if matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        Ok(())
    } else {
        Err(Error::new(
            "E_CANCELLED",
            "conversion declined; no output installed",
        ))
    }
}

/// Ask on a terminal or require explicit flags when the input cannot answer.
pub(super) fn confirm(
    args: &Args,
    report: &ConversionReport,
    companion: Option<&ConversionReport>,
) -> Result<()> {
    confirm_with(
        args,
        report,
        companion,
        std::io::stdin().is_terminal(),
        &mut std::io::stdin().lock(),
        &mut std::io::stderr().lock(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use caexfer::conversion::{AssumptionKind, Omission};

    /// A prompt lists the exact flag needed for each unaccepted change.
    #[test]
    fn interactive_answer_accepts_enumerated_changes() {
        let mut report = ConversionReport::default();
        report.omissions.push(Omission::assumed(
            AssumptionKind::ZeroRotations,
            "rotations filled with zero",
        ));
        let mut warning = Vec::new();
        confirm_with(
            &Args::default(),
            &report,
            None,
            true,
            &mut std::io::Cursor::new(b"yes\n"),
            &mut warning,
        )
        .unwrap();
        let warning = String::from_utf8(warning).unwrap();
        assert!(warning.contains("1. rotations filled with zero [--accept-zero-rotations]"));
        assert!(warning.contains("Continue with these changes?"));
    }

    /// An unattended conversion fails before install and names its needed flag.
    #[test]
    fn unattended_conversion_requires_specific_flag() {
        let mut report = ConversionReport::default();
        report.omissions.push(Omission {
            stage: Stage::Source,
            assumption: None,
            detail: "materials omitted".into(),
        });
        let error = confirm_with(
            &Args::default(),
            &report,
            None,
            false,
            &mut std::io::Cursor::new(Vec::<u8>::new()),
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(error.message.contains("--accept-omissions"));
    }

    /// Reject a declined prompt without approving a staged output.
    #[test]
    fn declined_prompt_returns_cancelled() {
        let report = ConversionReport {
            omissions: vec![Omission::assumed(
                AssumptionKind::SyntheticZero,
                "synthetic results",
            )],
            ..ConversionReport::default()
        };
        let error = confirm_with(
            &Args::default(),
            &report,
            None,
            true,
            &mut std::io::Cursor::new(b"no\n"),
            &mut Vec::new(),
        )
        .unwrap_err();
        assert_eq!(error.code, "E_CANCELLED");
    }

    /// A complete conversion needs no flag and never reads an answer.
    #[test]
    fn empty_report_needs_no_confirmation() {
        confirm_with(
            &Args::default(),
            &ConversionReport::default(),
            None,
            false,
            &mut std::io::Cursor::new(Vec::<u8>::new()),
            &mut Vec::new(),
        )
        .unwrap();
    }
}
