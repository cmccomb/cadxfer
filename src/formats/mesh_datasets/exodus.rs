//! Pure Rust adapter for the NetCDF-3 classic subset of Exodus II.
//!
//! Exodus blocks partition the native element order. Their identifiers are
//! reported, not reinterpreted as solver property IDs. Node and element maps
//! carry original IDs. Complete scalar nodal and element variables become
//! numeric fields at selected time steps.

use crate::core::{Cell, CellKind, Dataset, Error, Field, FieldLocation, Mesh, Point, Result};
use netcdf3::{DataSet, DataVector, FileReader};
use std::collections::BTreeSet;
use std::io::Cursor;

#[path = "exodus/write.rs"]
mod write;
pub use write::write_data;

/// Geometry, complete scalar results, and information outside this projection.
#[derive(Debug, Clone, PartialEq)]
pub struct Projection {
    /// Validated projected mesh and fields.
    pub dataset: Dataset,

    /// True when no native node number map supplied original IDs.
    pub generated_point_ids: bool,

    /// Source data or identities that cannot be represented here.
    pub omissions: Vec<String>,
}

/// Build a stable format-specific error.
fn err(message: impl Into<String>) -> Error {
    Error::new("E_EXODUS", message)
}

/// Read a variable only when its declared storage fits the bounded source.
fn variable(reader: &mut FileReader, name: &str, bytes: usize) -> Result<DataVector> {
    let var = reader
        .data_set()
        .get_var(name)
        .ok_or_else(|| err(format!("missing Exodus variable {name}")))?;
    let width = match var.data_type() {
        netcdf3::DataType::I8 | netcdf3::DataType::U8 => 1,
        netcdf3::DataType::I16 => 2,
        netcdf3::DataType::I32 | netcdf3::DataType::F32 => 4,
        netcdf3::DataType::F64 => 8,
    };
    if var.len() > bytes / width {
        return Err(err(format!("{name} count exceeds input size")));
    }
    reader
        .read_var(name)
        .map_err(|error| err(format!("cannot read {name}: {error:?}")))
}

/// Read a signed 32-bit Exodus index or identifier variable.
fn integers(reader: &mut FileReader, name: &str, bytes: usize) -> Result<Vec<i32>> {
    match variable(reader, name, bytes)? {
        DataVector::I32(values) => Ok(values),
        _ => Err(err(format!("{name} must use Int32 in classic Exodus"))),
    }
}

/// Read a finite floating-point variable, retaining its numeric precision.
fn numbers(reader: &mut FileReader, name: &str, bytes: usize) -> Result<Vec<f64>> {
    let values = match variable(reader, name, bytes)? {
        DataVector::F32(values) => values.into_iter().map(f64::from).collect(),
        DataVector::F64(values) => values,
        _ => return Err(err(format!("{name} must be Float32 or Float64"))),
    };
    if !values.iter().all(|value: &f64| value.is_finite()) {
        return Err(err(format!("{name} contains nonfinite values")));
    }
    Ok(values)
}

/// Get a required bounded dimension from a NetCDF-3 header.
fn dimension(data: &DataSet, name: &str, bytes: usize) -> Result<usize> {
    let count = data
        .dim_size(name)
        .ok_or_else(|| err(format!("missing Exodus dimension {name}")))?;
    if count == 0 || count > bytes / 4 {
        return Err(err(format!("{name} exceeds input size or is empty")));
    }
    Ok(count)
}

/// Read the original IDs from an optional Exodus number map.
fn ids(
    reader: &mut FileReader,
    name: &str,
    count: usize,
    bytes: usize,
    omissions: &mut Vec<String>,
) -> Result<Vec<u64>> {
    let values = if reader.data_set().has_var(name) {
        integers(reader, name, bytes)?
            .into_iter()
            .map(|value| {
                u64::try_from(value)
                    .ok()
                    .filter(|&value| value > 0)
                    .ok_or_else(|| err(format!("{name} has a nonpositive label")))
            })
            .collect::<Result<Vec<_>>>()?
    } else {
        omissions.push(format!("{name} absent; assigned one-based IDs"));
        (1..=count)
            .map(|value| u64::try_from(value).map_err(|_| err("too many Exodus labels")))
            .collect::<Result<Vec<_>>>()?
    };
    if values.len() != count {
        return Err(err(format!("{name} length does not match mesh size")));
    }
    Ok(values)
}

