//! ASCII VTK XML UnstructuredGrid linear mesh and numeric-field I/O.
//!
//! Binary, compressed, appended, parallel and multi-piece layouts are outside
//! this bounded reader/writer.

use crate::core::{Cell, CellKind, Dataset, Error, Field, FieldLocation, Mesh, Point, Result};
use std::collections::BTreeSet;
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
/// # Examples
///
/// ```
/// use caexfer::{bdf::Document, vtu};
/// let mesh = Document::parse("GRID,1,,0,0,0\nGRID,2,,1,0,0\nCROD,10,7,1,2\n")?
///     .geometry()?.mesh;
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
pub fn write_data(dataset: &Dataset, mut writer: impl Write) -> Result<()> {
    // Finish all checks that can fail independently of I/O before emitting XML.
    dataset.validate()?;
    let mesh = &dataset.mesh;

    // These names carry original mesh IDs, so fields cannot replace them.
    for field in &dataset.fields {
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
fn unescape(value: &str) -> String {
    // Decode ampersands last so an encoded literal entity stays literal.
    value
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}
/// Borrow a quoted attribute value from a tag in the supported XML layout.
/// This parser is deliberately bounded and is not a general XML parser.
fn attr<'a>(tag: &'a str, key: &str) -> Option<&'a str> {
    // This bounded parser expects the writer's key="value" attribute form.
    let pattern = format!("{key}=\"");
    let start = tag.find(&pattern)? + pattern.len();

    // Return a slice of the original tag; callers decode entities as needed.
    let end = tag[start..].find('"')? + start;
    Some(&tag[start..end])
}
/// Borrow the opening tag and body of the first named element.
/// Missing or unterminated tags fail with `E_VTU`.
fn element<'a>(source: &'a str, tag: &str) -> Result<(&'a str, &'a str)> {
    // Only the first matching element is relevant within each expected section.
    let start = source
        .find(&format!("<{tag}"))
        .ok_or_else(|| Error::new("E_VTU", format!("missing {tag}")))?;
    let open_end = source[start..]
        .find('>')
        .ok_or_else(|| Error::new("E_VTU", "unclosed XML tag"))?
        + start;

    // Keep the opening tag for attributes and the body for nested arrays.
    let close = format!("</{tag}>");
    let end = source[open_end + 1..]
        .find(&close)
        .ok_or_else(|| Error::new("E_VTU", format!("unclosed {tag}")))?
        + open_end
        + 1;
    Ok((&source[start..=open_end], &source[open_end + 1..end]))
}
/// Return an optional element body, including empty self-closing elements.
/// Malformed present elements remain errors rather than acting as absent.
fn optional_body<'a>(source: &'a str, tag: &str) -> Result<&'a str> {
    // Missing PointData/CellData means there are no arrays in that section.
    if !source.contains(&format!("<{tag}")) {
        return Ok("");
    }
    let start = source.find(&format!("<{tag}")).unwrap_or(0);
    let open_end = source[start..]
        .find('>')
        .ok_or_else(|| Error::new("E_VTU", "unclosed XML tag"))?
        + start;

    // An empty self-closing section has the same data content as an absent one.
    if source[start..=open_end].ends_with("/>") {
        return Ok("");
    }

    // A present nonempty section must have its matching closing tag.
    Ok(element(source, tag)?.1)
}
/// Collect `DataArray` tags and bodies in encounter order within one section.
/// Each array must have a complete closing tag.
fn arrays(body: &str) -> Result<Vec<(&str, &str)>> {
    let mut out = Vec::new();
    let mut rest = body;

    // Advance past each closing tag so encounter order matches file order.
    while let Some(start) = rest.find("<DataArray") {
        rest = &rest[start..];
        let (tag, values) = element(rest, "DataArray")?;
        out.push((tag, values));
        let close = rest
            .find("</DataArray>")
            .ok_or_else(|| Error::new("E_VTU", "unclosed array"))?;
        rest = &rest[close + 12..];
    }
    Ok(out)
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
fn array<'a>(body: &'a str, name: &str) -> Result<(&'a str, &'a str)> {
    // Geometry arrays are identified by Name rather than by their file order.
    arrays(body)?
        .into_iter()
        .find(|(tag, _)| attr(tag, "Name") == Some(name))
        .ok_or_else(|| Error::new("E_VTU", format!("missing array {name}")))
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
pub fn read(source: &str) -> Result<Dataset> {
    // Reject sections that this ASCII, single-piece reader cannot interpret.
    if source.contains("<AppendedData") || source.contains("<FieldData") {
        return Err(Error::new(
            "E_VTU",
            "AppendedData and FieldData are outside this reader's scope",
        ));
    }

    // The outer file tag establishes the grid kind and whether compression
    // would require a different decoding path.
    let (file_tag, file_body) = element(source, "VTKFile")?;
    if attr(file_tag, "type") != Some("UnstructuredGrid") || file_tag.contains("compressor=") {
        return Err(Error::new(
            "E_VTU",
            "unsupported VTKFile type or compression",
        ));
    }
    let (_, grid) = element(file_body, "UnstructuredGrid")?;

    // Multiple pieces need a merge of point indices and field tuples.
    if grid.matches("<Piece").count() != 1 {
        return Err(Error::new("E_VTU", "exactly one Piece is required"));
    }
    let (piece_tag, piece) = element(grid, "Piece")?;

    // Declared counts are used to check every geometry and ID array below.
    let point_count: usize = attr(piece_tag, "NumberOfPoints")
        .ok_or_else(|| Error::new("E_VTU", "missing point count"))?
        .parse()
        .map_err(|_| Error::new("E_VTU", "invalid point count"))?;
    let cell_count: usize = attr(piece_tag, "NumberOfCells")
        .ok_or_else(|| Error::new("E_VTU", "missing cell count"))?
        .parse()
        .map_err(|_| Error::new("E_VTU", "invalid cell count"))?;

    // Points must be one ASCII array with three coordinates per tuple.
    let (_, point_body) = element(piece, "Points")?;
    let coords = arrays(point_body)?;
    if coords.len() != 1
        || attr(coords[0].0, "format") != Some("ascii")
        || attr(coords[0].0, "NumberOfComponents") != Some("3")
    {
        return Err(Error::new("E_VTU", "unsupported Points array"));
    }
    let xyz: Vec<f64> = numbers(coords[0].1)?;
    if xyz.len() != point_count * 3 {
        return Err(Error::new("E_VTU", "coordinate count mismatch"));
    }

    // Original node IDs are optional for external VTU files. Without them,
    // assign stable one-based IDs while retaining VTK's zero-based positions.
    let pd = optional_body(piece, "PointData")?;
    let point_ids: Vec<u64> = match arrays(pd)?
        .into_iter()
        .find(|(tag, _)| attr(tag, "Name") == Some("nastran_node_id"))
    {
        Some((tag, data)) => {
            if attr(tag, "type") != Some("UInt64") {
                return Err(Error::new("E_VTU", "node ID type must be UInt64"));
            }
            numbers(data)?
        }
        None => (1..=point_count as u64).collect(),
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
    let (_, cell_body) = element(piece, "Cells")?;
    let connectivity: Vec<usize> = numbers(array(cell_body, "connectivity")?.1)?;
    let offsets: Vec<usize> = numbers(array(cell_body, "offsets")?.1)?;
    let types: Vec<u8> = numbers(array(cell_body, "types")?.1)?;
    if offsets.len() != cell_count || types.len() != cell_count {
        return Err(Error::new("E_VTU", "cell count mismatch"));
    }

    // As with nodes, fall back to one-based element IDs if the source lacks
    // caexfer's original-ID array.
    let cd = optional_body(piece, "CellData")?;
    let element_ids: Vec<u64> = match arrays(cd)?
        .into_iter()
        .find(|(tag, _)| attr(tag, "Name") == Some("nastran_element_id"))
    {
        Some((tag, data)) => {
            if attr(tag, "type") != Some("UInt64") {
                return Err(Error::new("E_VTU", "element ID type must be UInt64"));
            }
            numbers(data)?
        }
        None => (1..=cell_count as u64).collect(),
    };

    // A zero property ID means no property; absent arrays use that sentinel.
    let properties: Vec<u64> = match arrays(cd)?
        .into_iter()
        .find(|(tag, _)| attr(tag, "Name") == Some("nastran_property_id"))
    {
        Some((tag, data)) => {
            if attr(tag, "type") != Some("UInt64") {
                return Err(Error::new("E_VTU", "property ID type must be UInt64"));
            }
            numbers(data)?
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
        for (tag, data) in arrays(body)? {
            // ID arrays have already supplied mesh identity; they are not fields.
            let name =
                attr(tag, "Name").ok_or_else(|| Error::new("E_VTU", "unnamed data array"))?;
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
            if attr(tag, "format") != Some("ascii")
                || !matches!(attr(tag, "type"), Some("Float64" | "Float32"))
            {
                return Err(Error::new("E_VTU", "only ASCII float fields are supported"));
            }
            let count: usize = attr(tag, "NumberOfComponents")
                .unwrap_or("1")
                .parse()
                .map_err(|_| Error::new("E_VTU", "invalid component count"))?;
            if count == 0 {
                return Err(Error::new("E_VTU", "zero-component field"));
            }

            // Every point or cell contributes exactly one complete tuple.
            let values: Vec<f64> = numbers(data)?;
            if values.len() != count * expected {
                return Err(Error::new("E_VTU", "field value count mismatch"));
            }

            // External files may omit component labels; synthesize C1, C2,
            // and so on without changing the numeric component order.
            let components = (0..count)
                .map(|i| {
                    attr(tag, &format!("ComponentName{i}"))
                        .map(unescape)
                        .unwrap_or_else(|| format!("C{}", i + 1))
                })
                .collect();

            // Preserve the writer's optional per-array step and time values.
            let step = attr(tag, "caexfer_step")
                .map(|v| v.parse().map_err(|_| Error::new("E_VTU", "invalid step")))
                .transpose()?;
            let time = attr(tag, "caexfer_time")
                .map(|v| v.parse().map_err(|_| Error::new("E_VTU", "invalid time")))
                .transpose()?;
            dataset.fields.push(Field {
                name: unescape(name),
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
}
