//! Adapter tests for VTK.

use super::{read, read_projection, write_data};
use crate::core::{CellKind, Dataset, Field, FieldLocation};
use crate::formats::bdf;

/// Round-trip legacy VTK topology, original IDs, and numeric arrays.
#[test]
fn all_linear_cells_ids_and_fields_roundtrip() {
    let mesh = bdf::mesh::read(include_bytes!(
        "../../../../tests/fixtures/mixed-linear.bdf"
    ))
    .unwrap()
    .mesh;
    let dataset = Dataset {
        fields: vec![
            Field {
                name: "velocity".into(),
                location: FieldLocation::Point,
                components: vec!["C1".into(), "C2".into(), "C3".into()],
                values: vec![0.5; mesh.points.len() * 3],
                step: None,
                time: None,
            },
            Field {
                name: "energy".into(),
                location: FieldLocation::Cell,
                components: vec!["C1".into()],
                values: (0..mesh.cells.len())
                    .map(|index| f64::from(u32::try_from(index).unwrap()))
                    .collect(),
                step: None,
                time: None,
            },
        ],
        mesh,
    };
    let mut bytes = Vec::new();
    write_data(&dataset, &mut bytes).unwrap();
    let read = read(std::str::from_utf8(&bytes).unwrap()).unwrap();
    assert_eq!(read, dataset);
}

/// Assign deterministic IDs and report the loss when legacy VTK omits them.
#[test]
fn missing_ids_are_reported_and_assigned() {
    let source = "# vtk DataFile Version 2.0\nexample\nASCII\nDATASET UNSTRUCTURED_GRID\nPOINTS 2 float\n0 0 0 1 0 0\nCELLS 1 3\n2 0 1\nCELL_TYPES 1\n3\nPOINT_DATA 2\nSCALARS temperature double 1\nLOOKUP_TABLE default\n1.5 2.5\n";
    let projection = read_projection(source).unwrap();
    assert!(projection.generated_point_ids);
    assert!(projection.generated_cell_ids);
    assert_eq!(projection.dataset.mesh.points[0].id, 1);
    assert_eq!(projection.dataset.fields[0].values, [1.5, 2.5]);
}

/// Read an independently written VTK offsets/connectivity fixture.
#[test]
fn vtk_written_offsets_fixture_preserves_ids_and_topology() {
    let dataset = read(include_str!("../../../../tests/fixtures/vtk-5.1-mixed.vtk")).unwrap();
    assert_eq!(dataset.mesh.points.len(), 9);
    assert_eq!(dataset.mesh.cells.len(), 7);
    assert_eq!(dataset.mesh.cells[0].id, 101);
    assert_eq!(dataset.mesh.cells[6].kind, CellKind::Pyramid5);
    assert_eq!(dataset.mesh.cells[6].connectivity, [0, 1, 2, 3, 8]);
}

/// Reject malformed legacy VTK sections and unsupported datasets.
#[test]
fn malformed_and_unsupported_inputs_fail() {
    let source = "# vtk DataFile Version 2.0\nexample\nASCII\nDATASET UNSTRUCTURED_GRID\nPOINTS 2 float\n0 0 0 1 0 0\nCELLS 1 3\n2 0 2\nCELL_TYPES 1\n3\n";
    assert!(read(source).is_err());
    assert!(read(&source.replace("ASCII", "BINARY")).is_err());
    assert!(read(&source.replace("CELL_TYPES 1\n3", "CELL_TYPES 1\n22")).is_err());
    assert!(read(&source.replace("POINTS 2", "POINTS 999999999")).is_err());
}

/// Preserve supported legacy VTK scalar, vector, and integer arrays.
#[test]
fn scalar_vector_and_integer_attributes_are_preserved() {
    let source = "# vtk DataFile Version 2.0\nattributes\nASCII\nDATASET UNSTRUCTURED_GRID\nPOINTS 2 double\n0 0 0 1 0 0\nCELLS 1 3\n2 0 1\nCELL_TYPES 1\n3\nPOINT_DATA 2\nSCALARS temperature int\nLOOKUP_TABLE default\n2 3\nVECTORS velocity float\n1 2 3 4 5 6\nCELL_DATA 1\nSCALARS pressure unsigned_short 1\nLOOKUP_TABLE default\n7\n";
    let dataset = read(source).unwrap();
    assert_eq!(dataset.fields.len(), 3);
    assert_eq!(dataset.fields[0].values, [2.0, 3.0]);
    assert_eq!(dataset.fields[1].values, [1., 2., 3., 4., 5., 6.]);
    assert_eq!(dataset.fields[2].location, FieldLocation::Cell);
    assert_eq!(dataset.fields[2].values, [7.]);
}

/// Reject incomplete attribute records and invalid cell offsets.
#[test]
fn malformed_legacy_attributes_and_offsets_are_rejected() {
    let base = "# vtk DataFile Version 2.0\nmalformed\nASCII\nDATASET UNSTRUCTURED_GRID\nPOINTS 2 float\n0 0 0 1 0 0\nCELLS 1 3\n2 0 1\nCELL_TYPES 1\n3\n";
    for suffix in [
        "POINT_DATA 2\nSCALARS bad int 1\nLOOKUP_TABLE default\n9007199254740993 1\n",
        "POINT_DATA 2\nSCALARS bad bit 1\nLOOKUP_TABLE default\n0 1\n",
        "POINT_DATA 2\nSCALARS bad double 0\nLOOKUP_TABLE default\n0 1\n",
        "POINT_DATA 2\nSCALARS bad double 1\nLOOKUP_TABLE rainbow\n0 1\n",
        "POINT_DATA 2\nFIELD FieldData 1\nnastran_node_id 1 2 double\n1 2\n",
        "POINT_DATA 2\nSCALARS same double 1\nLOOKUP_TABLE default\n0 1\nSCALARS same double 1\nLOOKUP_TABLE default\n0 1\n",
        "POINT_DATA 2\nFIELD FieldData 1\nnastran_node_id 1 2 int\n0 2\n",
        "CELL_DATA 2\nSCALARS bad double 1\nLOOKUP_TABLE default\n0 1\n",
        "POINT_DATA 2\nFIELD FieldData 1\nshort 2 3 float\n0 1 2 3 4 5\n",
        "POINT_DATA 2\nTEXTURE_COORDINATES uv 2 float\n0 0 1 0\n",
    ] {
        let error = read(&format!("{base}{suffix}")).unwrap_err();
        assert_eq!(error.code, "E_VTK", "{suffix}");
    }
    let offset_base = base.replace(
        "CELLS 1 3\n2 0 1",
        "CELLS 2 2\nOFFSETS vtktypeint64\n0 1\nCONNECTIVITY vtktypeint64\n0 1",
    );
    assert_eq!(read(&offset_base).unwrap_err().code, "E_VTK");
    let unsupported_type = base.replace(
        "CELLS 1 3\n2 0 1",
        "CELLS 2 2\nOFFSETS float\n0 2\nCONNECTIVITY vtktypeint64\n0 1",
    );
    assert_eq!(read(&unsupported_type).unwrap_err().code, "E_VTK");
}
