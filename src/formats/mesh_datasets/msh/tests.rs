//! Adapter tests for msh.

use super::*;
use crate::core::{Cell, CellKind, Dataset, Field, FieldLocation, Mesh, Point};

/// Keep numeric node and element fields through MSH serialization.
#[test]
fn numeric_point_and_cell_fields_survive_msh_roundtrip() {
    let dataset = Dataset {
        mesh: Mesh {
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
                property_id: None,
            }],
            ..Mesh::default()
        },
        fields: vec![
            Field {
                name: "Displacement".into(),
                location: FieldLocation::Point,
                components: vec!["C1".into(), "C2".into()],
                values: vec![0., 1., 2., 3., 4., 5.],
                step: Some(1),
                time: Some(0.25),
            },
            Field {
                name: "Energy".into(),
                location: FieldLocation::Cell,
                components: vec!["C1".into()],
                values: vec![9.],
                step: Some(1),
                time: Some(0.25),
            },
        ],
    };
    let mut bytes = Vec::new();
    write(&dataset, &mut bytes).unwrap();
    assert_eq!(read(std::str::from_utf8(&bytes).unwrap()).unwrap(), dataset);
    let mut bytes_22 = Vec::new();
    write_22(&dataset, &mut bytes_22).unwrap();
    assert_eq!(
        read(std::str::from_utf8(&bytes_22).unwrap()).unwrap(),
        dataset
    );
}

/// Report MSH 2.2 tags and reject declared counts beyond input bounds.
#[test]
fn msh22_tags_are_reported_and_counts_are_bounded() {
    let source = "$MeshFormat\n2.2 0 8\n$EndMeshFormat\n$Nodes\n2\n10 0 0 0\n20 1 0 0\n$EndNodes\n$Elements\n1\n30 1 2 7 8 10 20\n$EndElements\n";
    let projection = read_projection(source).unwrap();
    assert_eq!(projection.tagged_elements, 1);
    assert_eq!(projection.generated_group_names, 1);
    assert_eq!(projection.dataset.mesh.cells[0].id, 30);
    assert_eq!(projection.dataset.mesh.cells[0].connectivity, [0, 1]);
    assert_eq!(projection.dataset.mesh.cell_sets[0].name, "physical_1_7");
    assert!(read(&source.replace("$Nodes\n2", "$Nodes\n999999999")).is_err());
    assert!(read(&source.replace("30 1 2", "30 1 999999999")).is_err());
}

/// Preserve overlapping named physical selections on boundary cells.
#[test]
fn msh41_physical_names_select_overlapping_boundary_cells() {
    let source = include_str!("../../../../tests/fixtures/named-boundary.msh");
    let dataset = read(source).unwrap();
    assert_eq!(dataset.mesh.cell_sets.len(), 3);
    assert_eq!(dataset.mesh.cell_sets[0].name, "inlet");
    assert_eq!(dataset.mesh.cell_sets[0].cell_ids, [1]);
    assert_eq!(dataset.mesh.cell_sets[1].name, "outer wall");
    assert_eq!(dataset.mesh.cell_sets[1].cell_ids, [1]);
    assert_eq!(write(&dataset, Vec::new()).unwrap_err().code, "E_MSH");
    let mut boundary_only = dataset.clone();
    boundary_only
        .mesh
        .cell_sets
        .retain(|set| set.dimension == 1);
    let mut output = Vec::new();
    crate::formats::su2::write_data(&boundary_only, &mut output).unwrap();
    assert!(
        std::str::from_utf8(&output)
            .unwrap()
            .contains("MARKER_TAG= outer wall")
    );
}

/// Map a named physical point group to a node set.
#[test]
fn msh41_physical_point_name_becomes_node_set() {
    let source = "$MeshFormat\n4.1 0 8\n$EndMeshFormat\n$PhysicalNames\n1\n0 42 \"fixed\"\n$EndPhysicalNames\n$Entities\n1 0 0 0\n1 0 0 0 1 42\n$EndEntities\n$Nodes\n1 1 10 10\n0 1 0 1\n10\n0 0 0\n$EndNodes\n$Elements\n0 0 0 0\n$EndElements\n";
    let projection = read_projection(source).unwrap();
    assert_eq!(projection.generated_group_names, 0);
    assert_eq!(projection.dataset.mesh.node_sets[0].name, "fixed");
    assert_eq!(projection.dataset.mesh.node_sets[0].point_ids, [10]);
}

