//! ASCII VTK XML UnstructuredGrid linear mesh and numeric-field I/O.
//!
//! Binary, compressed, appended, parallel and multi-piece layouts are outside
//! this bounded reader/writer.

use crate::core::{Cell, CellKind, Dataset, Error, Field, FieldLocation, Mesh, Point, Result};
use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

/// Map a supported linear topology to VTK's unstructured-cell type number.
fn vtk_type(kind: CellKind) -> u8 {
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
/// use caexfer::{bdf::Document, vtu};
/// let mesh = Document::parse("GRID,1,,0,0,0\nGRID,2,,1,0,0\nCROD,10,7,1,2\n")?
///     .geometry()?.mesh;
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
pub fn write_data(dataset: &Dataset, mut writer: impl Write) -> Result<()> {
    // Finish all checks that can fail independently of I/O before emitting XML.
    dataset.validate()?;
    let mesh = &dataset.mesh;

    // These names carry original mesh IDs, so fields cannot replace them.
    for field in &dataset.fields {
        if !valid_xml_text(&field.name) || field.components.iter().any(|name| !valid_xml_text(name))
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
    writeln!(writer, "</DataArray></Points><Cells><DataArray type=\"Int64\" Name=\"connectivity\" format=\"ascii\">")?;

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

fn valid_xml_text(value: &str) -> bool {
    value.chars().all(|character| {
        matches!(character, '\t' | '\n' | '\r')
            || matches!(character as u32, 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF)
    })
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

/// Decode the small entity subset accepted in names and component attributes.
/// General XML entity processing is outside the supported VTU subset.
struct XmlNode {
    name: String,
    attributes: BTreeMap<String, String>,
    children: Vec<Self>,
    text: String,
}

impl XmlNode {
    fn attr(&self, key: &str) -> Option<&str> {
        self.attributes.get(key).map(String::as_str)
    }

    fn children(&self, name: &str) -> Result<Vec<&Self>> {
        if self.children.iter().any(|child| child.name != name) {
            return Err(Error::new("E_VTU", "unsupported XML element"));
        }
        Ok(self.children.iter().collect())
    }

    fn one(&self, name: &str) -> Result<&Self> {
        let mut matches = self.children.iter().filter(|child| child.name == name);
        let node = matches
            .next()
            .ok_or_else(|| Error::new("E_VTU", format!("missing {name}")))?;
        if matches.next().is_some() {
            return Err(Error::new("E_VTU", format!("duplicate {name}")));
        }
        Ok(node)
    }

    fn optional(&self, name: &str) -> Result<Option<&Self>> {
        let mut matches = self.children.iter().filter(|child| child.name == name);
        let node = matches.next();
        if matches.next().is_some() {
            return Err(Error::new("E_VTU", format!("duplicate {name}")));
        }
        Ok(node)
    }
}

fn xml_start(start: &BytesStart<'_>, reader: &Reader<&[u8]>) -> Result<XmlNode> {
    let name = std::str::from_utf8(start.name().as_ref())
        .map_err(|_| Error::new("E_VTU", "invalid XML element name"))?
        .to_owned();
    let mut attributes = BTreeMap::new();
    for attribute in start.attributes() {
        let attribute = attribute.map_err(|e| Error::new("E_VTU", e.to_string()))?;
        let key = std::str::from_utf8(attribute.key.as_ref())
            .map_err(|_| Error::new("E_VTU", "invalid XML attribute name"))?
            .to_owned();
        let value = attribute
            .decode_and_unescape_value(reader.decoder())
            .map_err(|e| Error::new("E_VTU", e.to_string()))?
            .into_owned();
        if attributes.insert(key, value).is_some() {
            return Err(Error::new("E_VTU", "duplicate XML attribute"));
        }
    }
    Ok(XmlNode {
        name,
        attributes,
        children: Vec::new(),
        text: String::new(),
    })
}

fn xml_tree(source: &str) -> Result<XmlNode> {
    let mut reader = Reader::from_str(source);
    let mut stack: Vec<XmlNode> = Vec::new();
    let mut root = None;
    loop {
        let event = reader
            .read_event()
            .map_err(|e| Error::new("E_VTU", e.to_string()))?;
        match event {
            Event::Start(start) => {
                if stack.len() >= 16 {
                    return Err(Error::new("E_VTU", "XML nesting limit exceeded"));
                }
                stack.push(xml_start(&start, &reader)?);
            }
            Event::Empty(start) => {
                let node = xml_start(&start, &reader)?;
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(node);
                } else if root.replace(node).is_some() {
                    return Err(Error::new("E_VTU", "multiple XML roots"));
                }
            }
            Event::End(end) => {
                let node = stack
                    .pop()
                    .ok_or_else(|| Error::new("E_VTU", "unexpected XML close"))?;
                if node.name.as_bytes() != end.name().as_ref() {
                    return Err(Error::new("E_VTU", "mismatched XML close"));
                }
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(node);
                } else if root.replace(node).is_some() {
                    return Err(Error::new("E_VTU", "multiple XML roots"));
                }
            }
            Event::Text(value) => {
                let text = value
                    .xml_content()
                    .map_err(|e| Error::new("E_VTU", e.to_string()))?;
                if let Some(node) = stack.last_mut() {
                    node.text.push_str(&text);
                } else if !text.trim().is_empty() {
                    return Err(Error::new("E_VTU", "text outside XML root"));
                }
            }
            Event::Comment(_) | Event::Decl(_) | Event::PI(_) => {}
            Event::Eof => break,
            Event::CData(_) | Event::DocType(_) | Event::GeneralRef(_) => {
                return Err(Error::new("E_VTU", "unsupported XML construct"));
            }
        }
    }
    if !stack.is_empty() {
        return Err(Error::new("E_VTU", "unclosed XML element"));
    }
    root.ok_or_else(|| Error::new("E_VTU", "missing VTKFile"))
}
/// Parse whitespace-separated ASCII array values as the required number type.
/// Invalid tokens map to the stable VTU diagnostic code.
fn numbers<T: std::str::FromStr>(body: &str) -> Result<Vec<T>> {
    // Parsing is shared by integer geometry arrays and floating-point fields.
    body.split_whitespace()
        .map(|v| {
            v.parse()
                .map_err(|_| Error::new("E_VTU", "invalid numeric array"))
        })
        .collect()
}
/// Find one required named `DataArray` among a section's arrays.
/// A missing array is an error because geometry cannot be reconstructed.
fn array<'a>(body: &'a XmlNode, name: &str) -> Result<&'a XmlNode> {
    let mut matches = body
        .children("DataArray")?
        .into_iter()
        .filter(|node| node.attr("Name") == Some(name));
    let found = matches
        .next()
        .ok_or_else(|| Error::new("E_VTU", format!("missing array {name}")))?;
    if matches.next().is_some() {
        return Err(Error::new("E_VTU", format!("duplicate array {name}")));
    }
    Ok(found)
}
/// Map a VTK cell type number to a supported linear topology.
/// Higher-order and unknown numbers fail instead of losing nodes.
fn cell_kind(code: u8) -> Result<CellKind> {
    // Reject unknown and higher-order codes instead of truncating connectivity.
    match code {
        3 => Ok(CellKind::Line2),
        5 => Ok(CellKind::Triangle3),
        9 => Ok(CellKind::Quad4),
        10 => Ok(CellKind::Tet4),
        12 => Ok(CellKind::Hex8),
        13 => Ok(CellKind::Wedge6),
        14 => Ok(CellKind::Pyramid5),
        _ => Err(Error::new("E_VTU", format!("unsupported cell type {code}"))),
    }
}