/// Read separate or legacy interleaved coordinate arrays.
fn points(
    reader: &mut FileReader,
    count: usize,
    dim: usize,
    bytes: usize,
    omissions: &mut Vec<String>,
) -> Result<Vec<Point>> {
    let ids = ids(reader, "node_num_map", count, bytes, omissions)?;
    let coordinates = if reader.data_set().has_var("coordx") {
        let mut axes = Vec::with_capacity(dim);
        for name in ["coordx", "coordy", "coordz"].into_iter().take(dim) {
            let values = numbers(reader, name, bytes)?;
            if values.len() != count {
                return Err(err(format!("{name} length does not match num_nodes")));
            }
            axes.push(values);
        }
        (0..count)
            .map(|index| {
                let mut position = [0.0; 3];
                for (axis, values) in axes.iter().enumerate() {
                    position[axis] = values[index];
                }
                position
            })
            .collect::<Vec<_>>()
    } else {
        let values = numbers(reader, "coord", bytes)?;
        if values.len()
            != count
                .checked_mul(dim)
                .ok_or_else(|| err("coordinate count overflows"))?
        {
            return Err(err("coord length does not match num_dim × num_nodes"));
        }
        (0..count)
            .map(|index| {
                let mut position = [0.0; 3];
                for axis in 0..dim {
                    position[axis] = values[axis * count + index];
                }
                position
            })
            .collect::<Vec<_>>()
    };
    Ok(ids
        .into_iter()
        .zip(coordinates)
        .map(|(id, position)| Point { id, position })
        .collect())
}

/// Decode a known linear Exodus block type and its node count.
fn kind(name: &str, arity: usize) -> Result<CellKind> {
    let name = name.trim_matches('\0').trim().to_ascii_uppercase();
    match (name.as_str(), arity) {
        ("BAR" | "BEAM" | "TRUSS" | "EDGE" | "LINE" | "BAR2", 2) => Ok(CellKind::Line2),
        ("TRI" | "TRIANGLE" | "TRI3" | "TRISHELL", 3) => Ok(CellKind::Triangle3),
        ("QUAD" | "QUAD4" | "SHELL", 4) => Ok(CellKind::Quad4),
        ("TET" | "TETRA" | "TET4", 4) => Ok(CellKind::Tet4),
        ("PYRAMID" | "PYRAMID5", 5) => Ok(CellKind::Pyramid5),
        ("WEDGE" | "WEDGE6", 6) => Ok(CellKind::Wedge6),
        ("HEX" | "HEX8" | "HEXAHEDRON", 8) => Ok(CellKind::Hex8),
        _ => Err(err(format!(
            "unsupported Exodus element type {name}/{arity}"
        ))),
    }
}

/// Read all element blocks in native block order.
fn cells(
    reader: &mut FileReader,
    points: usize,
    expected: usize,
    blocks: usize,
    bytes: usize,
    omissions: &mut Vec<String>,
) -> Result<(Vec<Cell>, Vec<usize>)> {
    let ids = ids(reader, "elem_num_map", expected, bytes, omissions)?;
    if reader.data_set().has_var("eb_prop1") {
        let block_ids = integers(reader, "eb_prop1", bytes)?;
        if block_ids.len() != blocks {
            return Err(err("eb_prop1 length does not match num_el_blk"));
        }
        omissions.push(format!(
            "{blocks} Exodus element block ID(s) are not mesh property IDs"
        ));
    }
    let mut cells = Vec::with_capacity(expected);
    let mut offsets = Vec::with_capacity(blocks + 1);
    for block in 1..=blocks {
        offsets.push(cells.len());
        let count = dimension(reader.data_set(), &format!("num_el_in_blk{block}"), bytes)?;
        let arity = dimension(reader.data_set(), &format!("num_nod_per_el{block}"), bytes)?;
        let name = format!("connect{block}");
        let typ = reader
            .data_set()
            .get_var_attr_as_string(&name, "elem_type")
            .ok_or_else(|| err(format!("{name} is missing elem_type")))?;
        let kind = kind(&typ, arity)?;
        let connectivity = integers(reader, &name, bytes)?;
        if connectivity.len()
            != count
                .checked_mul(arity)
                .ok_or_else(|| err("connectivity count overflows"))?
        {
            return Err(err(format!(
                "{name} length does not match block dimensions"
            )));
        }
        for row in connectivity.chunks_exact(arity) {
            let indices = row
                .iter()
                .map(|&index| {
                    usize::try_from(
                        index
                            .checked_sub(1)
                            .ok_or_else(|| err("invalid Exodus node index"))?,
                    )
                    .ok()
                    .filter(|&index| index < points)
                    .ok_or_else(|| err("Exodus connectivity references missing node"))
                })
                .collect::<Result<Vec<_>>>()?;
            let id = *ids
                .get(cells.len())
                .ok_or_else(|| err("too many Exodus elements"))?;
            cells.push(Cell {
                id,
                kind,
                connectivity: indices,
                property_id: None,
            });
        }
    }
    offsets.push(cells.len());
    if cells.len() != expected {
        return Err(err("element blocks do not cover num_elem"));
    }
    Ok((cells, offsets))
}

