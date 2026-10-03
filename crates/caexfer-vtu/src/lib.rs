//! ASCII VTK XML UnstructuredGrid linear mesh and numeric-field I/O.
//!
//! Binary, compressed, appended, parallel and multi-piece layouts are outside
//! this bounded reader/writer.

use caexfer_core::{Cell, CellKind, Dataset, Error, Field, FieldLocation, Mesh, Point, Result};
use std::collections::BTreeSet;
use std::io::Write;

fn vtk_type(kind: CellKind) -> u8 {
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

/// Write geometry and original node/element/property IDs.
/// Topology is validated before any bytes are written. A writer I/O failure may
/// still leave partial output; the CLI uses a staged no-clobber file operation.
/// Input node ordering must be VTK-compatible for the specified linear cells.
pub fn write(mesh: &Mesh, writer: impl Write) -> Result<()> {
    write_data(
        &Dataset {
            mesh: mesh.clone(),
            fields: Vec::new(),
        },
        writer,
    )
}

/// Write one dataset as an ASCII VTU piece. Step and time metadata are retained
/// in caexfer attributes on numeric arrays.
pub fn write_data(dataset: &Dataset, mut writer: impl Write) -> Result<()> {
    dataset.validate()?;
    let mesh = &dataset.mesh;
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
    let mut entries = 0usize;
    for cell in &mesh.cells {
        entries = entries
            .checked_add(cell.connectivity.len())
            .ok_or_else(|| Error::new("E_LIMIT", "connectivity size overflows usize"))?;
    }
    i64::try_from(entries).map_err(|_| Error::new("E_LIMIT", "VTU offsets exceed Int64"))?;
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
        write_field(field, &mut writer)?;
    }
    writeln!(writer, "</PointData><CellData>")?;
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
        writeln!(writer, "{}", cell.property_id.unwrap_or(0))?;
    }
    writeln!(writer, "</DataArray>")?;
    for field in dataset
        .fields
        .iter()
        .filter(|f| f.location == FieldLocation::Cell)
    {
        write_field(field, &mut writer)?;
    }
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
    let mut offset = 0;
    for cell in &mesh.cells {
        offset += cell.connectivity.len();
        writeln!(writer, "{offset}")?;
    }
    writeln!(
        writer,
        "</DataArray><DataArray type=\"UInt8\" Name=\"types\" format=\"ascii\">"
    )?;
    for cell in &mesh.cells {
        writeln!(writer, "{}", vtk_type(cell.kind))?;
    }
    writeln!(
        writer,
        "</DataArray></Cells></Piece></UnstructuredGrid></VTKFile>"
    )?;
    Ok(())
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
fn write_field(field: &Field, writer: &mut impl Write) -> Result<()> {
    write!(
        writer,
        "<DataArray type=\"Float64\" Name=\"{}\" NumberOfComponents=\"{}\" format=\"ascii\"",
        escape(&field.name),
        field.components.len()
    )?;
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
    for chunk in field.values.chunks(field.components.len()) {
        for value in chunk {
            write!(writer, "{value} ")?;
        }
        writeln!(writer)?;
    }
    writeln!(writer, "</DataArray>")?;
    Ok(())
}

