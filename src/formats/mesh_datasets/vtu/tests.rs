//! Adapter tests for vtu.

use super::*;
use crate::core::{Cell, CellKind, Dataset, Field, FieldLocation, Mesh, Point};
use std::io::Write;

/// Build a small mesh with stable original IDs for VTU assertions.
fn triangle() -> Mesh {
    Mesh {
        points: vec![
            Point {
                id: 10,
                position: [0., 0., 0.],
            },
            Point {
                id: 20,
                position: [1., 0., 0.],
            },
            Point {
                id: 30,
                position: [0., 1., 0.],
            },
        ],
        cells: vec![Cell {
            id: 50,
            kind: CellKind::Triangle3,
            connectivity: vec![0, 1, 2],
            property_id: Some(7),
        }],
        ..Mesh::default()
    }
}

/// Write VTU arrays that retain mesh identity, connectivity, and cell kind.
#[test]
fn writes_ids_connectivity_and_type() {
    let mut output = Vec::new();
    write(&triangle(), &mut output).unwrap();
    let text = String::from_utf8(output).unwrap();
    assert!(text.contains("NumberOfPoints=\"3\" NumberOfCells=\"1\""));
    assert!(text.contains("nastran_node_id"));
    assert!(text.contains("0 1 2 "));
    assert!(text.contains("format=\"ascii\">\n5\n"));
}

/// Validate the mesh before emitting partial VTU output.
#[test]
fn invalid_mesh_writes_nothing() {
    let mut mesh = triangle();
    mesh.cells[0].connectivity[0] = 100;
    let mut output = Vec::new();
    assert!(write(&mesh, &mut output).is_err());
    assert_eq!(output, Vec::<u8>::new());
}

/// Map every supported linear cell to its VTK type code.
#[test]
fn all_linear_cell_numbers() {
    assert_eq!(
        [
            CellKind::Line2,
            CellKind::Triangle3,
            CellKind::Quad4,
            CellKind::Tet4,
            CellKind::Hex8,
            CellKind::Wedge6,
            CellKind::Pyramid5
        ]
        .map(write::vtk_type),
        [3, 5, 9, 10, 12, 13, 14]
    );
}

/// Preserve all seven linear topology families through VTU data arrays.
#[test]
fn all_linear_topologies_roundtrip() {
    let mesh = crate::formats::bdf::mesh::read(include_bytes!(
        "../../../../tests/fixtures/mixed-linear.bdf"
    ))
    .unwrap()
    .mesh;
    let mut bytes = Vec::new();
    write(&mesh, &mut bytes).unwrap();
    let decoded = read(std::str::from_utf8(&bytes).unwrap()).unwrap();
    assert_eq!(decoded.mesh.points, mesh.points);
    assert_eq!(decoded.mesh.cells, mesh.cells);
}

/// Keep large original IDs in integer arrays without floating-point rounding.
#[test]
fn preserves_large_ids_as_integers() {
    let mut mesh = triangle();
    mesh.points[0].id = 9_007_199_254_740_993;
    let mut output = Vec::new();
    write(&mesh, &mut output).unwrap();
    assert!(
        String::from_utf8(output)
            .unwrap()
            .contains("9007199254740993")
    );
}

