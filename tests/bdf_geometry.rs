use caexfer::bdf;
use caexfer::core::CellKind;
use std::fmt::Write as _;

const TRI: &str =
    "GRID,10,,0.,0.,0.\nGRID,20,,1.,0.,0.\nGRID,30,,0.,1.,0.\nCTRIA3,100,7,10,20,30\n";

fn fixed(head: &str, fields: &[&str], width: usize, tail: &str) -> String {
    let capacity = if width == 16 { 4 } else { 8 };
    let mut text = format!("{head:<8}");
    for index in 0..capacity {
        write!(text, "{:>width$}", fields.get(index).copied().unwrap_or("")).unwrap();
    }
    writeln!(text, "{tail:<8}").unwrap();
    text
}

#[test]
fn free_and_fixed_geometry_keep_ids_and_coordinates() {
    let projection = bdf::mesh::read(TRI).unwrap();
    assert_eq!(
        projection
            .mesh
            .points
            .iter()
            .map(|point| point.id)
            .collect::<Vec<_>>(),
        [10, 20, 30]
    );
    assert_eq!(projection.mesh.cells[0].kind, CellKind::Triangle3);
    assert_eq!(projection.mesh.cells[0].connectivity, [0, 1, 2]);
    assert_eq!(projection.mesh.cells[0].property_id, Some(7));

    let small = fixed("GRID", &["7", "", "1.2-3", "2.", "3."], 8, "");
    assert_eq!(
        bdf::mesh::read(small).unwrap().mesh.points[0].position,
        [0.0012, 2.0, 3.0]
    );
    let large = fixed("GRID*", &["7", "", "1.234567890123", "2."], 16, "*A")
        + &fixed("*A", &["3.", "", "", ""], 16, "");
    assert_eq!(
        bdf::mesh::read(large).unwrap().mesh.points[0].position,
        [1.234_567_890_123, 2.0, 3.0]
    );
}

#[test]
fn full_decks_and_continuations_are_bounded() {
    let full = "SOL 101\nCEND\nBEGIN,BULK\nGRID,1\nENDDATA\n";
    assert_eq!(bdf::mesh::read(full).unwrap().mesh.points.len(), 1);
    assert_eq!(
        bdf::mesh::read("SOL 101\nCEND\nGRID,1\n").unwrap_err().code,
        "E_DECK"
    );
    assert_eq!(
        bdf::mesh::read("BEGIN BULK\nGRID,1\nBEGIN BULK\nGRID,2\n")
            .unwrap_err()
            .code,
        "E_DECK"
    );
    assert_eq!(
        bdf::mesh::read("+,1,2\n").unwrap_err().code,
        "E_CONTINUATION"
    );
    let mismatch = fixed("GRID", &["1"], 8, "+A") + &fixed("+B", &["2"], 8, "");
    assert_eq!(
        bdf::mesh::read(mismatch).unwrap_err().code,
        "E_CONTINUATION"
    );
    assert_eq!(
        bdf::mesh::read("INCLUDE 'unfinished\n").unwrap_err().code,
        "E_INCLUDE"
    );
}

#[test]
fn unsupported_geometry_and_frames_fail() {
    for (source, code) in [
        ("GRID,1,7,1.,2.,3.\n", "E_COORDINATE_SYSTEM"),
        ("GRDSET,,7\nGRID,1\n", "E_GRDSET"),
        ("INCLUDE 'other.bdf'\nGRID,1\n", "E_INCLUDE_UNRESOLVED"),
        ("GRID,1\nCBUSH,10,7,1,2\n", "E_UNSUPPORTED_CARD"),
        (
            "GRID,1\nCTETRA,10,7,1,2,3,4,5,6\n+,7,8,9,10\n",
            "E_HIGH_ORDER",
        ),
    ] {
        assert_eq!(bdf::mesh::read(source).unwrap_err().code, code, "{source}");
    }
}

