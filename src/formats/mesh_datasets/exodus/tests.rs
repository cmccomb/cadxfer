//! Adapter tests for exodus.

use super::{read_projection, write_data};
use crate::core::{Cell, CellKind, Dataset, FieldLocation, Mesh, Point};

const SOURCE: &[u8] = include_bytes!("../../../../tests/fixtures/exodus-two-blocks.exo");

/// Read a classic Exodus fixture with its original maps and scalar values.
#[test]
fn independent_classic_fixture_preserves_ids_and_values() {
    let projection = read_projection(SOURCE, None).unwrap();
    let dataset = &projection.dataset;
    assert_eq!(
        dataset
            .mesh
            .points
            .iter()
            .map(|point| point.id)
            .collect::<Vec<_>>(),
        [10, 20, 30, 40]
    );
    assert_eq!(
        dataset
            .mesh
            .cells
            .iter()
            .map(|cell| cell.id)
            .collect::<Vec<_>>(),
        [100, 200]
    );
    assert_eq!(dataset.mesh.cells[0].kind, CellKind::Quad4);
    assert_eq!(dataset.mesh.cells[1].kind, CellKind::Triangle3);
    assert_eq!(dataset.fields.len(), 4);
    assert_eq!(dataset.fields[0].name, "TEMP");
    assert_eq!(dataset.fields[0].location, FieldLocation::Point);
    assert_eq!(dataset.fields[1].values, [5.0, 6.0, 7.0, 8.0]);
    assert_eq!(dataset.fields[2].name, "ENERGY");
    assert_eq!(dataset.fields[2].values, [10.0, 11.0]);
    assert_eq!(dataset.fields[3].values, [20.0, 21.0]);
    assert_eq!(dataset.fields[3].time, Some(0.5));
    assert!(
        projection
            .omissions
            .iter()
            .any(|item| item.contains("block ID"))
    );
}

/// Preserve chosen Exodus time-step fields through write and read.
#[test]
fn selected_step_and_writer_round_trip() {
    let selected = read_projection(SOURCE, Some(1)).unwrap();
    assert_eq!(selected.dataset.fields.len(), 2);
    assert_eq!(selected.dataset.fields[0].step, Some(2));
    let full = read_projection(SOURCE, None).unwrap().dataset;
    let mut output = Vec::new();
    write_data(&full, &mut output).unwrap();
    let reread = read_projection(&output, None).unwrap().dataset;
    assert_eq!(reread.mesh, full.mesh);
    assert_eq!(reread.fields, full.fields);
}

/// Refuse incomplete Exodus fields and unsupported container layouts.
#[test]
fn partial_and_unsupported_inputs_fail() {
    let partial = include_bytes!("../../../../tests/fixtures/exodus-partial.exo");
    assert!(
        read_projection(partial, None)
            .unwrap_err()
            .message
            .contains("partial")
    );
    assert_eq!(
        read_projection(SOURCE, Some(2)).unwrap_err().code,
        "E_EXODUS"
    );
    assert_eq!(
        read_projection(b"not NetCDF", None).unwrap_err().code,
        "E_EXODUS"
    );
    let mut incomplete = read_projection(SOURCE, None).unwrap().dataset;
    incomplete.fields.pop();
    assert!(
        write_data(&incomplete, Vec::new())
            .unwrap_err()
            .message
            .contains("missing a time step")
    );
    let mut vector = read_projection(SOURCE, None).unwrap().dataset;
    vector.fields[0].components.push("C2".to_owned());
    vector.fields[0].values.extend([1.0, 2.0, 3.0, 4.0]);
    assert_eq!(
        write_data(&vector, Vec::new()).unwrap_err().code,
        "E_EXODUS"
    );
}

/// Handle legacy coordinate arrays and report assigned IDs when maps are absent.
#[test]
fn legacy_coordinates_and_missing_maps_are_explicit() {
    let source = include_bytes!("../../../../tests/fixtures/exodus-legacy.exo");
    let projection = read_projection(source, None).unwrap();
    for (point, expected) in projection.dataset.mesh.points[1..].iter().zip([
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
    ]) {
        assert!(
            point
                .position
                .iter()
                .zip(expected)
                .all(|(actual, wanted)| (actual - wanted).abs() < f64::EPSILON)
        );
    }
    assert_eq!(projection.dataset.mesh.cells[0].kind, CellKind::Tet4);
    assert!(
        projection
            .omissions
            .iter()
            .any(|item| item.contains("node_num_map absent"))
    );
    assert!(
        projection
            .omissions
            .iter()
            .any(|item| item.contains("node sets"))
    );
    assert!(
        projection
            .omissions
            .iter()
            .any(|item| item.contains("node_ns1"))
    );
    assert_eq!(projection.dataset.fields, Vec::new());
    assert!(read_projection(source, Some(0)).is_err());
}