/// Decode fixed-width `NetCDF` character rows into nonempty field names.
fn names(reader: &mut FileReader, var: &str, count: usize, bytes: usize) -> Result<Vec<String>> {
    let width = dimension(reader.data_set(), "len_name", bytes)?;
    let DataVector::U8(values) = variable(reader, var, bytes)? else {
        return Err(err(format!("{var} must be a character array")));
    };
    if values.len()
        != count
            .checked_mul(width)
            .ok_or_else(|| err("name array count overflows"))?
    {
        return Err(err(format!("{var} has wrong width")));
    }
    values
        .chunks_exact(width)
        .map(|row| {
            let end = row.iter().position(|&value| value == 0).unwrap_or(width);
            let name = std::str::from_utf8(&row[..end])
                .map_err(|_| err(format!("{var} has non-UTF-8 name")))?
                .trim()
                .to_owned();
            if name.is_empty() {
                return Err(err(format!("{var} has empty name")));
            }
            Ok(name)
        })
        .collect()
}

/// Read complete scalar variables at the requested zero-based time step.
#[allow(clippy::too_many_lines)] // Nodal and blockwise element data share time-step checks.
fn fields(
    reader: &mut FileReader,
    nodes: usize,
    cells: usize,
    offsets: &[usize],
    bytes: usize,
    selected: Option<usize>,
) -> Result<Vec<Field>> {
    let data = reader.data_set();
    let nodal = data.dim_size("num_nod_var").unwrap_or(0);
    let element = data.dim_size("num_elem_var").unwrap_or(0);
    if nodal + element == 0 {
        if selected.is_some() {
            return Err(err("selected step has no Exodus fields"));
        }
        return Ok(Vec::new());
    }
    let steps = dimension(data, "time_step", bytes)?;
    let times = numbers(reader, "time_whole", bytes)?;
    if times.len() != steps {
        return Err(err("time_whole length does not match time_step"));
    }
    let nodal_names = if nodal > 0 {
        names(reader, "name_nod_var", nodal, bytes)?
    } else {
        Vec::new()
    };
    let element_names = if element > 0 {
        names(reader, "name_elem_var", element, bytes)?
    } else {
        Vec::new()
    };
    if reader.data_set().has_var("elem_var_tab") {
        let truth = integers(reader, "elem_var_tab", bytes)?;
        if truth.len()
            != (offsets.len() - 1)
                .checked_mul(element)
                .ok_or_else(|| err("truth table count overflows"))?
            || truth.iter().any(|&value| value != 1)
        {
            return Err(err("partial Exodus element variables are unsupported"));
        }
    }
    let selected = match selected {
        Some(index) if index < steps => vec![index],
        Some(_) => return Err(err("selected Exodus step is out of range")),
        None => (0..steps).collect(),
    };
    let mut fields = Vec::new();
    for (index, name) in nodal_names.iter().enumerate() {
        let var = format!("vals_nod_var{}", index + 1);
        let values = numbers(reader, &var, bytes)?;
        if values.len()
            != steps
                .checked_mul(nodes)
                .ok_or_else(|| err("nodal result count overflows"))?
        {
            return Err(err(format!("{var} has wrong value count")));
        }
        for &step in &selected {
            fields.push(Field {
                name: name.clone(),
                location: FieldLocation::Point,
                components: vec!["C1".to_owned()],
                values: values[step * nodes..(step + 1) * nodes].to_vec(),
                step: Some(i64::try_from(step + 1).map_err(|_| err("step exceeds Int64"))?),
                time: Some(times[step]),
            });
        }
    }
    for (index, name) in element_names.iter().enumerate() {
        let total = steps
            .checked_mul(cells)
            .ok_or_else(|| err("element result count overflows"))?;
        if total > bytes / 8 {
            return Err(err("element result count exceeds input size"));
        }
        let mut values = vec![0.0; total];
        for block in 1..offsets.len() {
            let var = format!("vals_elem_var{}eb{block}", index + 1);
            let block_values = numbers(reader, &var, bytes)?;
            let start = offsets[block - 1];
            let end = offsets[block];
            let block_size = end - start;
            if block_values.len()
                != steps
                    .checked_mul(block_size)
                    .ok_or_else(|| err("element result count overflows"))?
            {
                return Err(err(format!("{var} has wrong value count")));
            }
            for step in 0..steps {
                values[step * cells + start..step * cells + end]
                    .copy_from_slice(&block_values[step * block_size..(step + 1) * block_size]);
            }
        }
        for &step in &selected {
            fields.push(Field {
                name: name.clone(),
                location: FieldLocation::Cell,
                components: vec!["C1".to_owned()],
                values: values[step * cells..(step + 1) * cells].to_vec(),
                step: Some(i64::try_from(step + 1).map_err(|_| err("step exceeds Int64"))?),
                time: Some(times[step]),
            });
        }
    }
    Ok(fields)
}

