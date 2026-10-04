//! Adapter tests for FRD.

use super::write::type_code;
use super::{read, write};
use crate::core::{CellKind, Dataset, Field, FieldLocation};
use std::fmt::Write as _;

/// Parse long FRD IDs separately from adjacent fixed-width record markers.
#[test]
fn long_fixed_width_ids_do_not_merge_with_record_keys() {
    let nodes = [1_234_567_890, 1_234_567_891, 1_234_567_892];
    let mut source = format!(" 2C{:68}1\n", "");
    for (i, id) in nodes.iter().enumerate() {
        writeln!(
            source,
            " -1{id:>10}{:>12.5E}{:>12.5E}{:>12.5E}",
            [0.0, 1.0, 2.0][i],
            0.,
            0.
        )
        .unwrap();
    }
    write!(source, " -3\n 3C{:68}1\n", "").unwrap();
    writeln!(
        source,
        " -1{:>10}{:>5}{:>5}{:>5}",
        1_234_567_899u64, 7, 0, 0
    )
    .unwrap();
    write!(
        source,
        " -2{:>10}{:>10}{:>10}\n -3\n 9999\n",
        nodes[0], nodes[1], nodes[2]
    )
    .unwrap();
    let dataset = read(source.as_bytes()).unwrap();
    assert_eq!(
        dataset.mesh.points.iter().map(|p| p.id).collect::<Vec<_>>(),
        nodes
    );
    assert_eq!(dataset.mesh.cells[0].connectivity, vec![0, 1, 2]);
}

/// Round-trip FRD geometry and continued nodal result records.
#[test]
fn ascii_writer_preserves_mesh_and_nodal_values_with_continuation() {
    let mut dataset = read(include_bytes!(
        "../../../../tests/fixtures/linear-results.frd"
    ))
    .unwrap();
    dataset.fields.push(Field {
        name: "EXTRA".into(),
        location: FieldLocation::Point,
        components: (1..=7).map(|index| format!("C{index}")).collect(),
        values: (0..21).map(f64::from).collect(),
        step: Some(2),
        time: Some(0.25),
    });
    let mut bytes = Vec::new();
    write(&dataset, &mut bytes).unwrap();
    let decoded = read(&bytes).unwrap();
    assert_eq!(decoded.mesh.points, dataset.mesh.points);
    assert_eq!(decoded.mesh.cells, dataset.mesh.cells);
    assert_eq!(decoded.fields[0].values, dataset.fields[0].values);
    assert_eq!(decoded.fields[2].values, dataset.fields[2].values);
    assert_eq!(decoded.fields[2].step, Some(2));
    assert_eq!(decoded.fields[2].time, Some(0.25));
}

/// Refuse FRD output for a topology the writer cannot encode.
#[test]
fn ascii_writer_rejects_unsupported_pyramid() {
    assert_eq!(type_code(CellKind::Pyramid5).unwrap_err().code, "E_FRD");
}

