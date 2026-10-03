use cadxfer_bdf::{Document, ParseOptions};
use cadxfer_core::CellKind;
use std::fmt::Write as _;

const TRI: &str =
    "GRID,10,,0.,0.,0.\nGRID,20,,1.,0.,0.\nGRID,30,,0.,1.,0.\nCTRIA3,100,7,10,20,30\n";

fn fixed(head: &str, fields: &[&str], width: usize, tail: &str) -> String {
    let capacity = if width == 16 { 4 } else { 8 };
    let mut text = format!("{head:<8}");
    for i in 0..capacity {
        let field = fields.get(i).copied().unwrap_or("");
        text.push_str(&format!("{field:>width$}"));
    }
    text.push_str(&format!("{tail:<8}\n"));
    text
}

#[test]
fn free_grid_and_triangle() {
    let doc = Document::parse(TRI).unwrap();
    let mesh = doc.geometry().unwrap().mesh;
    assert_eq!(mesh.points.len(), 3);
    assert_eq!(mesh.cells[0].kind, CellKind::Triangle3);
    assert_eq!(mesh.cells[0].connectivity, [0, 1, 2]);
    assert_eq!(mesh.cells[0].id, 100);
    assert_eq!(mesh.cells[0].property_id, Some(7));
}

#[test]
fn fixed_small_grid() {
    let source = fixed("GRID", &["7", "", "1.2-3", "2.", "3."], 8, "");
    let doc = Document::parse(&source).unwrap();
    let grid = doc.grids().next().unwrap().unwrap();
    assert_eq!(grid.id, 7);
    assert_eq!(grid.coordinates, [0.0012, 2., 3.]);
    assert_eq!(doc.to_bytes(), source.as_bytes());
}

#[test]
fn fixed_large_grid_continuation() {
    let source = fixed("GRID*", &["7", "", "1.234567890123", "2."], 16, "*A")
        + &fixed("*A", &["3.", "", "", ""], 16, "");
    let doc = Document::parse(&source).unwrap();
    assert_eq!(
        doc.grids().next().unwrap().unwrap().coordinates,
        [1.234567890123, 2., 3.]
    );
}

#[test]
fn large_free_grid() {
    let source = "GRID*,7,,1.5,2.5\n*,3.5\n";
    assert_eq!(
        Document::parse(source)
            .unwrap()
            .grids()
            .next()
            .unwrap()
            .unwrap()
            .coordinates,
        [1.5, 2.5, 3.5]
    );
}

#[test]
fn blank_coordinate_defaults_are_explicit_semantics() {
    let doc = Document::parse("GRID,1\n").unwrap();
    assert_eq!(doc.grids().next().unwrap().unwrap().coordinates, [0.; 3]);
}

#[test]
fn preserves_unknown_cards_and_opaque_comment_bytes() {
    let source = b"$ \xff proprietary comment\r\nMYSTERY,42,foo\r\nGRID,1,,0.,0.,0.\n";
    let doc = Document::parse(source).unwrap();
    assert_eq!(doc.to_bytes(), source);
    assert_eq!(doc.card_counts()["MYSTERY"], 1);
    assert_eq!(doc.geometry().unwrap_err().code, "E_UNSUPPORTED_CARD");
}

#[test]
fn preserves_full_deck_and_trailing_bytes() {
    let source = b"SOL 101\r\nCEND\r\nTITLE = exact title\r\nBEGIN BULK\r\nGRID,1,,0,0,0\r\nENDDATA\r\n\xff trailer";
    let doc = Document::parse(source).unwrap();
    assert!(doc.is_full_deck());
    assert_eq!(doc.cards().len(), 1);
    assert_eq!(doc.to_bytes(), source);
    assert_eq!(doc.geometry().unwrap().mesh.points.len(), 1);
}

#[test]
fn recognizes_comma_begin_bulk() {
    assert!(
        Document::parse("SOL 101\nCEND\nBEGIN,BULK\nGRID,1\nENDDATA\n")
            .unwrap()
            .is_full_deck()
    );
}

