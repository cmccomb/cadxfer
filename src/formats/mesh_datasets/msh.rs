//! ASCII Gmsh MSH 4.1 and 2.2 linear mesh and numeric data blocks.
use crate::core::{
    Cell, CellKind, CellSet, Dataset, Error, Field, FieldLocation, Mesh, NodeSet, Point, Result,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
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

/// Mesh projection and metadata that had no native group mapping.
#[derive(Debug, Clone, PartialEq)]
pub struct Projection {
    /// Complete supported geometry and numeric fields.
    pub dataset: Dataset,

    /// Elements with nonzero geometrical or other unmapped 2.2 tags.
    pub tagged_elements: usize,

    /// Physical groups assigned deterministic names because none were stored.
    pub generated_group_names: usize,
}

/// One MSH 2.2 element before regrouping into internal 4.1 blocks.
struct Element22 {
    /// Original element tag.
    id: u64,

    /// Supported linear Gmsh type code.
    code: u32,

    /// Original node tags in Gmsh ordering.
    nodes: Vec<u64>,

    /// First metadata tag, when it is a positive physical group number.
    physical: Option<u64>,
}

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

/// Decode explicit physical group names, retaining spaces inside quotes.
fn physical_names(source: &str) -> Result<BTreeMap<(u8, u64), String>> {
    let Some(body) = section(source, "PhysicalNames")? else {
        return Ok(BTreeMap::new());
    };
    let mut lines = body.lines().map(str::trim).filter(|line| !line.is_empty());
    let count: usize = number(lines.next(), "physical name count")?;
    if count > body.len() / 5 {
        return Err(err("physical name count exceeds input"));
    }
    let mut names = BTreeMap::new();
    for _ in 0..count {
        let line = lines.next().ok_or_else(|| err("missing physical name"))?;
        let mut words = line.split_whitespace();
        let dimension: u8 = number(words.next(), "physical dimension")?;
        let tag: u64 = number(words.next(), "physical tag")?;
        let start = line
            .find('"')
            .ok_or_else(|| err("physical name must be quoted"))?;
        let end = line
            .rfind('"')
            .ok_or_else(|| err("physical name must be quoted"))?;
        if start == end || dimension > 3 || tag == 0 {
            return Err(err("invalid physical name record"));
        }
        let name = line[start + 1..end].to_owned();
        if name.is_empty() || names.insert((dimension, tag), name).is_some() {
            return Err(err("empty or duplicate physical group name"));
        }
    }
    if lines.next().is_some() {
        return Err(err("extra physical name records"));
    }
    Ok(names)
}

/// Read entity physical tags from the structured MSH 4.1 entity section.
fn entity_physical_tags(source: &str) -> Result<BTreeMap<(u8, u64), Vec<u64>>> {
    let Some(body) = section(source, "Entities")? else {
        return Ok(BTreeMap::new());
    };
    let mut lines = body.lines().map(str::trim).filter(|line| !line.is_empty());
    let counts = lines
        .next()
        .ok_or_else(|| err("missing entity counts"))?
        .split_whitespace()
        .map(|word| {
            word.parse::<usize>()
                .map_err(|_| err("invalid entity count"))
        })
        .collect::<Result<Vec<_>>>()?;
    let counts: [usize; 4] = counts
        .try_into()
        .map_err(|_| err("four entity counts required"))?;
    let declared = counts
        .iter()
        .try_fold(0_usize, |total, count| total.checked_add(*count))
        .ok_or_else(|| err("entity count overflows"))?;
    if declared > body.len() / 5 {
        return Err(err("entity count exceeds input"));
    }
    let mut entities = BTreeMap::new();
    for (dimension, count) in counts.into_iter().enumerate() {
        for _ in 0..count {
            let words = lines
                .next()
                .ok_or_else(|| err("missing entity record"))?
                .split_whitespace()
                .collect::<Vec<_>>();
            let prefix = if dimension == 0 { 4 } else { 7 };
            if words.len() <= prefix {
                return Err(err("short entity record"));
            }
            let tag: u64 = number(words.first().copied(), "entity tag")?;
            let physical_count: usize =
                number(words.get(prefix).copied(), "entity physical count")?;
            if physical_count > words.len().saturating_sub(prefix + 1) {
                return Err(err("entity physical count exceeds record"));
            }
            let physical = words[prefix + 1..prefix + 1 + physical_count]
                .iter()
                .map(|word| {
                    word.parse::<u64>()
                        .map_err(|_| err("invalid entity physical tag"))
                })
                .collect::<Result<Vec<_>>>()?;
            let dimension = u8::try_from(dimension).map_err(|_| err("invalid entity dimension"))?;
            if entities.insert((dimension, tag), physical).is_some() {
                return Err(err("duplicate entity tag"));
            }
        }
    }
    if lines.next().is_some() {
        return Err(err("extra entity records"));
    }
    Ok(entities)
}

/// Attach overlapping physical groups to the cells they select.
fn attach_cell_sets(
    mesh: &mut Mesh,
    names: &BTreeMap<(u8, u64), String>,
    groups: BTreeMap<(u8, u64), Vec<u64>>,
) -> usize {
    let mut generated = 0;
    for ((dimension, tag), cell_ids) in groups {
        if cell_ids.is_empty() {
            continue;
        }
        let name = names.get(&(dimension, tag)).cloned().unwrap_or_else(|| {
            generated += 1;
            format!("physical_{dimension}_{tag}")
        });
        mesh.cell_sets.push(CellSet {
            name,
            dimension,
            cell_ids,
        });
    }
    generated
}

/// Attach physical point groups from dimension-zero node entities.
fn attach_node_sets(
    mesh: &mut Mesh,
    names: &BTreeMap<(u8, u64), String>,
    groups: BTreeMap<u64, Vec<u64>>,
) -> usize {
    let mut generated = 0;
    for (tag, point_ids) in groups {
        if point_ids.is_empty() {
            continue;
        }
        let name = names.get(&(0, tag)).cloned().unwrap_or_else(|| {
            generated += 1;
            format!("physical_0_{tag}")
        });
        mesh.node_sets.push(NodeSet { name, point_ids });
    }
    generated
}

/// Read an ASCII MSH 4.1 file with linear elements and numeric data blocks.
///
/// Binary files, parametric nodes, and unknown element types are rejected.
/// Node and element tags remain the original IDs; connectivity becomes indices
/// into [`Mesh::points`].
///
/// # Errors
///
/// Returns an error for malformed sections, declared counts exceeding input,
/// unsupported element types, or incomplete numeric fields.
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
#[allow(clippy::too_many_lines)] // MSH section counts and records are validated in one pass.
fn read_41(source: &str) -> Result<(Dataset, usize)> {
    // Normalize line endings before looking for exact section delimiters.
    let normalized = source.replace("\r\n", "\n");
    let source = normalized.as_str();
    let header = section(source, "MeshFormat")?.ok_or_else(|| err("missing MeshFormat"))?;
    let mut h = header.split_whitespace();
    if h.next() != Some("4.1") || h.next() != Some("0") || h.next() != Some("8") {
        return Err(err("only ASCII MSH 4.1 with 8-byte data size is supported"));
    }
    let names = physical_names(source)?;
    let entity_physical = entity_physical_tags(source)?;
    let mut selected_cells: BTreeMap<(u8, u64), Vec<u64>> = BTreeMap::new();
    let mut selected_nodes: BTreeMap<u64, Vec<u64>> = BTreeMap::new();

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
        let entity_dim: u8 = number(t.next(), "entity dimension")?;
        let entity_tag: u64 = number(t.next(), "entity tag")?;
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
            if entity_dim == 0 {
                if let Some(tags) = entity_physical.get(&(0, entity_tag)) {
                    for &tag in tags {
                        selected_nodes.entry(tag).or_default().push(id);
                    }
                }
            }
        }
    }
    if mesh.points.len() != total {
        return Err(err("node count mismatch"));
    }

    // Element blocks are homogeneous in type and entity dimension.
    if let Some(elements) = section(source, "Elements")? {
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
            let entity_tag: u64 = number(t.next(), "entity tag")?;
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
                if let Some(tags) = entity_physical.get(&(
                    u8::try_from(dim).map_err(|_| err("invalid entity dimension"))?,
                    entity_tag,
                )) {
                    for &tag in tags {
                        selected_cells
                            .entry((
                                u8::try_from(dim).map_err(|_| err("invalid entity dimension"))?,
                                tag,
                            ))
                            .or_default()
                            .push(id);
                    }
                }
            }
        }
        if mesh.cells.len() != total {
            return Err(err("element count mismatch"));
        }
    }
    let generated_group_names = attach_cell_sets(&mut mesh, &names, selected_cells)
        + attach_node_sets(&mut mesh, &names, selected_nodes);
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
    Ok((dataset, generated_group_names))
}

