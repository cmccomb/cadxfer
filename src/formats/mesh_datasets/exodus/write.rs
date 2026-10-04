//! Classic Exodus II writer for geometry and complete scalar series.

use super::err;
use crate::core::{CellKind, Dataset, Field, FieldLocation, Result};
use netcdf3::{DataSet, FileWriter, Version};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::{Cursor, Seek, SeekFrom, Write};
use std::rc::Rc;

/// A seekable shared buffer for `NetCDF`'s writer and our caller-owned stream.
#[derive(Clone)]
struct SharedBuffer(Rc<RefCell<Cursor<Vec<u8>>>>);

impl Write for SharedBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.borrow_mut().write(bytes)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.borrow_mut().flush()
    }
}

impl Seek for SharedBuffer {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        self.0.borrow_mut().seek(position)
    }
}

/// Native Exodus type name for one supported linear cell.
fn type_name(kind: CellKind) -> &'static str {
    match kind {
        CellKind::Line2 => "BAR",
        CellKind::Triangle3 => "TRI",
        CellKind::Quad4 => "QUAD",
        CellKind::Tet4 => "TETRA",
        CellKind::Pyramid5 => "PYRAMID",
        CellKind::Wedge6 => "WEDGE",
        CellKind::Hex8 => "HEX",
    }
}

/// Fields for each native scalar variable, ordered by time step.
type FieldSeries<'a> = Vec<Vec<&'a Field>>;

/// Require every scalar variable to have one value array at each time step.
fn collect_series<'a>(
    map: BTreeMap<&str, BTreeMap<i64, &'a Field>>,
    steps: usize,
) -> Result<FieldSeries<'a>> {
    map.into_values()
        .map(|by_step| {
            if by_step.len() != steps {
                return Err(err("Exodus variable is missing a time step"));
            }
            Ok(by_step.into_values().collect())
        })
        .collect()
}

/// Validate field steps and build one scalar variable per name and location.
fn output_fields(dataset: &Dataset) -> Result<(Vec<f64>, FieldSeries<'_>, FieldSeries<'_>)> {
    if dataset.fields.is_empty() {
        return Ok((Vec::new(), Vec::new(), Vec::new()));
    }
    let mut times = BTreeMap::new();
    let mut nodal: BTreeMap<&str, BTreeMap<i64, &Field>> = BTreeMap::new();
    let mut element: BTreeMap<&str, BTreeMap<i64, &Field>> = BTreeMap::new();
    for field in &dataset.fields {
        if field.components.len() != 1 {
            return Err(err("classic Exodus writer requires scalar fields"));
        }
        if field.name.len() > 32 || field.name.chars().any(char::is_control) {
            return Err(err(
                "Exodus field name must be at most 32 non-control bytes",
            ));
        }
        let step = field
            .step
            .ok_or_else(|| err("Exodus fields require a one-based step"))?;
        let time = field
            .time
            .ok_or_else(|| err("Exodus fields require an explicit time"))?;
        if step < 1
            || times
                .insert(step, time)
                .is_some_and(|old| old.to_bits() != time.to_bits())
        {
            return Err(err("Exodus field steps and times disagree"));
        }
        let map = match field.location {
            FieldLocation::Point => &mut nodal,
            FieldLocation::Cell => &mut element,
        };
        if map
            .entry(&field.name)
            .or_default()
            .insert(step, field)
            .is_some()
        {
            return Err(err("duplicate Exodus variable at one step"));
        }
    }
    let steps = times.len();
    for (index, step) in times.keys().enumerate() {
        if *step != i64::try_from(index + 1).map_err(|_| err("too many Exodus steps"))? {
            return Err(err("Exodus steps must start at 1 and be contiguous"));
        }
    }
    Ok((
        times.into_values().collect(),
        collect_series(nodal, steps)?,
        collect_series(element, steps)?,
    ))
}