#[test]
fn missing_begin_bulk_is_error() {
    assert_eq!(
        Document::parse("SOL 101\nCEND\nGRID,1\n").unwrap_err().code,
        "E_DECK"
    );
}

#[test]
fn multiple_bulk_sections_are_not_merged() {
    assert_eq!(
        Document::parse("BEGIN BULK\nGRID,1\nBEGIN BULK\nGRID,2\n")
            .unwrap_err()
            .code,
        "E_DECK"
    );
}

#[test]
fn final_newline_not_added() {
    let source = "GRID,1,,0.,0.,0.";
    assert_eq!(
        Document::parse(source).unwrap().to_bytes(),
        source.as_bytes()
    );
}

#[test]
fn include_is_preserved_never_opened() {
    let source = "INCLUDE '../../not-opened.bdf'\nGRID,1\n";
    let doc = Document::parse(source).unwrap();
    assert_eq!(doc.cards()[0].name(), "INCLUDE");
    assert_eq!(doc.cards()[0].line, 1);
    assert_eq!(doc.geometry().unwrap_err().code, "E_INCLUDE_UNRESOLVED");
    assert_eq!(doc.to_bytes(), source.as_bytes());
}

#[test]
fn include_in_control_section_blocks_projection() {
    let doc = Document::parse("SOL 101\nINCLUDE 'case.dat'\nCEND\nBEGIN BULK\nGRID,1\n").unwrap();
    assert_eq!(doc.geometry().unwrap_err().code, "E_INCLUDE_UNRESOLVED");
}

#[test]
fn multiline_include_with_dollar_in_filename() {
    let source = "INCLUDE 'path/with$literal/\ncontinued.bdf'\nGRID,1\n";
    let doc = Document::parse(source).unwrap();
    assert_eq!(doc.cards().len(), 2);
    assert_eq!(doc.to_bytes(), source.as_bytes());
}

#[test]
fn unterminated_include_is_error() {
    assert_eq!(
        Document::parse("INCLUDE 'unfinished\n").unwrap_err().code,
        "E_INCLUDE"
    );
}

#[test]
fn ignores_comment_between_continuations() {
    let source = fixed("CHEXA", &["1", "2", "1", "2", "3", "4", "5", "6"], 8, "+A")
        + "$ between\n\n"
        + &fixed("+A", &["7", "8"], 8, "");
    let doc = Document::parse(source).unwrap();
    assert_eq!(doc.cards().len(), 1);
    assert_eq!(doc.card_text(&doc.cards()[0], 9), "8");
}

#[test]
fn blank_head_continuation() {
    let source = fixed("CHEXA", &["1", "2", "1", "2", "3", "4", "5", "6"], 8, "")
        + &fixed("", &["7", "8"], 8, "");
    assert_eq!(Document::parse(source).unwrap().cards().len(), 1);
}

#[test]
fn free_continuation_padding() {
    let source = "UNKNOWN,1,2,3\n+,9\n";
    let doc = Document::parse(source).unwrap();
    assert_eq!(doc.card_text(&doc.cards()[0], 3), "");
    assert_eq!(doc.card_text(&doc.cards()[0], 8), "9");
}

#[test]
fn explicit_free_continuation_label() {
    let source = "CHEXA,1,2,1,2,3,4,5,6,+A\n+A,7,8\n";
    assert_eq!(Document::parse(source).unwrap().cards().len(), 1);
}

#[test]
fn mismatched_label_is_error() {
    let source = fixed("UNKNOWN", &["1"], 8, "+A") + &fixed("+B", &["2"], 8, "");
    assert_eq!(Document::parse(source).unwrap_err().code, "E_CONTINUATION");
}

#[test]
fn dangling_label_is_error() {
    assert_eq!(
        Document::parse(fixed("UNKNOWN", &["1"], 8, "+A"))
            .unwrap_err()
            .code,
        "E_CONTINUATION"
    );
}