/// Read one ASCII `UnstructuredGrid` piece into a mesh and numeric fields.
///
/// Original IDs are taken from caexfer's ID arrays when present. Binary,
/// compressed, appended, multi-piece, and unsupported cell layouts fail with
/// [`Error`] rather than being silently omitted.
///
/// # Errors
///
/// Returns an error for unsupported VTU layouts, malformed numeric arrays,
/// inconsistent counts, or invalid reconstructed mesh and fields.
pub fn read(source: &str) -> Result<Dataset> {
    if !valid_xml_text(source) {
        return Err(Error::new("E_VTU", "invalid XML character"));
    }
    let file = xml_tree(source)?;
    if file.name != "VTKFile"
        || file.attr("type") != Some("UnstructuredGrid")
        || file.attr("compressor").is_some()
    {
        return Err(Error::new(
            "E_VTU",
            "unsupported VTKFile type or compression",
        ));
    }
    let grid = file.one("UnstructuredGrid")?;
    file.children("UnstructuredGrid")?;

    // Multiple pieces need a merge of point indices and field tuples.
    if grid.children("Piece")?.len() != 1 {
        return Err(Error::new("E_VTU", "exactly one Piece is required"));
    }
    let piece = grid.one("Piece")?;
    if piece.children.iter().any(|child| {
        !matches!(
            child.name.as_str(),
            "Points" | "Cells" | "PointData" | "CellData"
        )
    }) {
        return Err(Error::new("E_VTU", "unsupported Piece section"));
    }

    // Declared counts are used to check every geometry and ID array below.
    let point_count: usize = piece
        .attr("NumberOfPoints")
        .ok_or_else(|| Error::new("E_VTU", "missing point count"))?
        .parse()
        .map_err(|_| Error::new("E_VTU", "invalid point count"))?;
    let cell_count: usize = piece
        .attr("NumberOfCells")
        .ok_or_else(|| Error::new("E_VTU", "missing cell count"))?
        .parse()
        .map_err(|_| Error::new("E_VTU", "invalid cell count"))?;
    if point_count > source.len() / 3 || cell_count > source.len() / 3 {
        return Err(Error::new("E_VTU", "declared count exceeds input size"));
    }

    // Points must be one ASCII array with three coordinates per tuple.
    let point_body = piece.one("Points")?;
    let coords = point_body.children("DataArray")?;
    if coords.len() != 1
        || coords[0].attr("format") != Some("ascii")
        || coords[0].attr("NumberOfComponents") != Some("3")
    {
        return Err(Error::new("E_VTU", "unsupported Points array"));
    }
    let xyz: Vec<f64> = numbers(&coords[0].text)?;
    if xyz.len()
        != point_count
            .checked_mul(3)
            .ok_or_else(|| Error::new("E_VTU", "coordinate count overflows"))?
    {
        return Err(Error::new("E_VTU", "coordinate count mismatch"));
    }

    // Original node IDs are optional for external VTU files. Without them,
    // assign stable one-based IDs while retaining VTK's zero-based positions.
    let pd = piece.optional("PointData")?;
    let point_ids: Vec<u64> = match pd
        .map(|node| node.children("DataArray"))
        .transpose()?
        .unwrap_or_default()
        .into_iter()
        .find(|node| node.attr("Name") == Some("nastran_node_id"))
    {
        Some(node) => {
            if node.attr("type") != Some("UInt64") {
                return Err(Error::new("E_VTU", "node ID type must be UInt64"));
            }
            numbers(&node.text)?
        }
        None => (1..=u64::try_from(point_count)
            .map_err(|_| Error::new("E_VTU", "point count exceeds UInt64"))?)
            .collect(),
    };
    if point_ids.len() != point_count {
        return Err(Error::new("E_VTU", "node ID count mismatch"));
    }

    // Zip each ID with its three coordinates in Points array order.
    let points = point_ids
        .into_iter()
        .zip(xyz.chunks_exact(3))
        .map(|(id, p)| Point {
            id,
            position: [p[0], p[1], p[2]],
        })
        .collect();

    // Cells use flat connectivity plus cumulative offsets and one type per cell.
    let cell_body = piece.one("Cells")?;
    let connectivity: Vec<usize> = numbers(&array(cell_body, "connectivity")?.text)?;
    let offsets: Vec<usize> = numbers(&array(cell_body, "offsets")?.text)?;
    let types: Vec<u8> = numbers(&array(cell_body, "types")?.text)?;
    if offsets.len() != cell_count || types.len() != cell_count {
        return Err(Error::new("E_VTU", "cell count mismatch"));
    }

    // As with nodes, fall back to one-based element IDs if the source lacks
    // caexfer's original-ID array.
    let cd = piece.optional("CellData")?;
    let element_ids: Vec<u64> = match cd
        .map(|node| node.children("DataArray"))
        .transpose()?
        .unwrap_or_default()
        .into_iter()
        .find(|node| node.attr("Name") == Some("nastran_element_id"))
    {
        Some(node) => {
            if node.attr("type") != Some("UInt64") {
                return Err(Error::new("E_VTU", "element ID type must be UInt64"));
            }
            numbers(&node.text)?
        }
        None => (1..=u64::try_from(cell_count)
            .map_err(|_| Error::new("E_VTU", "cell count exceeds UInt64"))?)
            .collect(),
    };

    // A zero property ID means no property; absent arrays use that sentinel.
    let properties: Vec<u64> = match cd
        .map(|node| node.children("DataArray"))
        .transpose()?
        .unwrap_or_default()
        .into_iter()
        .find(|node| node.attr("Name") == Some("nastran_property_id"))
    {
        Some(node) => {
            if node.attr("type") != Some("UInt64") {
                return Err(Error::new("E_VTU", "property ID type must be UInt64"));
            }
            numbers(&node.text)?
        }
        None => vec![0; cell_count],
    };
    if element_ids.len() != cell_count || properties.len() != cell_count {
        return Err(Error::new("E_VTU", "cell ID count mismatch"));
    }

    // Each offset delimits the next slice of the flat connectivity array.
    // Monotonic, in-range offsets prevent slicing past the array.
    let mut cells = Vec::new();
    let mut start = 0;
    for (i, end) in offsets.into_iter().enumerate() {
        if end < start || end > connectivity.len() {
            return Err(Error::new("E_VTU", "invalid offset"));
        }
        cells.push(Cell {
            id: element_ids[i],
            kind: cell_kind(types[i])?,
            connectivity: connectivity[start..end].to_vec(),
            property_id: (properties[i] != 0).then_some(properties[i]),
        });
        start = end;
    }

    // The final offset must consume the entire connectivity array.
    if start != connectivity.len() {
        return Err(Error::new("E_VTU", "unused connectivity"));
    }
    let mut dataset = Dataset {
        mesh: Mesh { points, cells },
        fields: Vec::new(),
    };

    // Parse point and cell fields with the same rules, using each location's
    // tuple count to check the numeric payload.
    for (location, body, expected) in [
        (FieldLocation::Point, pd, point_count),
        (FieldLocation::Cell, cd, cell_count),
    ] {
        for node in body
            .map(|section| section.children("DataArray"))
            .transpose()?
            .unwrap_or_default()
        {
            // ID arrays have already supplied mesh identity; they are not fields.
            let name = node
                .attr("Name")
                .ok_or_else(|| Error::new("E_VTU", "unnamed data array"))?;
            if [
                "nastran_node_id",
                "nastran_element_id",
                "nastran_property_id",
            ]
            .contains(&name)
            {
                continue;
            }

            // This reader accepts numeric ASCII fields only, regardless of
            // what other DataArray types a larger VTK implementation supports.
            if node.attr("format") != Some("ascii")
                || !matches!(node.attr("type"), Some("Float64" | "Float32"))
            {
                return Err(Error::new("E_VTU", "only ASCII float fields are supported"));
            }
            let count: usize = node
                .attr("NumberOfComponents")
                .unwrap_or("1")
                .parse()
                .map_err(|_| Error::new("E_VTU", "invalid component count"))?;
            if count == 0 {
                return Err(Error::new("E_VTU", "zero-component field"));
            }

            // Every point or cell contributes exactly one complete tuple.
            let values: Vec<f64> = numbers(&node.text)?;
            if values.len()
                != count
                    .checked_mul(expected)
                    .ok_or_else(|| Error::new("E_VTU", "field count overflows"))?
            {
                return Err(Error::new("E_VTU", "field value count mismatch"));
            }
            if count > source.len() {
                return Err(Error::new("E_VTU", "component count exceeds input size"));
            }

            // External files may omit component labels; synthesize C1, C2,
            // and so on without changing the numeric component order.
            let components = (0..count)
                .map(|i| {
                    node.attr(&format!("ComponentName{i}"))
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("C{}", i + 1))
                })
                .collect();

            // Preserve the writer's optional per-array step and time values.
            let step = node
                .attr("caexfer_step")
                .map(|v| v.parse().map_err(|_| Error::new("E_VTU", "invalid step")))
                .transpose()?;
            let time = node
                .attr("caexfer_time")
                .map(|v| v.parse().map_err(|_| Error::new("E_VTU", "invalid time")))
                .transpose()?;
            dataset.fields.push(Field {
                name: name.to_owned(),
                location,
                components,
                values,
                step,
                time,
            });
        }
    }

    // A name may occur once per location, so point and cell fields can share
    // the same name while duplicates within either section are rejected.
    let mut names = BTreeSet::new();
    for f in &dataset.fields {
        if !names.insert((f.location as u8, f.name.clone())) {
            return Err(Error::new("E_VTU", "duplicate field name"));
        }
    }

    // Apply the core mesh and field invariants after rebuilding the dataset.
    dataset.validate()?;
    Ok(dataset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Cell, Point};

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
        }
    }

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

    #[test]
    fn invalid_mesh_writes_nothing() {
        let mut mesh = triangle();
        mesh.cells[0].connectivity[0] = 100;
        let mut output = Vec::new();
        assert!(write(&mesh, &mut output).is_err());
        assert!(output.is_empty());
    }

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
            .map(vtk_type),
            [3, 5, 9, 10, 12, 13, 14]
        );
    }

    #[test]
    fn preserves_large_ids_as_integers() {
        let mut mesh = triangle();
        mesh.points[0].id = 9_007_199_254_740_993;
        let mut output = Vec::new();
        write(&mesh, &mut output).unwrap();
        assert!(String::from_utf8(output)
            .unwrap()
            .contains("9007199254740993"));
    }

    #[test]
    fn propagates_writer_failure() {
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

    #[test]
    fn xml_attribute_names_match_exactly() {
        let mut bytes = Vec::new();
        write(&triangle(), &mut bytes).unwrap();
        let source = String::from_utf8(bytes)
            .unwrap()
            .replace("Name=\"connectivity\"", "XName=\"connectivity\"");
        assert_eq!(read(&source).unwrap_err().code, "E_VTU");
    }

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

    #[test]
    fn invalid_xml_names_fail_before_writing() {
        for invalid_name in ["bad\0name", "bad\u{1}name"] {
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
            assert!(output.is_empty());
            dataset.fields[0].name = "valid".into();
            dataset.fields[0].components[0] = invalid_name.into();
            assert_eq!(write_data(&dataset, &mut output).unwrap_err().code, "E_VTU");
            assert!(output.is_empty());
        }
    }
}
