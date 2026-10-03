//! ASCII Gmsh MSH 4.1 linear mesh and numeric data blocks.
use crate::core::{Cell, CellKind, Dataset, Error, Field, FieldLocation, Mesh, Point, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

/// Attach the MSH-specific diagnostic code to a parsing or writing failure.
fn err(message: impl Into<String>) -> Error {
    Error::new("E_MSH", message)
}
/// Parse a required token, preserving its field name in the diagnostic.
fn number<T: std::str::FromStr>(value: Option<&str>, what: &str) -> Result<T> {
    value
        .ok_or_else(|| err(format!("missing {what}")))?
        .parse()
        .map_err(|_| err(format!("invalid {what}")))
}
/// Decode a Gmsh linear element code; reject unsupported element orders.
fn kind(code: u32) -> Result<CellKind> {
    match code {
        1 => Ok(CellKind::Line2),
        2 => Ok(CellKind::Triangle3),
        3 => Ok(CellKind::Quad4),
        4 => Ok(CellKind::Tet4),
        5 => Ok(CellKind::Hex8),
        6 => Ok(CellKind::Wedge6),
        7 => Ok(CellKind::Pyramid5),
        _ => Err(err(format!("unsupported element type {code}"))),
    }
}
/// Return the Gmsh element code and topological dimension for a cell.
fn code(kind: CellKind) -> (u32, u32) {
    match kind {
        CellKind::Line2 => (1, 1),
        CellKind::Triangle3 => (2, 2),
        CellKind::Quad4 => (3, 2),
        CellKind::Tet4 => (4, 3),
        CellKind::Hex8 => (5, 3),
        CellKind::Wedge6 => (6, 3),
        CellKind::Pyramid5 => (7, 3),
    }
}
/// Borrow the first named section body, checking that its end marker exists.
fn section<'a>(source: &'a str, name: &str) -> Result<Option<&'a str>> {
    // A missing section is optional to callers; a present truncated one is not.
    let begin = format!("${name}\n");
    let end = format!("$End{name}");
    let Some(start) = source.find(&begin) else {
        return Ok(None);
    };
    let body_start = start + begin.len();
    let stop = source[body_start..]
        .find(&end)
        .ok_or_else(|| err(format!("unterminated ${name}")))?
        + body_start;
    Ok(Some(&source[body_start..stop]))
}
/// Borrow all repeated field sections in source order, rejecting truncation.
fn data_sections<'a>(source: &'a str, name: &str) -> Result<Vec<&'a str>> {
    // NodeData and ElementData may occur once per field and step.
    let mut out = Vec::new();
    let mut rest = source;
    let begin = format!("${name}\n");
    let end = format!("$End{name}");
    while let Some(start) = rest.find(&begin) {
        rest = &rest[start + begin.len()..];
        let stop = rest
            .find(&end)
            .ok_or_else(|| err(format!("unterminated ${name}")))?;
        out.push(&rest[..stop]);
        rest = &rest[stop + end.len()..];
    }
    Ok(out)
}