#[test]
fn orphan_continuation_is_error() {
    assert_eq!(
        Document::parse("+,1,2\n").unwrap_err().code,
        "E_CONTINUATION"
    );
}

#[test]
fn too_many_free_fields_fail_not_shift() {
    assert_eq!(
        Document::parse("GRID,1,0,0,0,0,0,,0,99\n")
            .unwrap_err()
            .code,
        "E_CONTINUATION"
    );
}

#[test]
fn data_tabs_rejected_comment_tabs_preserved() {
    assert_eq!(
        Document::parse("GRID,1,,\t0,0,0\n").unwrap_err().code,
        "E_TAB"
    );
    assert!(Document::parse("GRID,1,,0,0,0 $ \t comment\n").is_ok());
}

#[test]
fn non_ascii_data_rejected() {
    assert_eq!(
        Document::parse("GRID,1,,π,0,0\n").unwrap_err().code,
        "E_ENCODING"
    );
}

#[test]
fn limits_bytes_lines_cards_and_fields() {
    let base = ParseOptions::default();
    assert_eq!(
        Document::parse_with_options(
            TRI,
            ParseOptions {
                max_bytes: 8,
                ..base
            }
        )
        .unwrap_err()
        .code,
        "E_LIMIT"
    );
    assert_eq!(
        Document::parse_with_options(
            TRI,
            ParseOptions {
                max_line_bytes: 8,
                ..base
            }
        )
        .unwrap_err()
        .code,
        "E_LIMIT"
    );
    assert_eq!(
        Document::parse_with_options(
            TRI,
            ParseOptions {
                max_cards: 1,
                ..base
            }
        )
        .unwrap_err()
        .code,
        "E_LIMIT"
    );
    assert_eq!(
        Document::parse_with_options(
            TRI,
            ParseOptions {
                max_fields_per_card: 1,
                ..base
            }
        )
        .unwrap_err()
        .code,
        "E_LIMIT"
    );
}

#[test]
fn bounded_reader_checks_one_byte_past_limit() {
    let options = ParseOptions {
        max_bytes: 5,
        ..ParseOptions::default()
    };
    assert_eq!(
        Document::read_with_options(TRI.as_bytes(), options)
            .unwrap_err()
            .code,
        "E_LIMIT"
    );
}

#[test]
fn cp_is_never_treated_as_basic_frame() {
    let doc = Document::parse("GRID,1,7,1.,2.,3.\n").unwrap();
    assert_eq!(doc.grids().next().unwrap().unwrap().cp, 7);
    assert_eq!(doc.geometry().unwrap_err().code, "E_COORDINATE_SYSTEM");
}

#[test]
fn grdset_does_not_silently_change_defaults() {
    let doc = Document::parse("GRDSET,,7\nGRID,1\n").unwrap();
    assert_eq!(doc.geometry().unwrap_err().code, "E_GRDSET");
}

#[test]
fn duplicate_grid_not_overwritten() {
    let doc = Document::parse("GRID,1,,0,0,0\nGRID,1,,1,1,1\n").unwrap();
    assert_eq!(doc.geometry().unwrap_err().code, "E_DUPLICATE_GRID");
}

#[test]
fn duplicate_element_not_overwritten() {
    let doc = Document::parse(format!("{TRI}CTRIA3,100,8,10,20,30\n")).unwrap();
    assert_eq!(doc.geometry().unwrap_err().code, "E_DUPLICATE_ELEMENT");
}

#[test]
fn dangling_reference_reports_line() {
    let doc = Document::parse("GRID,1\nCROD,10,7,1,999\n").unwrap();
    let error = doc.geometry().unwrap_err();
    assert_eq!(error.code, "E_MISSING_GRID");
    assert_eq!(error.line, Some(2));
}