/// Read all supported topologies from an external Gmsh 2.2 fixture.
#[test]
fn gmsh_written_msh22_fixture_preserves_all_topologies() {
    let projection = read_projection(include_str!(
        "../../../../tests/fixtures/gmsh-2.2-mixed.msh"
    ))
    .unwrap();
    assert_eq!(projection.tagged_elements, 0);
    let dataset = projection.dataset;
    assert_eq!(dataset.mesh.points.len(), 9);
    assert_eq!(dataset.mesh.cells.len(), 7);
    assert_eq!(dataset.mesh.cells[0].id, 1);
    assert_eq!(dataset.mesh.cells[6].kind, CellKind::Pyramid5);
}

/// Retain original node IDs in MSH 2.2 and 4.1 without elements.
#[test]
fn nodes_only_meshes_preserve_ids_in_both_dialects() {
    let dataset = Dataset {
        mesh: Mesh {
            points: vec![
                Point {
                    id: 10,
                    position: [0., 0., 0.],
                },
                Point {
                    id: 20,
                    position: [1., 2., 3.],
                },
            ],
            cells: vec![],
            ..Mesh::default()
        },
        fields: vec![],
    };
    for version in [Version::V4_1, Version::V2_2] {
        let mut output = Vec::new();
        write_version(&dataset, version, &mut output).unwrap();
        let decoded = read(std::str::from_utf8(&output).unwrap()).unwrap();
        assert_eq!(decoded.mesh.points, dataset.mesh.points);
        assert_eq!(decoded.mesh.cells, []);
    }
}

/// Reject broken MSH field blocks before exposing partial results.
#[test]
fn malformed_field_blocks_cannot_create_partial_results() {
    let base = "$MeshFormat\n2.2 0 8\n$EndMeshFormat\n$Nodes\n2\n10 0 0 0\n20 1 0 0\n$EndNodes\n$Elements\n1\n30 1 0 10 20\n$EndElements\n$NodeData\n1\n\"temperature\"\n1\n0.5\n3\n2\n1\n2\n10 1\n20 2\n$EndNodeData\n";
    let valid = read(base).unwrap();
    assert_eq!(valid.fields[0].values, [1., 2.]);
    for (old, new) in [
        ("$NodeData\n1", "$NodeData\n2"),
        ("\"temperature\"", "temperature"),
        ("\"temperature\"\n1\n0.5", "\"temperature\"\n2\n0.5"),
        ("0.5\n3\n2\n1\n2", "0.5\n2\n2\n1\n2"),
        ("0.5\n3\n2\n1\n2", "0.5\n3\n2\n0\n2"),
        ("0.5\n3\n2\n1\n2", "0.5\n3\n2\n1\n1"),
        ("20 2\n$EndNodeData", "10 2\n$EndNodeData"),
        ("20 2\n$EndNodeData", "99 2\n$EndNodeData"),
        ("$EndNodeData", ""),
    ] {
        assert!(base.contains(old));
        assert!(read(&base.replacen(old, new, 1)).is_err(), "{new}");
    }
}

/// Refuse malformed MSH 2.2 records and IDs outside writer limits.
#[test]
fn msh22_rejects_extra_tokens_and_unrepresentable_ids() {
    let base = "$MeshFormat\n2.2 0 8\n$EndMeshFormat\n$Nodes\n2\n10 0 0 0\n20 1 0 0\n$EndNodes\n$Elements\n1\n30 1 0 10 20\n$EndElements\n";
    for (old, new) in [
        ("20 1 0 0\n$EndNodes", "20 1 0 0 99\n$EndNodes"),
        (
            "30 1 0 10 20\n$EndElements",
            "30 1 0 10 20 99\n$EndElements",
        ),
        ("30 1 0 10 20", "30 1 0 10 99"),
        ("2.2 0 8", "2.2 1 8"),
    ] {
        assert!(read(&base.replacen(old, new, 1)).is_err(), "{new}");
    }
    let mut dataset = read(base).unwrap();
    dataset.mesh.points[0].id = i32::MAX as u64 + 1;
    let mut output = Vec::new();
    assert_eq!(write_22(&dataset, &mut output).unwrap_err().code, "E_MSH");
    assert_eq!(output, []);
}
