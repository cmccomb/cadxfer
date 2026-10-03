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
    assert!(
        report
            .omissions
            .iter()
            .any(|item| item.stage == Stage::Source)
    );
    assert!(
        report.omissions.iter().any(|item| {
            item.stage == Stage::Destination && item.detail.contains("property IDs")
        })
    );
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

#[test]
fn pch_read_requires_matching_mesh_and_explicit_selection() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = root.join("tests/fixtures/pch-multiple.pch");
    assert_eq!(Format::from_input_path(&source).unwrap(), Format::Pch);
    assert_eq!(
        conversion::read_path(&source, &Options::default())
            .unwrap_err()
            .code,
        "E_USAGE"
    );
    let mut options = Options {
        mesh: Some(root.join("tests/fixtures/pch-companion.bdf")),
        ..Options::default()
    };
    assert_eq!(
        conversion::read_path(&source, &options).unwrap_err().code,
        "E_PCH"
    );
    options.subcase = Some(2);
    assert_eq!(
        conversion::read_path(&source, &options).unwrap_err().code,
        "E_PCH"
    );
    options.step = Some(1);
    let read = conversion::read_path(&source, &options).unwrap();
    assert_eq!(
        (read.dataset.mesh.points.len(), read.dataset.fields.len()),
        (2, 1)
    );
    assert_eq!(read.dataset.fields[0].time, Some(0.5));
    assert!((read.dataset.fields[0].values[0] - 3.0).abs() < f64::EPSILON);
    assert_eq!(
        conversion::convert(read, Format::Pch, &Options::default(), Vec::new())
            .unwrap_err()
            .code,
        "E_FORMAT"
    );
}

#[test]
fn pch_non_bdf_companion_requires_basic_frame_assertion() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = root.join("tests/fixtures/pch-multiple.pch");
    let companion = root.join("tests/fixtures/pch-companion.bdf");
    let mesh = conversion::read_path(&companion, &Options::default())
        .unwrap()
        .dataset
        .mesh;
    let folder = std::env::temp_dir().join(format!("caexfer-pch-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("mesh.vtu");
    let file = std::fs::File::create(&path).unwrap();
    caexfer::vtu::write(&mesh, file).unwrap();
    let mut options = Options {
        mesh: Some(path.clone()),
        subcase: Some(1),
        ..Options::default()
    };
    let error = conversion::read_path(&source, &options).unwrap_err();
    assert_eq!(error.code, "E_USAGE");
    assert!(error.message.contains("--assume-basic-frame"));
    options.assume_basic_frame = true;
    let read = conversion::read_path(&source, &options).unwrap();
    assert_eq!(read.dataset.fields.len(), 1);
    assert!(
        read.omissions
            .iter()
            .any(|item| item.stage == Stage::Assumption)
    );
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(folder).unwrap();
}