/// Parse MSH 2.2 flat sections and reuse the common data-block decoder.
#[allow(clippy::too_many_lines)] // Flat node/element records each require bounded count checks.
fn read_22(source: &str) -> Result<Projection> {
    let nodes = section(source, "Nodes")?.ok_or_else(|| err("missing Nodes"))?;
    let mut tokens = nodes.split_whitespace();
    let count: usize = number(tokens.next(), "node count")?;
    if count > tokens.clone().count() / 4 {
        return Err(err("MSH 2.2 node count exceeds available input"));
    }
    let mut points = Vec::with_capacity(count);
    for _ in 0..count {
        let id: u64 = number(tokens.next(), "node tag")?;
        let coords: [f64; 3] = [
            number(tokens.next(), "x")?,
            number(tokens.next(), "y")?,
            number(tokens.next(), "z")?,
        ];
        points.push((id, coords));
    }
    if tokens.next().is_some() {
        return Err(err("extra MSH 2.2 node tokens"));
    }

    let elements = section(source, "Elements")?.ok_or_else(|| err("missing Elements"))?;
    let mut tokens = elements.split_whitespace();
    let count: usize = number(tokens.next(), "element count")?;
    if count > tokens.clone().count() / 4 {
        return Err(err("MSH 2.2 element count exceeds available input"));
    }
    let mut parsed = Vec::with_capacity(count);
    let mut tagged_elements = 0usize;
    for _ in 0..count {
        let id = number(tokens.next(), "element tag")?;
        let code: u32 = number(tokens.next(), "element type")?;
        let cell_kind = kind(code)?;
        let tag_count: usize = number(tokens.next(), "element tag count")?;
        if tag_count > source.len() / 2 {
            return Err(err("element tags exceed available input"));
        }
        let mut meaningful_tag = false;
        let mut physical = None;
        for index in 0..tag_count {
            let tag: i64 = number(tokens.next(), "element metadata tag")?;
            if index == 0 {
                if tag < 0 {
                    return Err(err("physical tag must be nonnegative"));
                }
                if tag > 0 {
                    physical = Some(u64::try_from(tag).map_err(|_| err("invalid physical tag"))?);
                }
            } else {
                meaningful_tag |= tag != 0;
            }
        }
        if meaningful_tag {
            tagged_elements += 1;
        }
        let mut nodes = Vec::with_capacity(cell_kind.node_count());
        for _ in 0..cell_kind.node_count() {
            nodes.push(number(tokens.next(), "element node")?);
        }
        parsed.push(Element22 {
            id,
            code,
            nodes,
            physical,
        });
    }
    if tokens.next().is_some() {
        return Err(err("extra MSH 2.2 element tokens"));
    }
    let names = physical_names(source)?;
    let mut selected_cells: BTreeMap<(u8, u64), Vec<u64>> = BTreeMap::new();
    for element in &parsed {
        if let Some(tag) = element.physical {
            let dimension = kind(element.code)?.dimension();
            selected_cells
                .entry((dimension, tag))
                .or_default()
                .push(element.id);
        }
    }

    // The 4.1 parser already owns node-ID resolution and complete data-block
    // validation. Translate only the flat geometry grammar, preserving tags.
    let mut text = String::from("$MeshFormat\n4.1 0 8\n$EndMeshFormat\n$Nodes\n");
    let min = points.iter().map(|(id, _)| *id).min().unwrap_or(0);
    let max = points.iter().map(|(id, _)| *id).max().unwrap_or(0);
    if points.is_empty() {
        text.push_str("0 0 0 0\n");
    } else {
        writeln!(
            text,
            "1 {} {min} {max}\n3 1 0 {}",
            points.len(),
            points.len()
        )
        .expect("String write");
        for (id, _) in &points {
            writeln!(text, "{id}").expect("String write");
        }
        for (_, xyz) in &points {
            writeln!(text, "{} {} {}", xyz[0], xyz[1], xyz[2]).expect("String write");
        }
    }
    text.push_str("$EndNodes\n$Elements\n");
    let mut groups: Vec<Vec<Element22>> = Vec::new();
    for element in parsed {
        if groups
            .last()
            .is_none_or(|group| group[0].code != element.code)
        {
            groups.push(Vec::new());
        }
        groups.last_mut().expect("group just created").push(element);
    }
    let element_count: usize = groups.iter().map(Vec::len).sum();
    let min = groups
        .iter()
        .flat_map(|group| group.iter().map(|item| item.id))
        .min()
        .unwrap_or(0);
    let max = groups
        .iter()
        .flat_map(|group| group.iter().map(|item| item.id))
        .max()
        .unwrap_or(0);
    writeln!(text, "{} {element_count} {min} {max}", groups.len()).expect("String write");
    for group in groups {
        let cell_kind = kind(group[0].code)?;
        writeln!(
            text,
            "{} 1 {} {}",
            code(cell_kind).1,
            group[0].code,
            group.len()
        )
        .expect("String write");
        for element in group {
            write!(text, "{}", element.id).expect("String write");
            for node in element.nodes {
                write!(text, " {node}").expect("String write");
            }
            text.push('\n');
        }
    }
    text.push_str("$EndElements\n");
    for name in ["NodeData", "ElementData"] {
        for body in data_sections(source, name)? {
            writeln!(text, "${name}\n{}\n$End{name}", body.trim()).expect("String write");
        }
    }
    let (mut dataset, _) = read_41(&text)?;
    let generated_group_names = attach_cell_sets(&mut dataset.mesh, &names, selected_cells);
    dataset.validate()?;
    Ok(Projection {
        dataset,
        tagged_elements,
        generated_group_names,
    })
}

