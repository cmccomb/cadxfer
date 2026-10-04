//! ASCII VTU writer with XML attribute validation.

use crate::core::{CellKind, Dataset, Error, Field, FieldLocation, Mesh, Result};
use std::io::Write;

/// Map a supported linear topology to VTK's unstructured-cell type number.
pub(super) fn vtk_type(kind: CellKind) -> u8 {
    // These are VTK's linear cell codes; mesh connectivity is already in VTK order.
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

/// Write geometry and original node, element, and property IDs.
///
/// This is a geometry-only projection; use [`write_data`] to include numeric
/// fields. The mesh is validated before writing, but an I/O failure can leave
/// partial bytes in the caller's stream. Connectivity order must already match
/// the VTK convention for each linear cell.
///
/// # Errors
///
/// Returns a mesh validation or VTU representation error, or an I/O error
/// while writing to the caller's stream.
///
/// # Examples
///
/// ```
/// use caexfer::formats::{bdf, vtu};
/// let mesh = bdf::mesh::read("GRID,1,,0,0,0\nGRID,2,,1,0,0\nCROD,10,7,1,2\n")?.mesh;
/// // Geometry-only output still retains the original node IDs.
/// let mut bytes = Vec::new();
/// vtu::write(&mesh, &mut bytes)?;
/// let decoded = vtu::read(std::str::from_utf8(&bytes).unwrap())?;
/// assert_eq!(decoded.mesh.points[0].id, 1);
/// assert!(decoded.fields.is_empty());
/// # Ok::<(), caexfer::core::Error>(())
/// ```
pub fn write(mesh: &Mesh, writer: impl Write) -> Result<()> {
    // Reuse the dataset writer so geometry-only output follows the same
    // validation, ID preservation, and XML layout as output with fields.
    write_data(
        &Dataset {
            mesh: mesh.clone(),
            fields: Vec::new(),
        },
        writer,
    )
}

/// Write one dataset as a single ASCII VTU piece.
///
/// Point and cell fields must be complete and numeric. Their step and time
/// metadata are stored in caexfer attributes on the VTK data arrays. Reserved
/// ID array names cannot be reused as field names. Validation completes before
/// the first write; a later stream error may still leave partial output.
///
/// # Errors
///
/// Returns an error for invalid mesh or fields, reserved field names, oversized
/// offsets, or an output-stream failure.
#[allow(clippy::too_many_lines)] // One ordered XML piece is written after preflight validation.
pub fn write_data(dataset: &Dataset, mut writer: impl Write) -> Result<()> {
    // Finish all checks that can fail independently of I/O before emitting XML.
    dataset.validate()?;
    dataset.mesh.require_no_sets("E_VTU")?;
    let mesh = &dataset.mesh;

    // These names carry original mesh IDs, so fields cannot replace them.
    for field in &dataset.fields {
        if !valid_xml_attribute(&field.name)
            || field
                .components
                .iter()
                .any(|name| !valid_xml_attribute(name))
        {
            return Err(Error::new(
                "E_VTU",
                "field name contains an invalid XML character",
            ));
        }
        if field.name == "nastran_node_id"
            || field.name == "nastran_element_id"
            || field.name == "nastran_property_id"
        {
            return Err(Error::new(
                "E_VTU",
                "field name conflicts with reserved ID array",
            ));
        }
    }

    // Offsets are cumulative connectivity lengths stored as signed Int64.
    // Check the sum now so the writing loop cannot overflow midway through.
    let mut entries = 0usize;
    for cell in &mesh.cells {
        entries = entries
            .checked_add(cell.connectivity.len())
            .ok_or_else(|| Error::new("E_LIMIT", "connectivity size overflows usize"))?;
    }
    i64::try_from(entries).map_err(|_| Error::new("E_LIMIT", "VTU offsets exceed Int64"))?;

    // A VTU file contains one grid with one piece; all arrays below belong
    // to this piece and have lengths implied by its point and cell counts.
    writeln!(writer, "<?xml version=\"1.0\"?>")?;
    writeln!(
        writer,
        "<VTKFile type=\"UnstructuredGrid\" version=\"0.1\" byte_order=\"LittleEndian\">"
    )?;

    writeln!(
        writer,
        "<!-- caexfer 0.1.0: geometry only; units unspecified -->"
    )?;
    writeln!(
        writer,
        "<UnstructuredGrid><Piece NumberOfPoints=\"{}\" NumberOfCells=\"{}\">",
        mesh.points.len(),
        mesh.cells.len()
    )?;

    // Preserve Nastran node IDs separately from VTK's zero-based point indices.
    writeln!(
        writer,
        "<PointData><DataArray type=\"UInt64\" Name=\"nastran_node_id\" format=\"ascii\">"
    )?;
    for point in &mesh.points {
        writeln!(writer, "{}", point.id)?;
    }
    writeln!(writer, "</DataArray>")?;
    for field in dataset
        .fields
        .iter()
        .filter(|f| f.location == FieldLocation::Point)
    {
        // Point fields must stay in the same order as the Points tuples.
        write_field(field, &mut writer)?;
    }
    writeln!(writer, "</PointData><CellData>")?;

    // Cell arrays follow mesh.cells order, just like the Cells section below.
    writeln!(
        writer,
        "<DataArray type=\"UInt64\" Name=\"nastran_element_id\" format=\"ascii\">"
    )?;
    for cell in &mesh.cells {
        writeln!(writer, "{}", cell.id)?;
    }
    writeln!(
        writer,
        "</DataArray><DataArray type=\"UInt64\" Name=\"nastran_property_id\" format=\"ascii\">"
    )?;
    for cell in &mesh.cells {
        // Zero is the file sentinel for a cell without a property ID.
        writeln!(writer, "{}", cell.property_id.unwrap_or(0))?;
    }
    writeln!(writer, "</DataArray>")?;
    for field in dataset
        .fields
        .iter()
        .filter(|f| f.location == FieldLocation::Cell)
    {
        // Keep per-cell field tuples aligned with the original cell IDs.
        write_field(field, &mut writer)?;
    }

    // VTK expects exactly three coordinates for each point in this layout.
    writeln!(
        writer,
        "</CellData><Points><DataArray type=\"Float64\" NumberOfComponents=\"3\" format=\"ascii\">"
    )?;
    for point in &mesh.points {
        writeln!(
            writer,
            "{} {} {}",
            point.position[0], point.position[1], point.position[2]
        )?;
    }
    writeln!(
        writer,
        "</DataArray></Points><Cells><DataArray type=\"Int64\" Name=\"connectivity\" format=\"ascii\">"
    )?;

    // Connectivity refers to positions in the Points array, not node IDs.
    for cell in &mesh.cells {
        for index in &cell.connectivity {
            write!(writer, "{index} ")?;
        }
        writeln!(writer)?;
    }
    writeln!(
        writer,
        "</DataArray><DataArray type=\"Int64\" Name=\"offsets\" format=\"ascii\">"
    )?;

    // Each offset is the exclusive end of one cell in the flat connectivity array.
    let mut offset = 0;
    for cell in &mesh.cells {
        offset += cell.connectivity.len();
        writeln!(writer, "{offset}")?;
    }
    writeln!(
        writer,
        "</DataArray><DataArray type=\"UInt8\" Name=\"types\" format=\"ascii\">"
    )?;

    // The type array has one VTK topology code per cell.
    for cell in &mesh.cells {
        writeln!(writer, "{}", vtk_type(cell.kind))?;
    }
    writeln!(
        writer,
        "</DataArray></Cells></Piece></UnstructuredGrid></VTKFile>"
    )?;
    Ok(())
}

pub(super) fn valid_xml_text(value: &str) -> bool {
    value.chars().all(|character| {
        matches!(character, '\t' | '\n' | '\r')
            || matches!(character as u32, 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x0001_0000..=0x0010_FFFF)
    })
}

fn valid_xml_attribute(value: &str) -> bool {
    valid_xml_text(value) && !value.contains(['\t', '\n', '\r'])
}

/// Escape the XML attribute characters emitted by this bounded ASCII writer.
/// Field and component names pass through here before interpolation into tags.
fn escape(value: &str) -> String {
    // Escape ampersands first so later replacements do not re-escape entities.
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Emit one complete numeric `DataArray` with caexfer step/time metadata.
/// The caller validates tuple lengths and finite values before writing begins.
fn write_field(field: &Field, writer: &mut impl Write) -> Result<()> {
    // The name and component labels are XML attributes; numeric metadata can
    // be written directly after dataset validation.
    write!(
        writer,
        "<DataArray type=\"Float64\" Name=\"{}\" NumberOfComponents=\"{}\" format=\"ascii\"",
        escape(&field.name),
        field.components.len()
    )?;

    // Step and time are caexfer attributes because VTK has no matching
    // per-array slots in this small interchange format.
    if let Some(step) = field.step {
        write!(writer, " caexfer_step=\"{step}\"")?;
    }
    if let Some(time) = field.time {
        write!(writer, " caexfer_time=\"{time}\"")?;
    }
    for (i, component) in field.components.iter().enumerate() {
        write!(writer, " ComponentName{i}=\"{}\"", escape(component))?;
    }
    writeln!(writer, ">")?;

    // One line per tuple makes each field value group easy to inspect, while
    // the reader accepts any ASCII whitespace between numeric values.
    for chunk in field.values.chunks(field.components.len()) {
        for value in chunk {
            write!(writer, "{value} ")?;
        }
        writeln!(writer)?;
    }
    writeln!(writer, "</DataArray>")?;
    Ok(())
}