#[test]
fn repeated_connectivity_rejected() {
    let doc = Document::parse("GRID,1\nCROD,10,7,1,1\n").unwrap();
    assert_eq!(
        doc.geometry().unwrap_err().code,
        "E_DEGENERATE_CONNECTIVITY"
    );
}

#[test]
fn higher_order_tet_is_not_truncated() {
    let doc = Document::parse("GRID,1\nCTETRA,10,7,1,2,3,4,5,6\n+,7,8,9,10\n").unwrap();
    assert_eq!(doc.geometry().unwrap_err().code, "E_HIGH_ORDER");
}

#[test]
fn unknown_geometry_not_silently_skipped() {
    let doc = Document::parse("GRID,1\nCBUSH,10,7,1,2\n").unwrap();
    assert_eq!(doc.geometry().unwrap_err().code, "E_UNSUPPORTED_CARD");
}

#[test]
fn materials_are_opaque_warnings_not_false_validation() {
    let doc = Document::parse(format!("{TRI}MAT1,7,not-a-number\n")).unwrap();
    let report = doc.validate_geometry();
    assert!(report.valid_in_scope());
    assert_eq!(report.warning_count(), 1);
    assert!(doc
        .geometry()
        .unwrap()
        .omissions
        .iter()
        .any(|o| o.category == "MAT1"));
}

#[test]
fn empty_is_roundtrippable_not_projectable() {
    let doc = Document::parse(b"").unwrap();
    assert!(doc.to_bytes().is_empty());
    assert_eq!(doc.geometry().unwrap_err().code, "E_EMPTY_GEOMETRY");
}

#[test]
fn deterministic_id_sorting_preserves_connectivity() {
    let doc =
        Document::parse("GRID,30,,0,1,0\nGRID,10,,0,0,0\nGRID,20,,1,0,0\nCTRIA3,100,7,30,10,20\n")
            .unwrap();
    let mesh = doc.geometry().unwrap().mesh;
    assert_eq!(
        mesh.points.iter().map(|p| p.id).collect::<Vec<_>>(),
        [10, 20, 30]
    );
    assert_eq!(mesh.cells[0].connectivity, [2, 0, 1]);
}

#[test]
fn all_linear_cell_families() {
    let mut nodes = String::new();
    for id in 1..=8 {
        writeln!(&mut nodes, "GRID,{id},,{id},0,0").unwrap();
    }
    let source = nodes + "CROD,1,7,1,2\nCONROD,2,1,2,8,1.\nCBAR,3,7,1,2,0.,1.,0.\nCBEAM,4,7,1,2,0.,1.,0.\nCTRIA3,5,7,1,2,3\nCQUAD4,6,7,1,2,3,4\nCTETRA,7,7,1,2,3,4\nCHEXA,8,7,1,2,3,4,5,6\n+,7,8\nCPENTA,9,7,1,2,3,4,5,6\nCPYRAM,10,7,1,2,3,4,5\n";
    let mesh = Document::parse(source).unwrap().geometry().unwrap().mesh;
    assert_eq!(mesh.cells.len(), 10);
    assert_eq!(mesh.cells[1].property_id, None);
    assert_eq!(mesh.cells[7].connectivity, [0, 1, 2, 3, 4, 5, 6, 7]);
    assert_eq!(mesh.cells[8].kind, CellKind::Wedge6);
}

#[test]
fn free_edit_changes_only_coordinate_tokens() {
    let source = b"$ header\r\nGRID, 1 , , 0. , 2. , 3. ,0,,0 $ note\r\nMYSTERY,keep\n";
    let mut doc = Document::parse(source).unwrap();
    doc.set_grid_coordinates(1, [10., -2., 0.125]).unwrap();
    assert_eq!(
        doc.to_bytes(),
        b"$ header\r\nGRID, 1 , , 10. , -2. , 0.125 ,0,,0 $ note\r\nMYSTERY,keep\n"
    );
}

