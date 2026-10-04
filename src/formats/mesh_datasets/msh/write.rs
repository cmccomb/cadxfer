//! ASCII writers for Gmsh MSH 4.1 and 2.2.

use super::read::{code, err};
use crate::core::{Cell, Dataset, FieldLocation, Mesh, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

/// Output dialect for the shared MSH format family.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    /// Traditional flat node and element sections.
    V2_2,

    /// Entity-block node and element sections; the default output dialect.
    #[default]
    V4_1,
}

/// Write the discrete entities referenced by node and element blocks.
fn write_entities(mesh: &Mesh, writer: &mut impl Write) -> Result<Option<u32>> {
    let dimensions: BTreeSet<u32> = mesh.cells.iter().map(|cell| code(cell.kind).1).collect();
    let max_dim = dimensions.iter().next_back().copied();
    let mut bounds = [[0.; 3]; 2];
    if let Some(first) = mesh.points.first() {
        bounds = [first.position; 2];
        for point in &mesh.points {
            let [low, high] = &mut bounds;
            for ((min, max), coordinate) in low.iter_mut().zip(high.iter_mut()).zip(point.position)
            {
                *min = min.min(coordinate);
                *max = max.max(coordinate);
            }
        }
    }
    writeln!(writer, "$MeshFormat\n4.1 0 8\n$EndMeshFormat\n$Entities")?;
    let point_entities = if max_dim.is_none() {
        mesh.points.len()
    } else {
        0
    };
    writeln!(
        writer,
        "{point_entities} {} {} {}",
        u8::from(dimensions.contains(&1)),
        u8::from(dimensions.contains(&2)),
        u8::from(dimensions.contains(&3))
    )?;
    if max_dim.is_none() {
        for (index, point) in mesh.points.iter().enumerate() {
            writeln!(
                writer,
                "{} {} {} {} 0",
                index + 1,
                point.position[0],
                point.position[1],
                point.position[2]
            )?;
        }
    }
    for _dim in &dimensions {
        writeln!(
            writer,
            "1 {} {} {} {} {} {} 0 0",
            bounds[0][0], bounds[0][1], bounds[0][2], bounds[1][0], bounds[1][1], bounds[1][2]
        )?;
    }
    writeln!(writer, "$EndEntities")?;
    Ok(max_dim)
}

/// Write nodes classified on a declared entity, retaining original IDs.
fn write_nodes(mesh: &Mesh, max_dim: Option<u32>, writer: &mut impl Write) -> Result<()> {
    writeln!(writer, "$Nodes")?;
    let min = mesh.points.iter().map(|p| p.id).min().unwrap_or(0);
    let max = mesh.points.iter().map(|p| p.id).max().unwrap_or(0);
    if let Some(dim) = max_dim {
        writeln!(writer, "1 {} {min} {max}", mesh.points.len())?;
        writeln!(writer, "{dim} 1 0 {}", mesh.points.len())?;
        for point in &mesh.points {
            writeln!(writer, "{}", point.id)?;
        }
        for point in &mesh.points {
            writeln!(
                writer,
                "{} {} {}",
                point.position[0], point.position[1], point.position[2]
            )?;
        }
    } else {
        writeln!(
            writer,
            "{} {} {min} {max}",
            mesh.points.len(),
            mesh.points.len()
        )?;
        for (index, point) in mesh.points.iter().enumerate() {
            writeln!(writer, "0 {} 0 1\n{}", index + 1, point.id)?;
            writeln!(
                writer,
                "{} {} {}",
                point.position[0], point.position[1], point.position[2]
            )?;
        }
    }
    writeln!(writer, "$EndNodes")?;
    Ok(())
}

/// Write ASCII MSH 4.1 geometry and complete numeric fields.
///
/// Property IDs cannot be represented by this writer and cause an error.
/// Cells are classified on one discrete entity per occupied dimension;
/// physical groups and boundary relationships are outside this model.
///
/// # Errors
///
/// Returns an error for invalid or unrepresentable data, or when the output
/// stream rejects bytes.
pub fn write(dataset: &Dataset, mut writer: impl Write) -> Result<()> {
    // Finish validation and reject unrepresentable properties before writing.
    dataset.validate()?;
    dataset.mesh.require_no_sets("E_MSH")?;
    if dataset.mesh.cells.iter().any(|c| c.property_id.is_some()) {
        return Err(err(
            "property IDs have no lossless MSH mapping in this writer",
        ));
    }
    let mesh = &dataset.mesh;

    // The model has no CAD topology; these entities have no physical tags or
    // declared boundary relationships.
    let max_dim = write_entities(mesh, &mut writer)?;
    write_nodes(mesh, max_dim, &mut writer)?;
    writeln!(writer, "$Elements")?;

    // Group cells by Gmsh type because an element block has one type code.
    let mut groups: BTreeMap<u32, Vec<&Cell>> = BTreeMap::new();
    for cell in &mesh.cells {
        groups.entry(code(cell.kind).0).or_default().push(cell);
    }
    let min = mesh.cells.iter().map(|c| c.id).min().unwrap_or(0);
    let max = mesh.cells.iter().map(|c| c.id).max().unwrap_or(0);
    writeln!(writer, "{} {} {min} {max}", groups.len(), mesh.cells.len())?;
    for (type_code, cells) in groups {
        let dim = code(cells[0].kind).1;
        writeln!(writer, "{dim} 1 {type_code} {}", cells.len())?;
        for cell in cells {
            write!(writer, "{}", cell.id)?;
            for &idx in &cell.connectivity {
                // Convert internal point indices back to original node tags.
                write!(writer, " {}", mesh.points[idx].id)?;
            }
            writeln!(writer)?;
        }
    }
    writeln!(writer, "$EndElements")?;

    write_fields(dataset, &mut writer)
}

