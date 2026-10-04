use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use caexfer::conversion::{self, Format, Options, Stage};

static NEXT_SCRATCH: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "caexfer-conversion-{}-{}",
            std::process::id(),
            NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn write(&self, name: &str, bytes: impl AsRef<[u8]>) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

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

#[test]
fn format_names_and_output_capabilities_are_explicit() {
    assert_eq!(Format::parse("PCH").unwrap(), Format::Pch);
    assert_eq!(Format::parse("not-a-format").unwrap_err().code, "E_FORMAT");
    assert_eq!(
        Format::from_input_path(Path::new("unknown.mesh"))
            .unwrap_err()
            .code,
        "E_FORMAT"
    );
    for extension in ["pch", "dat", "unknown"] {
        assert_eq!(
            Format::from_output_path(Path::new(&format!("result.{extension}")))
                .unwrap_err()
                .code,
            "E_FORMAT"
        );
    }
}

#[test]
fn stl_route_reports_missing_identity_and_refuses_volume_export() {
    let scratch = Scratch::new();
    let source = scratch.write(
        "surface.stl",
        b"solid sample\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid sample\n",
    );
    let read = conversion::read_path(&source, &Options::default()).unwrap();
    assert_eq!(read.format, Format::Stl);
    assert_eq!(read.dataset.mesh.cells.len(), 1);
    assert!(
        read.omissions
            .iter()
            .any(|item| item.detail.contains("facet-local IDs"))
    );
    let mut output = Vec::new();
    let report = conversion::convert(read, Format::Stl, &Options::default(), &mut output).unwrap();
    assert_eq!(output.len(), 134);
    assert!(
        report
            .omissions
            .iter()
            .any(|item| item.stage == Stage::Destination)
    );

    let volume = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mixed-linear.bdf");
    assert_eq!(
        conversion::convert_path(&volume, Format::Stl, &Options::default(), Vec::new())
            .unwrap_err()
            .code,
        "E_STL"
    );
}

#[test]
fn named_msh_boundary_survives_su2_conversion() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/named-boundary.msh");
    let mut output = Vec::new();
    let report =
        conversion::convert_path(&source, Format::Su2, &Options::default(), &mut output).unwrap();
    assert_eq!((report.points, report.cells), (3, 2));
    let decoded = caexfer::su2::read_projection(std::str::from_utf8(&output).unwrap()).unwrap();
    assert_eq!(decoded.dataset.mesh.cell_sets.len(), 2);
    assert_eq!(decoded.dataset.mesh.cell_sets[0].name, "inlet");
    assert_eq!(decoded.dataset.mesh.cell_sets[1].name, "outer wall");

    let scratch = Scratch::new();
    let path = scratch.write("converted.su2", &output);
    let read = conversion::read_path(&path, &Options::default()).unwrap();
    let mut vtu = Vec::new();
    let report = conversion::convert(read, Format::Vtu, &Options::default(), &mut vtu).unwrap();
    assert!(
        report
            .omissions
            .iter()
            .any(|item| item.detail.contains("named cell set"))
    );
}

#[test]
fn unv_geometry_route_reports_native_omissions() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gmsh-six-kind.unv");
    let read = conversion::read_path(&source, &Options::default()).unwrap();
    assert_eq!(read.format, Format::Unv);
    assert_eq!(
        (
            read.dataset.mesh.points.len(),
            read.dataset.mesh.cells.len()
        ),
        (9, 6)
    );
    assert!(
        read.omissions
            .iter()
            .any(|item| item.detail.contains("2477"))
    );
    let mut output = Vec::new();
    let report = conversion::convert(read, Format::Unv, &Options::default(), &mut output).unwrap();
    assert_eq!((report.points, report.cells), (9, 6));
    let decoded = caexfer::unv::read_projection(std::str::from_utf8(&output).unwrap()).unwrap();
    assert_eq!(decoded.dataset.mesh.cells[5].id, 6);

    let pyramid = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gmsh-2.2-mixed.msh");
    assert_eq!(
        conversion::convert_path(&pyramid, Format::Unv, &Options::default(), Vec::new())
            .unwrap_err()
            .code,
        "E_UNV"
    );
}

#[test]
fn result_options_cannot_change_unrelated_mesh_readers() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = root.join("examples/plate.bdf");
    for options in [
        Options {
            subcase: Some(1),
            ..Options::default()
        },
        Options {
            assume_basic_frame: true,
            ..Options::default()
        },
        Options {
            step: Some(1),
            ..Options::default()
        },
    ] {
        assert_eq!(
            conversion::read_path(&source, &options).unwrap_err().code,
            "E_USAGE"
        );
    }
}