/// Encode and decode all supported Exodus linear element families.
#[test]
fn all_linear_kinds_write_and_read() {
    for kind in [
        CellKind::Line2,
        CellKind::Triangle3,
        CellKind::Quad4,
        CellKind::Tet4,
        CellKind::Pyramid5,
        CellKind::Wedge6,
        CellKind::Hex8,
    ] {
        let points = (0..kind.node_count())
            .map(|index| Point {
                id: u64::try_from(index + 10).unwrap(),
                position: [
                    f64::from(u32::try_from(index).unwrap()),
                    if kind.dimension() >= 2 {
                        f64::from(u32::try_from(index % 2).unwrap())
                    } else {
                        0.0
                    },
                    if kind.dimension() >= 3 {
                        f64::from(u32::try_from(index % 3).unwrap())
                    } else {
                        0.0
                    },
                ],
            })
            .collect();
        let dataset = Dataset {
            mesh: Mesh {
                points,
                cells: vec![Cell {
                    id: 42,
                    kind,
                    connectivity: (0..kind.node_count()).collect(),
                    property_id: None,
                }],
                ..Mesh::default()
            },
            fields: Vec::new(),
        };
        let mut output = Vec::new();
        write_data(&dataset, &mut output).unwrap();
        let reread = read_projection(&output, None).unwrap().dataset;
        assert_eq!(reread.mesh, dataset.mesh);
    }
}

/// Reject mixed dimensions and ID maps outside classic Exodus limits.
#[test]
fn writer_refuses_unrepresentable_dimensions_and_id_maps() {
    let mut dataset = read_projection(SOURCE, None).unwrap().dataset;
    dataset.fields.clear();
    dataset.mesh.cells.push(Cell {
        id: 300,
        kind: CellKind::Line2,
        connectivity: vec![0, 1],
        property_id: None,
    });
    assert!(
        write_data(&dataset, Vec::new())
            .unwrap_err()
            .message
            .contains("one cell dimension")
    );
    dataset.mesh.cells.pop();
    dataset.mesh.points[0].position[2] = 1.0;
    assert!(
        write_data(&dataset, Vec::new())
            .unwrap_err()
            .message
            .contains("omitted coordinates")
    );
    dataset.mesh.points[0].position[2] = 0.0;
    dataset.mesh.points[0].id = u64::try_from(i32::MAX).unwrap() + 1;
    assert!(
        write_data(&dataset, Vec::new())
            .unwrap_err()
            .message
            .contains("node ID exceeds Int32")
    );
    dataset.mesh.points[0].id = 10;
    dataset.mesh.cells[0].property_id = Some(7);
    assert!(
        write_data(&dataset, Vec::new())
            .unwrap_err()
            .message
            .contains("not solver property IDs")
    );
}

/// Require valid scalar and time metadata before writing Exodus results.
#[test]
fn writer_requires_explicit_scalar_time_metadata() {
    let original = read_projection(SOURCE, None).unwrap().dataset;
    let mut invalid = original.clone();
    invalid.fields[0].name = "bad\nname".into();
    assert!(
        write_data(&invalid, Vec::new())
            .unwrap_err()
            .message
            .contains("field name")
    );
    invalid = original.clone();
    invalid.fields[0].step = None;
    assert!(
        write_data(&invalid, Vec::new())
            .unwrap_err()
            .message
            .contains("one-based step")
    );
    invalid = original.clone();
    invalid.fields[0].time = None;
    assert!(
        write_data(&invalid, Vec::new())
            .unwrap_err()
            .message
            .contains("explicit time")
    );
    invalid = original;
    invalid.fields[0].time = Some(9.0);
    assert!(
        write_data(&invalid, Vec::new())
            .unwrap_err()
            .message
            .contains("times disagree")
    );
}