/// Read ASCII Gmsh MSH 4.1 or 2.2 and report unrepresented 2.2 tags.
///
/// # Errors
///
/// Returns an error for malformed, binary, unsupported, or incomplete data.
pub fn read_projection(source: &str) -> Result<Projection> {
    let normalized = source.replace("\r\n", "\n");
    let header = section(&normalized, "MeshFormat")?.ok_or_else(|| err("missing MeshFormat"))?;
    let mut tokens = header.split_whitespace();
    let version = tokens.next().ok_or_else(|| err("missing MSH version"))?;
    if tokens.next() != Some("0") || tokens.next() != Some("8") || tokens.next().is_some() {
        return Err(err("only ASCII MSH with 8-byte data size is supported"));
    }
    match version {
        "4.1" => {
            let (dataset, generated_group_names) = read_41(&normalized)?;
            Ok(Projection {
                dataset,
                tagged_elements: 0,
                generated_group_names,
            })
        }
        "2.2" => read_22(&normalized),
        _ => Err(err(format!("unsupported MSH version {version}"))),
    }
}

/// Read ASCII Gmsh MSH 4.1 or 2.2 into the common dataset.
///
/// # Errors
///
/// Returns an error for malformed, binary, unsupported, or incomplete data.
pub fn read(source: &str) -> Result<Dataset> {
    Ok(read_projection(source)?.dataset)
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
                ..Mesh::default()
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
        let mut bytes_22 = Vec::new();
        write_22(&dataset, &mut bytes_22).unwrap();
        assert_eq!(
            read(std::str::from_utf8(&bytes_22).unwrap()).unwrap(),
            dataset
        );
    }

    #[test]
    fn msh22_tags_are_reported_and_counts_are_bounded() {
        let source = "$MeshFormat\n2.2 0 8\n$EndMeshFormat\n$Nodes\n2\n10 0 0 0\n20 1 0 0\n$EndNodes\n$Elements\n1\n30 1 2 7 8 10 20\n$EndElements\n";
        let projection = read_projection(source).unwrap();
        assert_eq!(projection.tagged_elements, 1);
        assert_eq!(projection.generated_group_names, 1);
        assert_eq!(projection.dataset.mesh.cells[0].id, 30);
        assert_eq!(projection.dataset.mesh.cells[0].connectivity, [0, 1]);
        assert_eq!(projection.dataset.mesh.cell_sets[0].name, "physical_1_7");
        assert!(read(&source.replace("$Nodes\n2", "$Nodes\n999999999")).is_err());
        assert!(read(&source.replace("30 1 2", "30 1 999999999")).is_err());
    }

    #[test]
    fn msh41_physical_names_select_overlapping_boundary_cells() {
        let source = include_str!("../../../tests/fixtures/named-boundary.msh");
        let dataset = read(source).unwrap();
        assert_eq!(dataset.mesh.cell_sets.len(), 3);
        assert_eq!(dataset.mesh.cell_sets[0].name, "inlet");
        assert_eq!(dataset.mesh.cell_sets[0].cell_ids, [1]);
        assert_eq!(dataset.mesh.cell_sets[1].name, "outer wall");
        assert_eq!(dataset.mesh.cell_sets[1].cell_ids, [1]);
        assert_eq!(write(&dataset, Vec::new()).unwrap_err().code, "E_MSH");
        let mut boundary_only = dataset.clone();
        boundary_only
            .mesh
            .cell_sets
            .retain(|set| set.dimension == 1);
        let mut output = Vec::new();
        crate::su2::write_data(&boundary_only, &mut output).unwrap();
        assert!(
            std::str::from_utf8(&output)
                .unwrap()
                .contains("MARKER_TAG= outer wall")
        );
    }

    #[test]
    fn msh41_physical_point_name_becomes_node_set() {
        let source = "$MeshFormat\n4.1 0 8\n$EndMeshFormat\n$PhysicalNames\n1\n0 42 \"fixed\"\n$EndPhysicalNames\n$Entities\n1 0 0 0\n1 0 0 0 1 42\n$EndEntities\n$Nodes\n1 1 10 10\n0 1 0 1\n10\n0 0 0\n$EndNodes\n$Elements\n0 0 0 0\n$EndElements\n";
        let projection = read_projection(source).unwrap();
        assert_eq!(projection.generated_group_names, 0);
        assert_eq!(projection.dataset.mesh.node_sets[0].name, "fixed");
        assert_eq!(projection.dataset.mesh.node_sets[0].point_ids, [10]);
    }

    #[test]
    fn gmsh_written_msh22_fixture_preserves_all_topologies() {
        let projection =
            read_projection(include_str!("../../../tests/fixtures/gmsh-2.2-mixed.msh")).unwrap();
        assert_eq!(projection.tagged_elements, 0);
        let dataset = projection.dataset;
        assert_eq!(dataset.mesh.points.len(), 9);
        assert_eq!(dataset.mesh.cells.len(), 7);
        assert_eq!(dataset.mesh.cells[0].id, 1);
        assert_eq!(dataset.mesh.cells[6].kind, CellKind::Pyramid5);
    }

    #[test]
    fn nodes_only_meshes_preserve_ids_in_both_dialects() {
        let dataset = Dataset {
            mesh: Mesh {
                points: vec![
                    Point {
                        id: 10,
                        position: [0., 0., 0.],
                    },
                    Point {
                        id: 20,
                        position: [1., 2., 3.],
                    },
                ],
                cells: vec![],
                ..Mesh::default()
            },
            fields: vec![],
        };
        for version in [Version::V4_1, Version::V2_2] {
            let mut output = Vec::new();
            write_version(&dataset, version, &mut output).unwrap();
            let decoded = read(std::str::from_utf8(&output).unwrap()).unwrap();
            assert_eq!(decoded.mesh.points, dataset.mesh.points);
            assert_eq!(decoded.mesh.cells, []);
        }
    }

    #[test]
    fn malformed_field_blocks_cannot_create_partial_results() {
        let base = "$MeshFormat\n2.2 0 8\n$EndMeshFormat\n$Nodes\n2\n10 0 0 0\n20 1 0 0\n$EndNodes\n$Elements\n1\n30 1 0 10 20\n$EndElements\n$NodeData\n1\n\"temperature\"\n1\n0.5\n3\n2\n1\n2\n10 1\n20 2\n$EndNodeData\n";
        let valid = read(base).unwrap();
        assert_eq!(valid.fields[0].values, [1., 2.]);
        for (old, new) in [
            ("$NodeData\n1", "$NodeData\n2"),
            ("\"temperature\"", "temperature"),
            ("\"temperature\"\n1\n0.5", "\"temperature\"\n2\n0.5"),
            ("0.5\n3\n2\n1\n2", "0.5\n2\n2\n1\n2"),
            ("0.5\n3\n2\n1\n2", "0.5\n3\n2\n0\n2"),
            ("0.5\n3\n2\n1\n2", "0.5\n3\n2\n1\n1"),
            ("20 2\n$EndNodeData", "10 2\n$EndNodeData"),
            ("20 2\n$EndNodeData", "99 2\n$EndNodeData"),
            ("$EndNodeData", ""),
        ] {
            assert!(base.contains(old));
            assert!(read(&base.replacen(old, new, 1)).is_err(), "{new}");
        }
    }

    #[test]
    fn msh22_rejects_extra_tokens_and_unrepresentable_ids() {
        let base = "$MeshFormat\n2.2 0 8\n$EndMeshFormat\n$Nodes\n2\n10 0 0 0\n20 1 0 0\n$EndNodes\n$Elements\n1\n30 1 0 10 20\n$EndElements\n";
        for (old, new) in [
            ("20 1 0 0\n$EndNodes", "20 1 0 0 99\n$EndNodes"),
            (
                "30 1 0 10 20\n$EndElements",
                "30 1 0 10 20 99\n$EndElements",
            ),
            ("30 1 0 10 20", "30 1 0 10 99"),
            ("2.2 0 8", "2.2 1 8"),
        ] {
            assert!(read(&base.replacen(old, new, 1)).is_err(), "{new}");
        }
        let mut dataset = read(base).unwrap();
        dataset.mesh.points[0].id = i32::MAX as u64 + 1;
        let mut output = Vec::new();
        assert_eq!(write_22(&dataset, &mut output).unwrap_err().code, "E_MSH");
        assert_eq!(output, []);
    }
}