fn unescape(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}
fn attr<'a>(tag: &'a str, key: &str) -> Option<&'a str> {
    let pattern = format!("{key}=\"");
    let start = tag.find(&pattern)? + pattern.len();
    let end = tag[start..].find('"')? + start;
    Some(&tag[start..end])
}
fn element<'a>(source: &'a str, tag: &str) -> Result<(&'a str, &'a str)> {
    let start = source
        .find(&format!("<{tag}"))
        .ok_or_else(|| Error::new("E_VTU", format!("missing {tag}")))?;
    let open_end = source[start..]
        .find('>')
        .ok_or_else(|| Error::new("E_VTU", "unclosed XML tag"))?
        + start;
    let close = format!("</{tag}>");
    let end = source[open_end + 1..]
        .find(&close)
        .ok_or_else(|| Error::new("E_VTU", format!("unclosed {tag}")))?
        + open_end
        + 1;
    Ok((&source[start..=open_end], &source[open_end + 1..end]))
}
fn optional_body<'a>(source: &'a str, tag: &str) -> Result<&'a str> {
    if !source.contains(&format!("<{tag}")) {
        return Ok("");
    }
    let start = source.find(&format!("<{tag}")).unwrap_or(0);
    let open_end = source[start..]
        .find('>')
        .ok_or_else(|| Error::new("E_VTU", "unclosed XML tag"))?
        + start;
    if source[start..=open_end].ends_with("/>") {
        return Ok("");
    }
    Ok(element(source, tag)?.1)
}
fn arrays(body: &str) -> Result<Vec<(&str, &str)>> {
    let mut out = Vec::new();
    let mut rest = body;
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
fn numbers<T: std::str::FromStr>(body: &str) -> Result<Vec<T>> {
    body.split_whitespace()
        .map(|v| {
            v.parse()
                .map_err(|_| Error::new("E_VTU", "invalid numeric array"))
        })
        .collect()
}
fn array<'a>(body: &'a str, name: &str) -> Result<(&'a str, &'a str)> {
    arrays(body)?
        .into_iter()
        .find(|(tag, _)| attr(tag, "Name") == Some(name))
        .ok_or_else(|| Error::new("E_VTU", format!("missing array {name}")))
}
fn cell_kind(code: u8) -> Result<CellKind> {
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

/// Read one ASCII UnstructuredGrid piece with linear cells and complete Float32/Float64 fields.
pub fn read(source: &str) -> Result<Dataset> {
    if source.contains("<AppendedData") || source.contains("<FieldData") {
        return Err(Error::new(
            "E_VTU",
            "AppendedData and FieldData are outside this reader's scope",
        ));
    }
    let (file_tag, file_body) = element(source, "VTKFile")?;
    if attr(file_tag, "type") != Some("UnstructuredGrid") || file_tag.contains("compressor=") {
        return Err(Error::new(
            "E_VTU",
            "unsupported VTKFile type or compression",
        ));
    }
    let (_, grid) = element(file_body, "UnstructuredGrid")?;
    if grid.matches("<Piece").count() != 1 {
        return Err(Error::new("E_VTU", "exactly one Piece is required"));
    }
    let (piece_tag, piece) = element(grid, "Piece")?;
    let point_count: usize = attr(piece_tag, "NumberOfPoints")
        .ok_or_else(|| Error::new("E_VTU", "missing point count"))?
        .parse()
        .map_err(|_| Error::new("E_VTU", "invalid point count"))?;
    let cell_count: usize = attr(piece_tag, "NumberOfCells")
        .ok_or_else(|| Error::new("E_VTU", "missing cell count"))?
        .parse()
        .map_err(|_| Error::new("E_VTU", "invalid cell count"))?;
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
    let points = point_ids
        .into_iter()
        .zip(xyz.chunks_exact(3))
        .map(|(id, p)| Point {
            id,
            position: [p[0], p[1], p[2]],
        })
        .collect();
    let (_, cell_body) = element(piece, "Cells")?;
    let connectivity: Vec<usize> = numbers(array(cell_body, "connectivity")?.1)?;
    let offsets: Vec<usize> = numbers(array(cell_body, "offsets")?.1)?;
    let types: Vec<u8> = numbers(array(cell_body, "types")?.1)?;
    if offsets.len() != cell_count || types.len() != cell_count {
        return Err(Error::new("E_VTU", "cell count mismatch"));
    }
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
    if start != connectivity.len() {
        return Err(Error::new("E_VTU", "unused connectivity"));
    }
    let mut dataset = Dataset {
        mesh: Mesh { points, cells },
        fields: Vec::new(),
    };
    for (location, body, expected) in [
        (FieldLocation::Point, pd, point_count),
        (FieldLocation::Cell, cd, cell_count),
    ] {
        for (tag, data) in arrays(body)? {
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
            let values: Vec<f64> = numbers(data)?;
            if values.len() != count * expected {
                return Err(Error::new("E_VTU", "field value count mismatch"));
            }
            let components = (0..count)
                .map(|i| {
                    attr(tag, &format!("ComponentName{i}"))
                        .map(unescape)
                        .unwrap_or_else(|| format!("C{}", i + 1))
                })
                .collect();
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
    let mut names = BTreeSet::new();
    for f in &dataset.fields {
        if !names.insert((f.location as u8, f.name.clone())) {
            return Err(Error::new("E_VTU", "duplicate field name"));
        }
    }
    dataset.validate()?;
    Ok(dataset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use caexfer_core::{Cell, Point};

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