#[test]
fn fixed_edit_preserves_card_width_and_comment() {
    let source = fixed("GRID", &["1", "", "0.", "0.", "0."], 8, "") + "$ keep\n";
    let mut doc = Document::parse(&source).unwrap();
    doc.set_grid_coordinates(1, [1., 2., 3.]).unwrap();
    assert_eq!(doc.to_bytes().len(), source.len());
    assert_eq!(&doc.to_bytes()[..24], &source.as_bytes()[..24]);
    assert_eq!(&doc.to_bytes()[48..], &source.as_bytes()[48..]);
}

#[test]
fn failed_edit_is_transactional() {
    let source = fixed("GRID", &["1", "", "0.", "0.", "0."], 8, "");
    let mut doc = Document::parse(&source).unwrap();
    assert_eq!(
        doc.set_grid_coordinates(1, [1., std::f64::consts::PI, 3.])
            .unwrap_err()
            .code,
        "E_FIELD_WIDTH"
    );
    assert_eq!(doc.to_bytes(), source.as_bytes());
}

#[test]
fn edit_can_be_repeated_without_stale_spans() {
    let mut doc = Document::parse("GRID,1,,0.,0.,0.\n").unwrap();
    doc.set_grid_coordinates(1, [123456., 0., 0.]).unwrap();
    doc.set_grid_coordinates(1, [1., 2., 3.]).unwrap();
    assert_eq!(
        doc.grids().next().unwrap().unwrap().coordinates,
        [1., 2., 3.]
    );
}

#[test]
fn edit_uses_native_not_basic_frame() {
    let mut doc = Document::parse("GRID,1,42,0.,0.,0.\n").unwrap();
    doc.set_grid_coordinates(1, [1., 2., 3.]).unwrap();
    assert!(String::from_utf8_lossy(doc.to_bytes()).starts_with("GRID,1,42,"));
}

#[test]
fn refuses_implicit_field_edits() {
    let mut doc = Document::parse("GRID,1\n").unwrap();
    assert_eq!(
        doc.set_grid_coordinates(1, [0.; 3]).unwrap_err().code,
        "E_IMPLICIT_FIELD"
    );
}

#[test]
fn write_propagates_failures() {
    struct Broken;
    impl std::io::Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("fail"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    assert!(Document::parse(TRI).unwrap().write_to(Broken).is_err());
}

#[test]
fn all_single_byte_inputs_do_not_panic_and_success_roundtrips() {
    for byte in 0..=255u8 {
        let input = [byte];
        if let Ok(doc) = Document::parse(input) {
            assert_eq!(doc.to_bytes(), input);
            let _ = doc.validate_geometry();
        }
    }
}

#[test]
fn deterministic_random_corpus_roundtrip_and_no_panic() {
    // Small deterministic property smoke test, not a substitute for fuzzing.
    let mut state = 0x12345678u64;
    for len in 0..256 {
        let mut input = Vec::new();
        for _ in 0..len {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            input.push((state >> 32) as u8);
        }
        if let Ok(doc) = Document::parse(&input) {
            assert_eq!(doc.to_bytes(), input);
            let _ = doc.validate_geometry();
        }
    }
}

#[test]
fn typed_grid_does_not_guess_grdset_defaults() {
    let doc = Document::parse("GRID,1\nGRDSET,,7\n").unwrap();
    assert_eq!(doc.grids().next().unwrap().unwrap_err().code, "E_GRDSET");
}

#[test]
fn post_enddata_marker_is_opaque() {
    let source = "GRID,1\nENDDATA\nSOL 101\n";
    let doc = Document::parse(source).unwrap();
    assert!(!doc.is_full_deck());
    assert_eq!(doc.to_bytes(), source.as_bytes());
}

#[test]
fn line_count_limit_is_enforced() {
    let options = ParseOptions {
        max_lines: 2,
        ..ParseOptions::default()
    };
    assert_eq!(
        Document::parse_with_options("\n\n\n", options)
            .unwrap_err()
            .code,
        "E_LIMIT"
    );
}