#[test]
fn bounded_reads_and_bad_encodings_fail_before_projection() {
    let s = Scratch::new();
    let source = s.write("large.vtu", vec![b'x'; 80]);
    let options = Options {
        max_bytes: 10,
        ..Options::default()
    };
    assert_eq!(
        conversion::read_path(&source, &options).unwrap_err().code,
        "E_LIMIT"
    );
    for (extension, code) in [
        ("vtu", "E_VTU"),
        ("vtk", "E_VTK"),
        ("msh", "E_MSH"),
        ("inp", "E_INP"),
    ] {
        let path = s.write(&format!("invalid.{extension}"), [0xff, 0xfe]);
        assert_eq!(
            conversion::read_path(&path, &Options::default())
                .unwrap_err()
                .code,
            code
        );
    }
}

#[test]
fn external_identity_and_ignored_sections_are_visible_in_receipts() {
    let s = Scratch::new();
    let vtk = s.write(
        "external.vtk",
        b"# vtk DataFile Version 2.0\nexternal\nASCII\nDATASET UNSTRUCTURED_GRID\nPOINTS 2 float\n0 0 0 1 0 0\nCELLS 1 3\n2 0 1\nCELL_TYPES 1\n3\n",
    );
    let read = conversion::read_path(&vtk, &Options::default()).unwrap();
    assert_eq!(read.omissions.len(), 2);
    assert!(
        read.omissions
            .iter()
            .all(|item| item.stage == Stage::Source)
    );
    let msh = s.write(
        "tagged.msh",
        b"$MeshFormat\n2.2 0 8\n$EndMeshFormat\n$PhysicalNames\n0\n$EndPhysicalNames\n$Nodes\n2\n10 0 0 0\n20 1 0 0\n$EndNodes\n$Elements\n1\n30 1 2 7 8 10 20\n$EndElements\n",
    );
    let read = conversion::read_path(&msh, &Options::default()).unwrap();
    assert_eq!(read.dataset.mesh.cell_sets[0].name, "physical_1_7");
    assert!(
        read.omissions
            .iter()
            .any(|item| item.detail.contains("tag list"))
    );

    let pch = s.write(
        "with-other-result.pch",
        format!(
            "$STRESSES\n10 G 1 2 3\n{}",
            include_str!("fixtures/pch-multiple.pch")
        ),
    );
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let options = Options {
        mesh: Some(root.join("tests/fixtures/pch-companion.bdf")),
        subcase: Some(1),
        ..Options::default()
    };
    let read = conversion::read_path(&pch, &options).unwrap();
    assert!(
        read.omissions
            .iter()
            .any(|item| item.detail.contains("other PCH"))
    );
}

#[test]
fn destination_reports_ambiguous_or_dropped_fields() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = conversion::read_path(
        &root.join("tests/fixtures/linear-results.frd"),
        &Options::default(),
    )
    .unwrap();
    let mut duplicated = source.clone();
    duplicated
        .dataset
        .fields
        .push(duplicated.dataset.fields[0].clone());
    assert_eq!(
        conversion::convert(duplicated, Format::Vtu, &Options::default(), Vec::new())
            .unwrap_err()
            .code,
        "E_VTU"
    );
    let mut cell_field = source.clone();
    cell_field.dataset.fields[0].location = caexfer::core::FieldLocation::Cell;
    cell_field.dataset.fields[0].values.truncate(3);
    let mut encoded = Vec::new();
    let report =
        conversion::convert(cell_field, Format::Frd, &Options::default(), &mut encoded).unwrap();
    assert!(
        report
            .omissions
            .iter()
            .any(|item| item.detail.contains("cell field"))
    );
    assert_ne!(encoded, []);
}

#[test]
fn external_vtu_without_identity_arrays_gets_explicit_stable_ids() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let read =
        conversion::read_path(&root.join("examples/plate.bdf"), &Options::default()).unwrap();
    let mut encoded = Vec::new();
    caexfer::vtu::write(&read.dataset.mesh, &mut encoded).unwrap();
    let mut source = String::from_utf8(encoded).unwrap();
    for (start, end) in [
        ("<PointData>", "</PointData>"),
        ("<CellData>", "</CellData>"),
    ] {
        let first = source.find(start).unwrap();
        let last = source.find(end).unwrap() + end.len();
        source.replace_range(first..last, "");
    }
    let dataset = caexfer::vtu::read(&source).unwrap();
    assert_eq!(
        dataset
            .mesh
            .points
            .iter()
            .map(|point| point.id)
            .collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
    assert_eq!(dataset.mesh.cells[0].id, 1);
    assert_eq!(dataset.mesh.cells[0].property_id, None);
}