/// Encode one fixed-width name row per scalar variable.
fn name_rows(variables: &[Vec<&Field>]) -> Vec<u8> {
    let mut rows = vec![0; variables.len() * 33];
    for (index, fields) in variables.iter().enumerate() {
        let name = fields[0].name.as_bytes();
        rows[index * 33..index * 33 + name.len()].copy_from_slice(name);
    }
    rows
}

/// Add a `NetCDF` dimension and map errors to the Exodus diagnostic code.
fn add_dim(data: &mut DataSet, name: &str, count: usize) -> Result<()> {
    data.add_fixed_dim(name, count)
        .map_err(|error| err(format!("cannot define {name}: {error:?}")))
}

/// Add a typed variable and map `NetCDF` schema errors.
fn add_i32(data: &mut DataSet, name: &str, dims: &[&str]) -> Result<()> {
    data.add_var_i32(name, dims)
        .map_err(|error| err(format!("cannot define {name}: {error:?}")))
}

/// Add a floating-point variable and map `NetCDF` schema errors.
fn add_f64(data: &mut DataSet, name: &str, dims: &[&str]) -> Result<()> {
    data.add_var_f64(name, dims)
        .map_err(|error| err(format!("cannot define {name}: {error:?}")))
}

/// Write a classic NetCDF-3 Exodus mesh and complete scalar time series.
///
/// One native element block is generated per cell kind. Original positive IDs
/// are stored in `node_num_map` and `elem_num_map`. Named selections and
/// properties have no mapping in this writer. Mixed cell dimensions and
/// partial/multicomponent result fields fail explicitly.
///
/// # Errors
///
/// Returns `E_EXODUS` for unsupported data or `NetCDF` failures, or `E_IO` for
/// the caller-owned output stream. A stream error can leave partial output.
#[allow(clippy::too_many_lines)] // Exodus schema definitions and corresponding writes stay together.
pub fn write_data(dataset: &Dataset, mut output: impl Write) -> Result<()> {
    dataset.validate()?;
    dataset.mesh.require_no_sets("E_EXODUS")?;
    if dataset.mesh.points.is_empty() || dataset.mesh.cells.is_empty() {
        return Err(err("classic Exodus writer requires nodes and elements"));
    }
    let dimension = dataset.mesh.cells[0].kind.dimension();
    if dataset
        .mesh
        .cells
        .iter()
        .any(|cell| cell.kind.dimension() != dimension)
    {
        return Err(err("classic Exodus writer requires one cell dimension"));
    }
    if dataset.mesh.points.iter().any(|point| {
        point.position[usize::from(dimension)..]
            .iter()
            .any(|&value| value != 0.0)
    }) {
        return Err(err(
            "Exodus dimension cannot encode nonzero omitted coordinates",
        ));
    }
    if dataset
        .mesh
        .cells
        .iter()
        .any(|cell| cell.property_id.is_some())
    {
        return Err(err("Exodus blocks are not solver property IDs"));
    }
    let node_ids = dataset
        .mesh
        .points
        .iter()
        .map(|point| i32::try_from(point.id).map_err(|_| err("node ID exceeds Int32")))
        .collect::<Result<Vec<_>>>()?;
    let (times, nodal, element) = output_fields(dataset)?;
    let mut blocks: Vec<(CellKind, Vec<usize>)> = Vec::new();
    for (index, cell) in dataset.mesh.cells.iter().enumerate() {
        if let Some((_, indices)) = blocks.iter_mut().find(|(kind, _)| *kind == cell.kind) {
            indices.push(index);
        } else {
            blocks.push((cell.kind, vec![index]));
        }
    }
    let ordered = blocks
        .iter()
        .flat_map(|(_, indices)| indices.iter().copied())
        .collect::<Vec<_>>();
    let element_ids = ordered
        .iter()
        .map(|&index| {
            i32::try_from(dataset.mesh.cells[index].id).map_err(|_| err("element ID exceeds Int32"))
        })
        .collect::<Result<Vec<_>>>()?;
    let field_values = dataset
        .fields
        .iter()
        .try_fold(0usize, |count, field| count.checked_add(field.values.len()))
        .ok_or_else(|| err("Exodus field count overflows"))?;
    let estimate = dataset
        .mesh
        .points
        .len()
        .checked_mul(32)
        .and_then(|count| count.checked_add(dataset.mesh.cells.len().checked_mul(64)?))
        .and_then(|count| count.checked_add(field_values.checked_mul(8)?))
        .and_then(|count| count.checked_add(1_048_576))
        .ok_or_else(|| err("Exodus output size overflows"))?;
    if estimate > 256 * 1024 * 1024 {
        return Err(err("classic Exodus output exceeds 256 MiB limit"));
    }
    let mut schema = DataSet::new();
    add_dim(&mut schema, "num_dim", usize::from(dimension))?;
    add_dim(&mut schema, "num_nodes", node_ids.len())?;
    add_dim(&mut schema, "num_elem", element_ids.len())?;
    add_dim(&mut schema, "num_el_blk", blocks.len())?;
    add_dim(&mut schema, "len_name", 33)?;
    if !times.is_empty() {
        schema
            .set_unlimited_dim("time_step", times.len())
            .map_err(|error| err(format!("cannot define time_step: {error:?}")))?;
    }
    for (block, (kind, indices)) in blocks.iter().enumerate() {
        let number = block + 1;
        add_dim(
            &mut schema,
            &format!("num_el_in_blk{number}"),
            indices.len(),
        )?;
        add_dim(
            &mut schema,
            &format!("num_nod_per_el{number}"),
            kind.node_count(),
        )?;
        let name = format!("connect{number}");
        add_i32(
            &mut schema,
            &name,
            &[
                &format!("num_el_in_blk{number}"),
                &format!("num_nod_per_el{number}"),
            ],
        )?;
        schema
            .add_var_attr_string(&name, "elem_type", type_name(*kind))
            .map_err(|error| err(format!("cannot name {name}: {error:?}")))?;
    }
    for name in ["coordx", "coordy", "coordz"]
        .into_iter()
        .take(usize::from(dimension))
    {
        add_f64(&mut schema, name, &["num_nodes"])?;
    }
    add_i32(&mut schema, "node_num_map", &["num_nodes"])?;
    add_i32(&mut schema, "elem_num_map", &["num_elem"])?;
    add_i32(&mut schema, "eb_prop1", &["num_el_blk"])?;
    if !times.is_empty() {
        add_f64(&mut schema, "time_whole", &["time_step"])?;
    }
    if !nodal.is_empty() {
        add_dim(&mut schema, "num_nod_var", nodal.len())?;
        schema
            .add_var_u8("name_nod_var", &["num_nod_var", "len_name"])
            .map_err(|error| err(format!("cannot define nodal names: {error:?}")))?;
        for index in 0..nodal.len() {
            add_f64(
                &mut schema,
                &format!("vals_nod_var{}", index + 1),
                &["time_step", "num_nodes"],
            )?;
        }
    }
    if !element.is_empty() {
        add_dim(&mut schema, "num_elem_var", element.len())?;
        schema
            .add_var_u8("name_elem_var", &["num_elem_var", "len_name"])
            .map_err(|error| err(format!("cannot define element names: {error:?}")))?;
        add_i32(&mut schema, "elem_var_tab", &["num_el_blk", "num_elem_var"])?;
        for index in 0..element.len() {
            for block in 0..blocks.len() {
                add_f64(
                    &mut schema,
                    &format!("vals_elem_var{}eb{}", index + 1, block + 1),
                    &["time_step", &format!("num_el_in_blk{}", block + 1)],
                )?;
            }
        }
    }
    schema
        .add_global_attr_string("title", "caexfer classic Exodus geometry and scalar fields")
        .map_err(|error| err(format!("cannot define title: {error:?}")))?;
    schema
        .add_global_attr_f32("api_version", vec![8.24])
        .map_err(|error| err(format!("cannot define API version: {error:?}")))?;
    schema
        .add_global_attr_f32("version", vec![8.24])
        .map_err(|error| err(format!("cannot define Exodus version: {error:?}")))?;
    schema
        .add_global_attr_i32("floating_point_word_size", vec![8])
        .map_err(|error| err(format!("cannot define word size: {error:?}")))?;
    schema
        .add_global_attr_i32("file_size", vec![1])
        .map_err(|error| err(format!("cannot define file size: {error:?}")))?;

    let buffer = SharedBuffer(Rc::new(RefCell::new(Cursor::new(Vec::new()))));
    let mut writer = FileWriter::open_seek_write("exodus", Box::new(buffer.clone()))
        .map_err(|error| err(format!("cannot open NetCDF writer: {error:?}")))?;
    writer
        .set_def(&schema, Version::Classic, 0)
        .map_err(|error| err(format!("cannot define NetCDF schema: {error:?}")))?;
    for (axis, name) in ["coordx", "coordy", "coordz"]
        .into_iter()
        .take(usize::from(dimension))
        .enumerate()
    {
        let values = dataset
            .mesh
            .points
            .iter()
            .map(|point| point.position[axis])
            .collect::<Vec<_>>();
        writer
            .write_var_f64(name, &values)
            .map_err(|error| err(format!("cannot write {name}: {error:?}")))?;
    }
    writer
        .write_var_i32("node_num_map", &node_ids)
        .map_err(|error| err(format!("cannot write node map: {error:?}")))?;
    writer
        .write_var_i32("elem_num_map", &element_ids)
        .map_err(|error| err(format!("cannot write element map: {error:?}")))?;
    let block_ids = (1..=blocks.len())
        .map(|id| i32::try_from(id).map_err(|_| err("too many blocks")))
        .collect::<Result<Vec<_>>>()?;
    writer
        .write_var_i32("eb_prop1", &block_ids)
        .map_err(|error| err(format!("cannot write block IDs: {error:?}")))?;
    for (block, (_, indices)) in blocks.iter().enumerate() {
        let values = indices
            .iter()
            .flat_map(|&index| {
                dataset.mesh.cells[index].connectivity.iter().map(|&node| {
                    i32::try_from(node + 1).map_err(|_| err("node index exceeds Int32"))
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let name = format!("connect{}", block + 1);
        writer
            .write_var_i32(&name, &values)
            .map_err(|error| err(format!("cannot write {name}: {error:?}")))?;
    }
    if !times.is_empty() {
        writer
            .write_var_f64("time_whole", &times)
            .map_err(|error| err(format!("cannot write times: {error:?}")))?;
    }
    if !nodal.is_empty() {
        writer
            .write_var_u8("name_nod_var", &name_rows(&nodal))
            .map_err(|error| err(format!("cannot write nodal names: {error:?}")))?;
        for (index, series) in nodal.iter().enumerate() {
            let values = series
                .iter()
                .flat_map(|field| field.values.iter().copied())
                .collect::<Vec<_>>();
            let name = format!("vals_nod_var{}", index + 1);
            writer
                .write_var_f64(&name, &values)
                .map_err(|error| err(format!("cannot write {name}: {error:?}")))?;
        }
    }
    if !element.is_empty() {
        writer
            .write_var_u8("name_elem_var", &name_rows(&element))
            .map_err(|error| err(format!("cannot write element names: {error:?}")))?;
        writer
            .write_var_i32("elem_var_tab", &vec![1; blocks.len() * element.len()])
            .map_err(|error| err(format!("cannot write element truth table: {error:?}")))?;
        for (index, series) in element.iter().enumerate() {
            for (block, (_, indices)) in blocks.iter().enumerate() {
                let values = series
                    .iter()
                    .flat_map(|field| indices.iter().map(|&cell| field.values[cell]))
                    .collect::<Vec<_>>();
                let name = format!("vals_elem_var{}eb{}", index + 1, block + 1);
                writer
                    .write_var_f64(&name, &values)
                    .map_err(|error| err(format!("cannot write {name}: {error:?}")))?;
            }
        }
    }
    writer
        .close()
        .map_err(|error| err(format!("cannot finish NetCDF file: {error:?}")))?;
    output.write_all(buffer.0.borrow().get_ref())?;
    Ok(())
}