/// Read an ASCII MSH 4.1 file with linear elements and numeric data blocks.
///
/// Binary files, parametric nodes, and unknown element types are rejected.
/// Node and element tags remain the original IDs; connectivity becomes indices
/// into [`Mesh::points`].
///
/// # Examples
///
/// ```
/// use caexfer::{core::Dataset, inp, msh};
/// let mesh = inp::read("*NODE\n1,0,0,0\n2,1,0,0\n*ELEMENT, TYPE=T3D2\n10,1,2\n")?.mesh;
/// let mut bytes = Vec::new();
/// msh::write(&Dataset { mesh, fields: vec![] }, &mut bytes)?;
/// let decoded = msh::read(std::str::from_utf8(&bytes).unwrap())?;
/// assert_eq!(decoded.mesh.cells[0].id, 10);
/// assert_eq!(decoded.mesh.cells[0].connectivity, vec![0, 1]);
/// # Ok::<(), caexfer::core::Error>(())
/// ```
pub fn read(source: &str) -> Result<Dataset> {
    // Normalize line endings before looking for exact section delimiters.
    let normalized = source.replace("\r\n", "\n");
    let source = normalized.as_str();
    let header = section(source, "MeshFormat")?.ok_or_else(|| err("missing MeshFormat"))?;
    let mut h = header.split_whitespace();
    if h.next() != Some("4.1") || h.next() != Some("0") || h.next() != Some("8") {
        return Err(err("only ASCII MSH 4.1 with 8-byte data size is supported"));
    }

    // Gmsh stores node tags before coordinate tuples within each block.
    // Build a tag-to-index map for later element connectivity.
    let mut mesh = Mesh::default();
    let mut node_ids = BTreeMap::new();
    let nodes = section(source, "Nodes")?.ok_or_else(|| err("missing Nodes"))?;
    let mut t = nodes.split_whitespace();
    let blocks: usize = number(t.next(), "node block count")?;
    let total: usize = number(t.next(), "node count")?;
    let _: u64 = number(t.next(), "minimum node tag")?;
    let _: u64 = number(t.next(), "maximum node tag")?;
    if blocks > t.clone().count() / 4 || total > t.clone().count() / 4 {
        return Err(err("node counts exceed available input"));
    }
    for _ in 0..blocks {
        let _: u32 = number(t.next(), "entity dimension")?;
        let _: u64 = number(t.next(), "entity tag")?;
        let parametric: u8 = number(t.next(), "parametric flag")?;
        if parametric != 0 {
            return Err(err("parametric nodes are unsupported"));
        }
        let count: usize = number(t.next(), "block node count")?;
        if count > total.saturating_sub(mesh.points.len()) || count > t.clone().count() / 4 {
            return Err(err("block node count exceeds available input"));
        }
        let mut tags = Vec::with_capacity(count);
        for _ in 0..count {
            tags.push(number::<u64>(t.next(), "node tag")?);
        }
        for id in tags {
            let position = [
                number(t.next(), "x")?,
                number(t.next(), "y")?,
                number(t.next(), "z")?,
            ];
            if node_ids.insert(id, mesh.points.len()).is_some() {
                return Err(err("duplicate node tag"));
            }
            mesh.points.push(Point { id, position });
        }
    }
    if mesh.points.len() != total {
        return Err(err("node count mismatch"));
    }

    // Element blocks are homogeneous in type and entity dimension.
    let elements = section(source, "Elements")?.ok_or_else(|| err("missing Elements"))?;
    let mut t = elements.split_whitespace();
    let blocks: usize = number(t.next(), "element block count")?;
    let total: usize = number(t.next(), "element count")?;
    let _: u64 = number(t.next(), "minimum element tag")?;
    let _: u64 = number(t.next(), "maximum element tag")?;
    if blocks > t.clone().count() / 4 || total > t.clone().count() / 3 {
        return Err(err("element counts exceed available input"));
    }
    for _ in 0..blocks {
        let dim: u32 = number(t.next(), "entity dimension")?;
        let _: u64 = number(t.next(), "entity tag")?;
        let type_code: u32 = number(t.next(), "element type")?;
        let cell_kind = kind(type_code)?;
        if dim != code(cell_kind).1 {
            return Err(err("element dimension mismatch"));
        }
        let count: usize = number(t.next(), "block element count")?;
        let tokens_per_element = cell_kind.node_count() + 1;
        if count > total.saturating_sub(mesh.cells.len())
            || count > t.clone().count() / tokens_per_element
        {
            return Err(err("block element count exceeds available input"));
        }
        for _ in 0..count {
            let id = number(t.next(), "element tag")?;
            let mut connectivity = Vec::with_capacity(cell_kind.node_count());
            for _ in 0..cell_kind.node_count() {
                let node: u64 = number(t.next(), "element node")?;

                // The mesh model stores point indices, not Gmsh node tags.
                connectivity.push(
                    *node_ids
                        .get(&node)
                        .ok_or_else(|| err(format!("unknown node {node}")))?,
                );
            }
            mesh.cells.push(Cell {
                id,
                kind: cell_kind,
                connectivity,
                property_id: None,
            });
        }
    }
    if mesh.cells.len() != total {
        return Err(err("element count mismatch"));
    }
    let mut dataset = Dataset {
        mesh,
        fields: Vec::new(),
    };

    // Numeric data blocks identify entities by original tag and can appear
    // in an order different from the geometry arrays.
    for (name, location) in [
        ("NodeData", FieldLocation::Point),
        ("ElementData", FieldLocation::Cell),
    ] {
        for body in data_sections(source, name)? {
            let mut lines = body.lines().map(str::trim).filter(|line| !line.is_empty());
            let strings: usize = number(lines.next(), "string tag count")?;
            if strings != 1 {
                return Err(err("data block requires exactly one name tag"));
            }
            let raw_name = lines.next().ok_or_else(|| err("missing field name"))?;
            if !raw_name.starts_with('"') || !raw_name.ends_with('"') {
                return Err(err("field name must be quoted"));
            }
            let field_name = raw_name[1..raw_name.len() - 1].to_string();
            let reals: usize = number(lines.next(), "real tag count")?;
            let time: Option<f64> = match reals {
                0 => None,
                1 => Some(number(lines.next(), "time")?),
                _ => return Err(err("data block supports at most one time tag")),
            };
            let ints: usize = number(lines.next(), "integer tag count")?;
            if ints != 3 {
                return Err(err("data block requires three integer tags"));
            }
            let step: i64 = number(lines.next(), "step")?;
            let components: usize = number(lines.next(), "component count")?;
            let entries: usize = number(lines.next(), "data entry count")?;
            let remainder = lines.collect::<Vec<_>>().join(" ");
            let mut t = remainder.split_whitespace();
            if components == 0 {
                return Err(err("zero-component field"));
            }
            let values_len = entries
                .checked_mul(components)
                .ok_or_else(|| err("data value count overflows usize"))?;
            if components > source.len() || entries > t.clone().count() / (components + 1) {
                return Err(err("data counts exceed available input"));
            }
            let ids: Vec<u64> = match location {
                FieldLocation::Point => dataset.mesh.points.iter().map(|p| p.id).collect(),
                FieldLocation::Cell => dataset.mesh.cells.iter().map(|c| c.id).collect(),
            };
            if entries != ids.len() {
                return Err(err(
                    "partial data blocks cannot be projected without missing-value semantics",
                ));
            }

            // Reorder each complete block into the mesh's entity-major order.
            let index: BTreeMap<u64, usize> =
                ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
            let mut values = vec![0.; values_len];
            let mut seen = BTreeSet::new();
            for _ in 0..entries {
                let id: u64 = number(t.next(), "data entity tag")?;
                let at = *index
                    .get(&id)
                    .ok_or_else(|| err("data references an unknown entity"))?;
                if !seen.insert(id) {
                    return Err(err("duplicate data entity"));
                }
                for component in 0..components {
                    values[at * components + component] = number(t.next(), "data value")?;
                }
            }
            dataset.fields.push(Field {
                name: field_name,
                location,
                components: (0..components).map(|i| format!("C{}", i + 1)).collect(),
                values,
                step: Some(step),
                time,
            });
        }
    }
    dataset.validate()?;
    Ok(dataset)
}

