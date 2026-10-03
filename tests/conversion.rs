use std::path::Path;

use caexfer::conversion::{self, Format, Options, Stage};

#[test]
fn inp_generators_fail_instead_of_projecting_incomplete_geometry() {
    for generator in ["*NGEN\n1,3,1\n", "*ELGEN\n10,3,1\n"] {
        let input = format!("*NODE\n1,0,0,0\n2,1,0,0\n*ELEMENT, TYPE=T3D2\n10,1,2\n{generator}");
        let error = caexfer::inp::read(&input).unwrap_err();
        assert_eq!(error.code, "E_INP");
        assert!(error.message.contains("expanded geometry"));
    }
}

#[test]
fn msh_declared_counts_cannot_allocate_beyond_input() {
    let huge_nodes = "$MeshFormat\n4.1 0 8\n$EndMeshFormat\n$Nodes\n1 1 1 1\n3 1 0 18446744073709551615\n$EndNodes\n$Elements\n0 0 0 0\n$EndElements\n";
    assert_eq!(caexfer::msh::read(huge_nodes).unwrap_err().code, "E_MSH");

    let huge_elements = "$MeshFormat\n4.1 0 8\n$EndMeshFormat\n$Nodes\n0 0 0 0\n$EndNodes\n$Elements\n1 1 1 1\n1 1 1 18446744073709551615\n$EndElements\n";
    assert_eq!(caexfer::msh::read(huge_elements).unwrap_err().code, "E_MSH");
}

#[test]
fn public_conversion_reports_source_and_destination_losses() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/plate.bdf");
    let mut output = Vec::new();
    let report = conversion::convert_path(&source, Format::Msh, &Options::default(), &mut output)
        .expect("BDF mesh projects to MSH");
    assert_eq!((report.points, report.cells, report.fields), (4, 1, 0));
    assert!(report
        .omissions
        .iter()
        .any(|item| item.stage == Stage::Source));
    assert!(report
        .omissions
        .iter()
        .any(|item| { item.stage == Stage::Destination && item.detail.contains("property IDs") }));
    let dataset = caexfer::msh::read(std::str::from_utf8(&output).unwrap()).unwrap();
    assert_eq!(
        (dataset.mesh.points.len(), dataset.mesh.cells.len()),
        (4, 1)
    );
}

#[test]
fn op2_non_bdf_companion_requires_explicit_frame_assertion() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let options = Options {
        mesh: Some(root.join("tests/fixtures/linear-results.frd")),
        ..Options::default()
    };
    let error = conversion::read_path(&root.join("tests/fixtures/solid_bending.op2"), &options)
        .unwrap_err();
    assert_eq!(error.code, "E_USAGE");
    assert!(error.message.contains("--assume-basic-frame"));
}