/// Surface caller-owned stream failures from the VTU writer.
#[test]
fn propagates_writer_failure() {
    /// Output stream that always fails for writer error propagation tests.
    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("test failure"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    assert_eq!(write(&triangle(), Broken).unwrap_err().code, "E_IO");
}

/// Separate numeric fields from identity arrays at both entity locations.
#[test]
fn reads_numeric_point_and_cell_fields_without_mixing_ids() {
    let dataset = Dataset {
        mesh: triangle(),
        fields: vec![
            Field {
                name: "Temperature".into(),
                location: FieldLocation::Point,
                components: vec!["T".into()],
                values: vec![10., 20., 30.],
                step: Some(2),
                time: Some(0.5),
            },
            Field {
                name: "Energy".into(),
                location: FieldLocation::Cell,
                components: vec!["E".into()],
                values: vec![7.],
                step: Some(2),
                time: Some(0.5),
            },
        ],
    };
    let mut bytes = Vec::new();
    write_data(&dataset, &mut bytes).unwrap();
    let parsed = read(std::str::from_utf8(&bytes).unwrap()).unwrap();
    assert_eq!(parsed, dataset);
}

/// Decode escaped field labels with the current XML reader API.
#[test]
fn escaped_field_attributes_roundtrip() {
    let dataset = Dataset {
        mesh: triangle(),
        fields: vec![Field {
            name: "A&B<\"C\"".into(),
            location: FieldLocation::Point,
            components: vec!["T&1".into()],
            values: vec![1., 2., 3.],
            step: None,
            time: None,
        }],
    };
    let mut bytes = Vec::new();
    write_data(&dataset, &mut bytes).unwrap();
    assert_eq!(read(std::str::from_utf8(&bytes).unwrap()).unwrap(), dataset);
}

/// Ignore commented XML tags when selecting mesh coordinates.
#[test]
fn xml_comments_cannot_spoof_geometry() {
    let mut bytes = Vec::new();
    write(&triangle(), &mut bytes).unwrap();
    let source = String::from_utf8(bytes).unwrap();
    let spoof = "<!-- <Points><DataArray format=\"ascii\" NumberOfComponents=\"3\">100 0 0 200 0 0 300 0 0</DataArray></Points> -->\n";
    let source = source.replace("<Points>", &format!("{spoof}<Points>"));
    let dataset = read(&source).unwrap();
    assert_eq!(dataset.mesh.points, triangle().points);
}

/// Require exact XML attribute names rather than suffix matches.
#[test]
fn xml_attribute_names_match_exactly() {
    let mut bytes = Vec::new();
    write(&triangle(), &mut bytes).unwrap();
    let source = String::from_utf8(bytes)
        .unwrap()
        .replace("Name=\"connectivity\"", "XName=\"connectivity\"");
    assert_eq!(read(&source).unwrap_err().code, "E_VTU");
}

/// Bound declared VTU array sizes before allocation or multiplication.
#[test]
fn declared_counts_and_components_cannot_overflow() {
    let mut bytes = Vec::new();
    write(&triangle(), &mut bytes).unwrap();
    let source = String::from_utf8(bytes).unwrap();
    for malformed in [
        source.replace(
            "NumberOfPoints=\"3\"",
            "NumberOfPoints=\"18446744073709551615\"",
        ),
        source.replace(
            "NumberOfCells=\"1\"",
            "NumberOfCells=\"18446744073709551615\"",
        ),
    ] {
        assert_eq!(read(&malformed).unwrap_err().code, "E_VTU");
    }
    let source = source.replace(
        "</PointData>",
        "<DataArray type=\"Float64\" Name=\"x\" NumberOfComponents=\"18446744073709551615\" format=\"ascii\">1</DataArray></PointData>",
    );
    assert_eq!(read(&source).unwrap_err().code, "E_VTU");
}

/// Reject field names that cannot be represented in XML.
#[test]
fn invalid_xml_names_fail_before_writing() {
    for invalid_name in ["bad\0name", "bad\u{1}name", "bad\nname"] {
        let mut dataset = Dataset {
            mesh: triangle(),
            fields: vec![Field {
                name: invalid_name.into(),
                location: FieldLocation::Point,
                components: vec!["C1".into()],
                values: vec![1., 2., 3.],
                step: None,
                time: None,
            }],
        };
        let mut output = Vec::new();
        assert_eq!(write_data(&dataset, &mut output).unwrap_err().code, "E_VTU");
        assert_eq!(output, Vec::<u8>::new());
        dataset.fields[0].name = "valid".into();
        dataset.fields[0].components[0] = invalid_name.into();
        assert_eq!(write_data(&dataset, &mut output).unwrap_err().code, "E_VTU");
        assert_eq!(output, Vec::<u8>::new());
    }
}

/// Reject malformed XML structure before accepting changed geometry.
#[test]
fn structural_xml_mutations_fail_before_geometry_can_change() {
    let mut bytes = Vec::new();
    write(&triangle(), &mut bytes).unwrap();
    let source = String::from_utf8(bytes).unwrap();
    for (old, new) in [
        ("type=\"UnstructuredGrid\"", "type=\"PolyData\""),
        ("version=\"0.1\"", "compressor=\"vtkZLibDataCompressor\""),
        ("NumberOfPoints=\"3\"", "NumberOfPoints=\"4\""),
        ("NumberOfCells=\"1\"", "NumberOfCells=\"2\""),
        (
            "type=\"UInt64\" Name=\"nastran_node_id\"",
            "type=\"Int64\" Name=\"nastran_node_id\"",
        ),
        ("Name=\"connectivity\"", "Name=\"wrong_connectivity\""),
        ("Name=\"offsets\"", "Name=\"wrong_offsets\""),
        ("format=\"ascii\">\n0 0 0", "format=\"binary\">\n0 0 0"),
        ("NumberOfComponents=\"3\"", "NumberOfComponents=\"2\""),
        ("<Points>", "<Points><Unsupported/>"),
        ("</Piece>", "<Unsupported/></Piece>"),
        ("</UnstructuredGrid>", "<Piece/></UnstructuredGrid>"),
        ("</VTKFile>", "<![CDATA[untrusted]]></VTKFile>"),
    ] {
        assert!(source.contains(old), "missing fixture marker {old}");
        let changed = source.replacen(old, new, 1);
        assert_eq!(read(&changed).unwrap_err().code, "E_VTU", "{new}");
    }
}

/// Reject ambiguous XML trees and arrays even when the remaining mesh is valid.
#[test]
fn duplicate_and_nested_xml_records_cannot_shadow_mesh_data() {
    let mut bytes = Vec::new();
    write(&triangle(), &mut bytes).unwrap();
    let source = String::from_utf8(bytes).unwrap();
    for malformed in [
        source.replace("<Points>", "<Points></Points><Points>"),
        source.replace("<PointData>", "<PointData></PointData><PointData>"),
        source.replace("0 0 0", "<Nested>0 0 0</Nested>"),
        source.replace("0 0 0", "<Nested/>0 0 0"),
        source.replace("<Points>", "<Points>unexpected text"),
        source.replace("</VTKFile>", ""),
        format!("{source}<Extra/>"),
        format!("{source}<Extra></Extra>"),
        source.replace(
            "</Cells>",
            "<DataArray type=\"Int64\" Name=\"offsets\" format=\"ascii\">3</DataArray></Cells>",
        ),
        source.replace("<Points>", "<Points>\0"),
    ] {
        assert_eq!(read(&malformed).unwrap_err().code, "E_VTU");
    }
}

/// Detect truncated identity arrays, unused cells, and duplicate numeric fields.
#[test]
fn malformed_identity_and_field_arrays_are_not_projected() {
    let dataset = Dataset {
        mesh: triangle(),
        fields: vec![Field {
            name: "TEMP".into(),
            location: FieldLocation::Point,
            components: vec!["T".into()],
            values: vec![1.0, 2.0, 3.0],
            step: None,
            time: None,
        }],
    };
    let mut bytes = Vec::new();
    write_data(&dataset, &mut bytes).unwrap();
    let source = String::from_utf8(bytes).unwrap();
    for (old, new) in [
        (
            "Name=\"nastran_node_id\" format=\"ascii\">\n10\n20\n30\n",
            "Name=\"nastran_node_id\" format=\"ascii\">\n10\n20\n",
        ),
        (
            "type=\"UInt64\" Name=\"nastran_element_id\"",
            "type=\"Int64\" Name=\"nastran_element_id\"",
        ),
        (
            "type=\"UInt64\" Name=\"nastran_property_id\"",
            "type=\"Int64\" Name=\"nastran_property_id\"",
        ),
        ("0 1 2 \n</DataArray>", "0 1 2 0 \n</DataArray>"),
        (
            "Name=\"TEMP\" NumberOfComponents=\"1\" format=\"ascii\" ComponentName0=\"T\">\n1 \n2 \n3 \n",
            "Name=\"TEMP\" NumberOfComponents=\"1\" format=\"ascii\" ComponentName0=\"T\">\n1 \n2 \n",
        ),
        (
            "</PointData>",
            "<DataArray type=\"Float64\" Name=\"TEMP\" format=\"ascii\">1 2 3</DataArray></PointData>",
        ),
    ] {
        assert!(source.contains(old), "missing fixture marker {old}");
        let malformed = source.replacen(old, new, 1);
        assert_eq!(read(&malformed).unwrap_err().code, "E_VTU", "{new}");
    }
}

/// Reject truncated identity and field arrays instead of accepting partial data.
#[test]
fn numeric_vtu_mutations_reject_incomplete_identity_and_fields() {
    let dataset = Dataset {
        mesh: triangle(),
        fields: vec![Field {
            name: "TEMP".into(),
            location: FieldLocation::Point,
            components: vec!["T".into()],
            values: vec![1., 2., 3.],
            step: Some(2),
            time: Some(0.5),
        }],
    };
    let mut bytes = Vec::new();
    write_data(&dataset, &mut bytes).unwrap();
    let source = String::from_utf8(bytes).unwrap();
    for (old, new) in [
        (
            "Name=\"offsets\" format=\"ascii\">\n3",
            "Name=\"offsets\" format=\"ascii\">\n4",
        ),
        (
            "Name=\"types\" format=\"ascii\">\n5",
            "Name=\"types\" format=\"ascii\">\n22",
        ),
        (
            "Name=\"nastran_element_id\" format=\"ascii\">\n50",
            "Name=\"nastran_element_id\" format=\"ascii\">\n50 51",
        ),
        (
            "Name=\"nastran_property_id\" format=\"ascii\">\n7",
            "Name=\"nastran_property_id\" format=\"ascii\">\n7 8",
        ),
        (
            "Name=\"TEMP\" NumberOfComponents=\"1\" format=\"ascii\"",
            "Name=\"TEMP\" NumberOfComponents=\"1\" format=\"binary\"",
        ),
        (
            "Name=\"TEMP\" NumberOfComponents=\"1\"",
            "Name=\"TEMP\" NumberOfComponents=\"0\"",
        ),
        (
            "Name=\"TEMP\" NumberOfComponents=\"1\"",
            "Name=\"TEMP\" NumberOfComponents=\"x\"",
        ),
        (
            "type=\"Float64\" Name=\"TEMP\"",
            "type=\"Int64\" Name=\"TEMP\"",
        ),
    ] {
        assert!(source.contains(old), "missing fixture marker {old}");
        let changed = source.replacen(old, new, 1);
        assert_eq!(read(&changed).unwrap_err().code, "E_VTU", "{new}");
    }
}