/// Emit version-independent complete `NodeData` and `ElementData` blocks.
fn write_fields(dataset: &Dataset, writer: &mut impl Write) -> Result<()> {
    let mesh = &dataset.mesh;
    // Each field becomes a complete NodeData or ElementData block.
    for field in &dataset.fields {
        let name = match field.location {
            FieldLocation::Point => "NodeData",
            FieldLocation::Cell => "ElementData",
        };
        if field.name.contains('"') || field.name.contains('\n') {
            return Err(err("field name cannot be represented in MSH"));
        }
        let ids: Vec<u64> = match field.location {
            FieldLocation::Point => mesh.points.iter().map(|p| p.id).collect(),
            FieldLocation::Cell => mesh.cells.iter().map(|c| c.id).collect(),
        };
        writeln!(writer, "${name}\n1\n\"{}\"", field.name)?;
        if let Some(time) = field.time {
            writeln!(writer, "1\n{time}")?;
        } else {
            writeln!(writer, "0")?;
        }
        writeln!(
            writer,
            "3\n{}\n{}\n{}",
            field.step.unwrap_or(0),
            field.components.len(),
            ids.len()
        )?;

        // The data header counts entity rows, each followed by one tuple.
        for (i, id) in ids.iter().enumerate() {
            write!(writer, "{id}")?;
            for value in &field.values[i * field.components.len()..(i + 1) * field.components.len()]
            {
                write!(writer, " {value}")?;
            }
            writeln!(writer)?;
        }
        writeln!(writer, "$End{name}")?;
    }
    Ok(())
}

/// Write ASCII MSH 2.2 geometry and complete numeric fields.
///
/// Element metadata tags have no neutral source representation, so the writer
/// emits zero tags. Node and element IDs must fit the classic signed-int range.
///
/// # Errors
///
/// Returns an error for invalid data, unrepresentable IDs or properties, or
/// failure of the caller-owned output stream.
pub fn write_22(dataset: &Dataset, mut writer: impl Write) -> Result<()> {
    dataset.validate()?;
    dataset.mesh.require_no_sets("E_MSH")?;
    let mesh = &dataset.mesh;
    if mesh.cells.iter().any(|cell| cell.property_id.is_some()) {
        return Err(err(
            "property IDs have no lossless MSH mapping in this writer",
        ));
    }
    if mesh.points.iter().any(|point| point.id > i32::MAX as u64)
        || mesh.cells.iter().any(|cell| cell.id > i32::MAX as u64)
    {
        return Err(err(
            "MSH 2.2 node and element IDs must fit signed 32-bit integers",
        ));
    }
    writeln!(
        writer,
        "$MeshFormat\n2.2 0 8\n$EndMeshFormat\n$Nodes\n{}",
        mesh.points.len()
    )?;
    for point in &mesh.points {
        writeln!(
            writer,
            "{} {} {} {}",
            point.id, point.position[0], point.position[1], point.position[2]
        )?;
    }
    writeln!(writer, "$EndNodes\n$Elements\n{}", mesh.cells.len())?;
    for cell in &mesh.cells {
        write!(writer, "{} {} 0", cell.id, code(cell.kind).0)?;
        for &index in &cell.connectivity {
            write!(writer, " {}", mesh.points[index].id)?;
        }
        writeln!(writer)?;
    }
    writeln!(writer, "$EndElements")?;
    write_fields(dataset, &mut writer)
}

/// Write the selected ASCII MSH dialect. Version 4.1 is the default.
///
/// # Errors
///
/// Returns a data, representation, or output-stream error.
pub fn write_version(dataset: &Dataset, version: Version, writer: impl Write) -> Result<()> {
    match version {
        Version::V2_2 => write_22(dataset, writer),
        Version::V4_1 => write(dataset, writer),
    }
}
