//! Write ASCII legacy VTK geometry and complete numeric fields.

use super::read::err;
use crate::core::{CellKind, Dataset, FieldLocation, Mesh, Result};
use std::collections::BTreeSet;
use std::io::Write;

/// Encode a supported linear cell number.
fn cell_number(kind: CellKind) -> u8 {
    match kind {
        CellKind::Line2 => 3,
        CellKind::Triangle3 => 5,
        CellKind::Quad4 => 9,
        CellKind::Tet4 => 10,
        CellKind::Hex8 => 12,
        CellKind::Wedge6 => 13,
        CellKind::Pyramid5 => 14,
    }
}

/// Write ASCII legacy VTK version 2.0 with complete numeric fields.
///
/// Names must be single tokens. Step, time, and component labels have no
/// direct representation; conversion callers should report their omission.
///
/// # Errors
///
/// Returns an error for invalid datasets, unrepresentable indices or names,
/// or a failure from the caller-owned output stream.
#[allow(clippy::too_many_lines)] // One ordered legacy grid is emitted after preflight checks.
pub fn write_data(dataset: &Dataset, mut writer: impl Write) -> Result<()> {
    dataset.validate()?;
    dataset.mesh.require_no_sets("E_VTK")?;
    let mesh = &dataset.mesh;
    let mut cell_size = 0usize;
    for cell in &mesh.cells {
        cell_size = cell_size
            .checked_add(cell.connectivity.len() + 1)
            .ok_or_else(|| err("cell list overflow"))?;
    }
    i32::try_from(mesh.points.len()).map_err(|_| err("too many points for legacy VTK"))?;
    i32::try_from(mesh.cells.len()).map_err(|_| err("too many cells for legacy VTK"))?;
    i32::try_from(cell_size).map_err(|_| err("cell list exceeds legacy VTK int range"))?;
    let mut names = BTreeSet::new();
    for field in &dataset.fields {
        if field.name.is_empty()
            || field
                .name
                .chars()
                .any(|character| character.is_whitespace() || character.is_control())
            || matches!(
                field.name.as_str(),
                "nastran_node_id" | "nastran_element_id" | "nastran_property_id"
            )
            || !names.insert((field.location as u8, field.name.as_str()))
        {
            return Err(err("field name is not representable in legacy VTK"));
        }
    }
    writeln!(
        writer,
        "# vtk DataFile Version 2.0\ncaexfer geometry and numeric fields; units unspecified\nASCII\nDATASET UNSTRUCTURED_GRID"
    )?;
    writeln!(writer, "POINTS {} double", mesh.points.len())?;
    for point in &mesh.points {
        writeln!(
            writer,
            "{} {} {}",
            point.position[0], point.position[1], point.position[2]
        )?;
    }
    writeln!(writer, "CELLS {} {cell_size}", mesh.cells.len())?;
    for cell in &mesh.cells {
        write!(writer, "{}", cell.connectivity.len())?;
        for &index in &cell.connectivity {
            write!(writer, " {index}")?;
        }
        writeln!(writer)?;
    }
    writeln!(writer, "CELL_TYPES {}", mesh.cells.len())?;
    for cell in &mesh.cells {
        writeln!(writer, "{}", cell_number(cell.kind))?;
    }
    for location in [FieldLocation::Point, FieldLocation::Cell] {
        let count = if location == FieldLocation::Point {
            mesh.points.len()
        } else {
            mesh.cells.len()
        };
        let section = if location == FieldLocation::Point {
            "POINT_DATA"
        } else {
            "CELL_DATA"
        };
        let fields: Vec<_> = dataset
            .fields
            .iter()
            .filter(|field| field.location == location)
            .collect();
        let arrays = fields.len()
            + if location == FieldLocation::Point {
                1
            } else {
                2
            };
        writeln!(writer, "{section} {count}\nFIELD FieldData {arrays}")?;
        let ids = if location == FieldLocation::Point {
            mesh.points.iter().map(|point| point.id).collect::<Vec<_>>()
        } else {
            mesh.cells.iter().map(|cell| cell.id).collect::<Vec<_>>()
        };
        let id_name = if location == FieldLocation::Point {
            "nastran_node_id"
        } else {
            "nastran_element_id"
        };
        writeln!(writer, "{id_name} 1 {count} unsigned_long_long")?;
        for id in ids {
            writeln!(writer, "{id}")?;
        }
        if location == FieldLocation::Cell {
            writeln!(writer, "nastran_property_id 1 {count} unsigned_long_long")?;
            for cell in &mesh.cells {
                writeln!(writer, "{}", cell.property_id.unwrap_or(0))?;
            }
        }
        for field in fields {
            writeln!(
                writer,
                "{} {} {count} double",
                field.name,
                field.components.len()
            )?;
            for tuple in field.values.chunks(field.components.len()) {
                for value in tuple {
                    write!(writer, "{value} ")?;
                }
                writeln!(writer)?;
            }
        }
    }
    Ok(())
}

/// Write geometry and original IDs without numeric fields.
///
/// # Errors
///
/// Returns a dataset validation, representation, or output-stream error.
pub fn write(mesh: &Mesh, writer: impl Write) -> Result<()> {
    write_data(
        &Dataset {
            mesh: mesh.clone(),
            fields: Vec::new(),
        },
        writer,
    )
}
