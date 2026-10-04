//! Read supported VTU datasets.

use crate::core::{Cell, CellKind, Dataset, Error, Field, FieldLocation, Mesh, Point, Result};
use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader;
use std::collections::{BTreeMap, BTreeSet};

use super::write::valid_xml_text;

/// Parsed VTU data and whether original identity arrays were absent.
#[derive(Debug, Clone, PartialEq)]
pub struct Projection {
    /// Validated mesh and numeric fields.
    pub dataset: Dataset,

    /// True when point IDs were assigned in array order.
    pub generated_point_ids: bool,

    /// True when cell IDs were assigned in array order.
    pub generated_cell_ids: bool,
}

/// Tokenized XML element with decoded attributes and direct text content.
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

fn xml_start(start: &BytesStart<'_>) -> Result<XmlNode> {
    let name = start.name().as_ref().to_owned();
    let mut attributes = BTreeMap::new();
    for attribute in start.attributes() {
        let attribute = attribute.map_err(|e| Error::new("E_VTU", e.to_string()))?;
        let key = attribute.key.as_ref().to_owned();
        let value = attribute
            .normalized_value(XmlVersion::Implicit1_0)
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
                if stack
                    .last()
                    .is_some_and(|parent| parent.name == "DataArray")
                {
                    return Err(Error::new(
                        "E_VTU",
                        "nested DataArray content is unsupported",
                    ));
                }
                stack.push(xml_start(&start)?);
            }
            Event::Empty(start) => {
                if stack
                    .last()
                    .is_some_and(|parent| parent.name == "DataArray")
                {
                    return Err(Error::new(
                        "E_VTU",
                        "nested DataArray content is unsupported",
                    ));
                }
                let node = xml_start(&start)?;
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
                if node.name != end.name().as_ref() {
                    return Err(Error::new("E_VTU", "mismatched XML close"));
                }
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(node);
                } else if root.replace(node).is_some() {
                    return Err(Error::new("E_VTU", "multiple XML roots"));
                }
            }
            Event::Text(value) => {
                let text = value.xml10_content();
                if let Some(node) = stack.last_mut() {
                    if node.name != "DataArray" && !text.trim().is_empty() {
                        return Err(Error::new("E_VTU", "text outside DataArray"));
                    }
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
#[allow(clippy::too_many_lines)] // Parsed XML sections are validated against one piece together.
pub fn read_projection(source: &str) -> Result<Projection> {
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
    let point_id_array = pd
        .map(|node| node.children("DataArray"))
        .transpose()?
        .unwrap_or_default()
        .into_iter()
        .find(|node| node.attr("Name") == Some("nastran_node_id"));
    let generated_point_ids = point_id_array.is_none();
    let point_ids: Vec<u64> = match point_id_array {
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
    let cell_id_array = cd
        .map(|node| node.children("DataArray"))
        .transpose()?
        .unwrap_or_default()
        .into_iter()
        .find(|node| node.attr("Name") == Some("nastran_element_id"));
    let generated_cell_ids = cell_id_array.is_none();
    let element_ids: Vec<u64> = match cell_id_array {
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
        mesh: Mesh {
            points,
            cells,
            ..Mesh::default()
        },
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
                        .map_or_else(|| format!("C{}", i + 1), str::to_owned)
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
    Ok(Projection {
        dataset,
        generated_point_ids,
        generated_cell_ids,
    })
}

/// Read one ASCII `UnstructuredGrid` piece into a mesh and numeric fields.
///
/// Use [`read_projection`] when assigned point or cell IDs must be reported.
///
/// # Errors
///
/// Returns an error for unsupported layouts, malformed arrays, invalid counts,
/// or invalid reconstructed data.
pub fn read(source: &str) -> Result<Dataset> {
    Ok(read_projection(source)?.dataset)
}