/// Write ASCII MSH 4.1 geometry and complete numeric fields.
///
/// Property IDs cannot be represented by this writer and cause an error.
/// Validate the returned bytes with [`read`] when interoperability matters;
/// external Gmsh entity and physical-group semantics are outside this model.
pub fn write(dataset: &Dataset, mut writer: impl Write) -> Result<()> {
    // Finish validation and reject unrepresentable properties before writing.
    dataset.validate()?;
    if dataset.mesh.cells.iter().any(|c| c.property_id.is_some()) {
        return Err(err(
            "property IDs have no lossless MSH mapping in this writer",
        ));
    }
    let mesh = &dataset.mesh;

    // This bounded writer emits one global node block. Tags and coordinate
    // tuples are separate sequences in the MSH 4.1 block layout.
    writeln!(writer, "$MeshFormat\n4.1 0 8\n$EndMeshFormat\n$Nodes")?;
    let min = mesh.points.iter().map(|p| p.id).min().unwrap_or(0);
    let max = mesh.points.iter().map(|p| p.id).max().unwrap_or(0);
    writeln!(
        writer,
        "1 {} {min} {max}\n3 1 0 {}",
        mesh.points.len(),
        mesh.points.len()
    )?;
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
    writeln!(writer, "$EndNodes\n$Elements")?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_point_and_cell_fields_survive_msh_roundtrip() {
        let dataset = Dataset {
            mesh: Mesh {
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
                    property_id: None,
                }],
            },
            fields: vec![
                Field {
                    name: "Displacement".into(),
                    location: FieldLocation::Point,
                    components: vec!["C1".into(), "C2".into()],
                    values: vec![0., 1., 2., 3., 4., 5.],
                    step: Some(1),
                    time: Some(0.25),
                },
                Field {
                    name: "Energy".into(),
                    location: FieldLocation::Cell,
                    components: vec!["C1".into()],
                    values: vec![9.],
                    step: Some(1),
                    time: Some(0.25),
                },
            ],
        };
        let mut bytes = Vec::new();
        write(&dataset, &mut bytes).unwrap();
        assert_eq!(read(std::str::from_utf8(&bytes).unwrap()).unwrap(), dataset);
    }
}