/// Round-trip every supported FRD linear cell family in one dataset.
#[test]
fn all_supported_linear_topologies_roundtrip_in_one_file() {
    let mut mesh = crate::formats::bdf::mesh::read(include_bytes!(
        "../../../../tests/fixtures/mixed-linear.bdf"
    ))
    .unwrap()
    .mesh;
    mesh.cells.retain(|cell| cell.kind != CellKind::Pyramid5);
    for cell in &mut mesh.cells {
        cell.property_id = None;
    }
    let expected = mesh.cells.iter().map(|cell| cell.kind).collect::<Vec<_>>();
    let mut encoded = Vec::new();
    write(
        &Dataset {
            mesh,
            fields: vec![],
        },
        &mut encoded,
    )
    .unwrap();
    let decoded = read(&encoded).unwrap();
    assert_eq!(
        decoded
            .mesh
            .cells
            .iter()
            .map(|cell| cell.kind)
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(decoded.mesh.cells.len(), 6);
}

/// Reject broken FRD result and element records with format errors.
#[test]
fn malformed_result_and_connectivity_records_fail_explicitly() {
    let source = include_str!("../../../../tests/fixtures/linear-results.frd");
    for (old, new) in [
        ("-2 1 2 3", "-2 1 2 99"),
        ("-2 1 2 3", "-2 1 2"),
        ("-1 1 1\n -2 1 1.0", "-1 1 2\n -2 1 1.0"),
        ("-1 3 0.0 0.2 0.0", "-1 3 0.0 0.2"),
        ("-4 DISP 3 1", "-4 DISP 4 1"),
        ("-1 3 0.0 0.2 0.0", "-1 2 0.0 0.2 0.0"),
        ("-1 1 0.0 0.0 0.0", "-1 1 not-a-number 0.0 0.0"),
    ] {
        assert!(source.contains(old));
        let changed = source.replacen(old, new, 1);
        assert_eq!(read(changed.as_bytes()).unwrap_err().code, "E_FRD", "{new}");
    }
}

/// Reject malformed FRD result headers, duplicate rows, and continuations.
#[test]
fn malformed_result_state_never_yields_a_partial_field() {
    let source = include_str!("../../../../tests/fixtures/linear-results.frd");
    for (old, new) in [
        ("-1 10 7 0 0", "-1 10 99 0 0"),
        ("-4 DISP 3 1", "-4 DISP 3"),
        ("-4 DISP 3 1", "-4 DISP 3 9"),
        ("-4 DISP 3 1", "-4 DISP 0 1"),
        ("-5 D1 1 2 1 0", "-5"),
        ("-5 D3 1 2 3 0", ""),
        ("-1 2 0.1 0.0 0.0", "-1 2 0.1 0.0 0.0\n -1 2 0.1 0.0 0.0"),
        ("-1 2 1\n -2 1 2.0", "-1 2 1\n -1 2 1\n -2 1 2.0"),
    ] {
        assert!(source.contains(old), "missing fixture marker {old}");
        let changed = source.replacen(old, new, 1);
        assert_eq!(read(changed.as_bytes()).unwrap_err().code, "E_FRD", "{new}");
    }
}

/// Decode a seven-component result split across a node row and continuation.
#[test]
fn short_result_continuations_complete_each_node_tuple() {
    let mut source = String::from(
        " 2C\n -1 1 0 0 0\n -1 2 1 0 0\n -1 3 0 1 0\n -3\n 3C\n -1 10 7 0 0\n -2 1 2 3\n -3\n 100CL101\n -4 EXTRA 7 1\n",
    );
    for component in 1..=7 {
        writeln!(source, " -5 C{component} 1 2 {component} 0").unwrap();
    }
    for node in 1..=3 {
        writeln!(source, " -1 {node} 1 2 3 4 5 6\n -2 7").unwrap();
    }
    source.push_str(" -3\n 9999\n");
    let dataset = read(source.as_bytes()).unwrap();
    assert_eq!(
        dataset.fields[0].values,
        [1., 2., 3., 4., 5., 6., 7.].repeat(3)
    );
}

/// Do not return incomplete geometry or results when required records are absent.
#[test]
fn missing_geometry_or_result_rows_are_errors() {
    assert_eq!(read(b" 9999\n").unwrap_err().code, "E_FRD");
    let source = include_str!("../../../../tests/fixtures/linear-results.frd");
    let marker = " -1 3 0.0 0.2 0.0\n";
    assert!(source.contains(marker));
    let incomplete = source.replacen(marker, "", 1);
    assert_eq!(read(incomplete.as_bytes()).unwrap_err().code, "E_FRD");
    let unexpected = source.replacen(" -1 1 0.0 0.0 0.0", " -9 1 0.0 0.0 0.0", 1);
    assert_eq!(read(unexpected.as_bytes()).unwrap_err().code, "E_FRD");
}

/// Reject FRD values that exceed fixed-width columns before output begins.
#[test]
fn writer_preflights_fixed_width_limits_before_writing() {
    let baseline = read(include_bytes!(
        "../../../../tests/fixtures/linear-results.frd"
    ))
    .unwrap();
    let mut cases = Vec::new();
    let mut oversized_id = baseline.clone();
    oversized_id.mesh.cells[0].id = 10_000_000_000;
    cases.push(oversized_id);
    let mut oversized_value = baseline.clone();
    oversized_value.fields[0].values[0] = 1e100;
    cases.push(oversized_value);
    let mut invalid_label = baseline.clone();
    invalid_label.fields[0].name = "TOO LONG FIELD".into();
    cases.push(invalid_label);
    let mut invalid_step = baseline;
    invalid_step.fields[0].step = Some(-1);
    cases.push(invalid_step);
    for dataset in cases {
        let mut encoded = Vec::new();
        assert_eq!(write(&dataset, &mut encoded).unwrap_err().code, "E_FRD");
        assert_eq!(encoded, []);
    }
}
