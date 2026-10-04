//! Guard the assumption boundary before any file is installed.

use super::*;
use crate::core::Dataset;

fn empty_source(format: Format) -> ReadResult {
    ReadResult {
        format,
        dataset: Dataset::default(),
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