#[test]
fn malformed_ids_and_connectivity_fail_without_partial_mesh() {
    for (source, code) in [
        ("GRID,1,,0,0,0\nGRID,1,,1,1,1\n", "E_DUPLICATE_GRID"),
        ("GRID,1\nCROD,10,7,1,999\n", "E_MISSING_GRID"),
        ("GRID,1\nCROD,10,7,1,1\n", "E_DEGENERATE_CONNECTIVITY"),
        ("GRID,1,,not-a-real,0,0\n", "E_REAL"),
        ("GRID,1,,\t0,0,0\n", "E_TAB"),
        ("GRID,1,,π,0,0\n", "E_ENCODING"),
        ("GRID,0\n", "E_INTEGER"),
        ("GRID,1,,0,0,0,-2\n", "E_INTEGER"),
        ("GRID,1,,0,0,0,,11\n", "E_GRID_PS"),
        ("GRID,1,,0,0,0,,,0\n+,extra\n", "E_GRID_FIELDS"),
        ("GRID,1,,0,0,0,,,x\n", "E_INTEGER"),
        (
            "GRID,1,,0,0,0,,,,\nGRID,2\nCROD,10,7,1,2\nCROD,10,7,1,2\n",
            "E_DUPLICATE_ELEMENT",
        ),
        ("GRID,1,,0,0,0,,,1\n", "E_SUPERELEMENT"),
    ] {
        assert_eq!(bdf::mesh::read(source).unwrap_err().code, code, "{source}");
    }
    assert_eq!(
        bdf::mesh::read("GRID,1\nCROD,10,7,1,999\n")
            .unwrap_err()
            .line,
        Some(2)
    );
}

#[test]
fn opaque_solver_cards_are_reported() {
    let projection = bdf::mesh::read(format!("{TRI}MAT1,7,not-a-number\n")).unwrap();
    assert!(
        projection
            .omissions
            .iter()
            .any(|item| item.category == "MAT1")
    );
    assert!(!projection.has_nonbasic_output_frame);
    let nonbasic = bdf::mesh::read("GRID,1,,0,0,0,7\n").unwrap();
    assert!(nonbasic.has_nonbasic_output_frame);
}

#[test]
fn byte_limit_and_writer_are_explicit() {
    assert_eq!(
        bdf::mesh::read_from(TRI.as_bytes(), 5).unwrap_err().code,
        "E_LIMIT"
    );
    let projection = bdf::mesh::read(TRI).unwrap();
    let mut output = Vec::new();
    bdf::mesh::write(&projection.mesh, &mut output).unwrap();
    assert_eq!(bdf::mesh::read(&output).unwrap().mesh.cells[0].id, 100);
    assert_ne!(output, TRI.as_bytes());
}

#[test]
fn deterministic_ids_and_all_linear_families() {
    let mut nodes = String::new();
    for id in (1..=8).rev() {
        writeln!(&mut nodes, "GRID,{id},,{id},0,0").unwrap();
    }
    let source = nodes
        + "CROD,1,7,1,2\nCONROD,2,1,2,8,1.\nCBAR,3,7,1,2,0.,1.,0.\nCBEAM,4,7,1,2,0.,1.,0.\nCTRIA3,5,7,1,2,3\nCQUAD4,6,7,1,2,3,4\nCTETRA,7,7,1,2,3,4\nCHEXA,8,7,1,2,3,4,5,6\n+,7,8\nCPENTA,9,7,1,2,3,4,5,6\nCPYRAM,10,7,1,2,3,4,5\n";
    let mesh = bdf::mesh::read(source).unwrap().mesh;
    assert_eq!(
        mesh.points.iter().map(|point| point.id).collect::<Vec<_>>(),
        [1, 2, 3, 4, 5, 6, 7, 8]
    );
    assert_eq!(mesh.cells.len(), 10);
    assert_eq!(mesh.cells[1].property_id, None);
    assert_eq!(mesh.cells[7].kind, CellKind::Hex8);
    let mut output = Vec::new();
    bdf::mesh::write(&mesh, &mut output).unwrap();
    let rewritten = bdf::mesh::read(&output).unwrap().mesh;
    assert_eq!(rewritten.cells.len(), 10);
    assert_eq!(rewritten.cells[7].kind, CellKind::Hex8);
}

#[test]
fn arbitrary_short_inputs_do_not_panic() {
    for byte in 0..=255_u8 {
        let _ = bdf::mesh::read([byte]);
    }
}