/// Read classic NetCDF-3 Exodus geometry and complete scalar result variables.
///
/// `selected_step` is zero-based. NetCDF-4/HDF5 files, partial element
/// variables, higher-order cells, and unsupported mesh entities fail. Native
/// node/element sets and side sets are reported but not projected.
///
/// # Errors
///
/// Returns `E_EXODUS` for unsupported or malformed data. The caller should
/// first bound the input bytes, as [`crate::conversion::read_path`] does.
pub fn read_projection(source: &[u8], selected_step: Option<usize>) -> Result<Projection> {
    if source.len() < 4 || &source[..3] != b"CDF" || source[3] != 1 {
        return Err(err("only NetCDF-3 classic Exodus files are supported"));
    }
    let mut reader = FileReader::open_seek_read("exodus", Box::new(Cursor::new(source.to_vec())))
        .map_err(|error| err(format!("invalid NetCDF-3 file: {error:?}")))?;
    let data = reader.data_set();
    let dim = dimension(data, "num_dim", source.len())?;
    if !(1..=3).contains(&dim) {
        return Err(err("Exodus dimension must be 1, 2, or 3"));
    }
    let node_count = dimension(data, "num_nodes", source.len())?;
    let cell_count = dimension(data, "num_elem", source.len())?;
    let block_count = dimension(data, "num_el_blk", source.len())?;
    let generated_point_ids = !data.has_var("node_num_map");
    let mut omissions = Vec::new();
    let points = points(&mut reader, node_count, dim, source.len(), &mut omissions)?;
    let (cells, offsets) = cells(
        &mut reader,
        node_count,
        cell_count,
        block_count,
        source.len(),
        &mut omissions,
    )?;
    for (dimension, label) in [
        ("num_node_sets", "node sets"),
        ("num_side_sets", "side sets"),
        ("num_edge_blk", "edge blocks"),
        ("num_face_blk", "face blocks"),
    ] {
        if reader.data_set().dim_size(dimension).unwrap_or(0) > 0 {
            omissions.push(format!(
                "Exodus {label} are not represented in this projection"
            ));
        }
    }
    let fields = fields(
        &mut reader,
        node_count,
        cell_count,
        &offsets,
        source.len(),
        selected_step,
    )?;
    let mut projected = BTreeSet::from([
        "node_num_map".to_owned(),
        "elem_num_map".to_owned(),
        "eb_prop1".to_owned(),
        "time_whole".to_owned(),
        "name_nod_var".to_owned(),
        "name_elem_var".to_owned(),
        "elem_var_tab".to_owned(),
    ]);
    if reader.data_set().has_var("coordx") {
        for name in ["coordx", "coordy", "coordz"].into_iter().take(dim) {
            projected.insert(name.to_owned());
        }
    } else {
        projected.insert("coord".to_owned());
    }
    for block in 1..=block_count {
        projected.insert(format!("connect{block}"));
    }
    for index in 1..=reader.data_set().dim_size("num_nod_var").unwrap_or(0) {
        projected.insert(format!("vals_nod_var{index}"));
    }
    for index in 1..=reader.data_set().dim_size("num_elem_var").unwrap_or(0) {
        for block in 1..=block_count {
            projected.insert(format!("vals_elem_var{index}eb{block}"));
        }
    }
    for name in reader.data_set().get_var_names() {
        if !projected.contains(&name) {
            omissions.push(format!("Exodus variable {name} is not represented"));
        }
    }
    let attributes = reader.data_set().get_global_attr_names().len();
    if attributes > 0 {
        omissions.push(format!(
            "{attributes} Exodus global attribute(s) are not represented"
        ));
    }
    let dataset = Dataset {
        mesh: Mesh {
            points,
            cells,
            ..Mesh::default()
        },
        fields,
    };
    dataset.validate()?;
    Ok(Projection {
        dataset,
        generated_point_ids,
        omissions,
    })
}

#[cfg(test)]
mod tests {
    use super::{read_projection, write_data};
    use crate::core::{Cell, CellKind, Dataset, FieldLocation, Mesh, Point};

    const SOURCE: &[u8] = include_bytes!("../../../tests/fixtures/exodus-two-blocks.exo");

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
        let partial = include_bytes!("../../../tests/fixtures/exodus-partial.exo");
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
        let source = include_bytes!("../../../tests/fixtures/exodus-legacy.exo");
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
}
