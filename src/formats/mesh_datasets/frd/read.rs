//! Read supported FRD geometry and nodal result records.
use crate::core::{Cell, CellKind, Dataset, Error, Field, FieldLocation, Mesh, Point, Result};
use std::collections::BTreeMap;

/// Construct an FRD-specific diagnostic for a rejected record or value.
pub(super) fn err(message: impl Into<String>) -> Error {
    Error::new("E_FRD", message)
}

/// Parse an ASCII number, accepting Fortran `D` exponents from solver output.
fn n<T: std::str::FromStr>(text: &str, what: &str) -> Result<T> {
    text.trim()
        .replace('D', "E")
        .parse()
        .map_err(|_| err(format!("invalid {what}")))
}

/// Decode a supported FRD element type; higher-order types are rejected.
fn kind(code: u32) -> Result<CellKind> {
    match code {
        1 => Ok(CellKind::Hex8),
        2 => Ok(CellKind::Wedge6),
        3 => Ok(CellKind::Tet4),
        7 => Ok(CellKind::Triangle3),
        9 => Ok(CellKind::Quad4),
        11 => Ok(CellKind::Line2),
        _ => Err(err(format!("unsupported FRD element type {code}"))),
    }
}

/// Decode one result row's entity ID and expected numeric components.
/// Accepts whitespace records and fixed-width short or long records.
fn values(line: &str, long: bool, expected: usize) -> Result<(u64, Vec<f64>)> {
    // Prefer whitespace-separated records when all columns are distinct.
    let tokens: Vec<&str> = line.split_whitespace().collect();
    if tokens.len() >= expected + 2 {
        let id = n(tokens[1], "entity ID")?;
        let vals = tokens[2..2 + expected]
            .iter()
            .map(|v| n(v, "value"))
            .collect::<Result<Vec<_>>>()?;
        return Ok((id, vals));
    }

    // Long IDs can touch adjacent columns, requiring fixed-width slicing.
    let width = if long { 10 } else { 5 };
    let id = n(
        line.get(3..3 + width).ok_or_else(|| err("truncated ID"))?,
        "entity ID",
    )?;
    let mut vals = Vec::new();
    for i in 0..expected {
        let start = 3 + width + i * 12;
        vals.push(n(
            line.get(start..start + 12)
                .ok_or_else(|| err("truncated fixed-width value"))?,
            "value",
        )?);
    }
    Ok((id, vals))
}

/// Decode the numeric payload of an FRD `-2` result continuation.
/// The caller chooses the remaining component count for this entity.
fn continuation_values(line: &str, long: bool, expected: usize) -> Result<Vec<f64>> {
    // A full-width row can be sliced without depending on whitespace.
    let width = if long { 10 } else { 5 };
    if line.len() >= 3 + width + expected * 12 {
        let mut values = Vec::with_capacity(expected);
        for i in 0..expected {
            let start = 3 + width + i * 12;
            values.push(n(&line[start..start + 12], "continuation value")?);
        }
        return Ok(values);
    }

    // Shorter rows are accepted only in the explicit -2 token form.
    let tokens: Vec<&str> = line.split_whitespace().collect();
    if tokens.len() != expected + 1 || tokens.first() != Some(&"-2") {
        return Err(err("invalid result continuation"));
    }
    tokens[1..]
        .iter()
        .map(|token| n(token, "continuation value"))
        .collect()
}

/// Decode element node IDs from a whitespace or fixed-width connectivity row.
/// Blank fixed-width slots are skipped; malformed IDs fail.
fn integers(line: &str, long: bool) -> Result<Vec<u64>> {
    // Element connectivity can appear as spaced tags or packed columns.
    let tokens: Vec<&str> = line.split_whitespace().collect();
    if tokens.len() > 2 {
        return tokens[1..].iter().map(|v| n(v, "node ID")).collect();
    }
    let width = if long { 10 } else { 5 };
    line.get(3..)
        .unwrap_or("")
        .as_bytes()
        .chunks(width)
        .filter(|c| !c.iter().all(u8::is_ascii_whitespace))
        .map(|c| n(std::str::from_utf8(c).unwrap_or(""), "node ID"))
        .collect()
}

/// Decode an element record's ID and its type/count field.
/// The named second value is included in parse diagnostics.
fn header_pair(line: &str, long: bool, second: &str) -> Result<(u64, u64)> {
    // The fallback preserves IDs when fixed-width columns run together.
    let tokens: Vec<&str> = line.split_whitespace().collect();
    if tokens.len() >= 3 && tokens[0] == "-1" {
        return Ok((n(tokens[1], "entity ID")?, n(tokens[2], second)?));
    }
    let width = if long { 10 } else { 5 };
    let id = n(
        line.get(3..3 + width)
            .ok_or_else(|| err("truncated entity ID"))?,
        "entity ID",
    )?;
    let value = n(
        line.get(3 + width..3 + width + 5)
            .ok_or_else(|| err("truncated header value"))?,
        second,
    )?;
    Ok((id, value))
}

/// Inspect the FRD header's format column: false is short, true is long.
/// Binary mode flags are rejected even when their record body is not read.
fn header_format(line: &str, column: usize) -> Result<bool> {
    // Header format flags are column-specific for geometry and result blocks.
    if line.len() < column {
        return Ok(false);
    }
    match line.as_bytes().get(column - 1).copied().unwrap_or(b'0') {
        b'1' => Ok(true),
        b'2' | b'3' => Err(err("binary FRD block is unsupported")),
        _ => Ok(false),
    }
}

/// FRD block currently being decoded; bool records the long-format flag.
#[derive(Clone, Copy)]
enum Mode {
    /// No supported FRD block is active.
    None,

    /// Node records; the flag selects long-format IDs.
    Nodes(bool),

    /// Element records; the flag selects long-format IDs.
    Elements(bool),

    /// Result records; the flag selects long-format IDs.
    Results(bool),
}

/// Partial nodal field assembled across FRD result and continuation records.
#[derive(Default)]
struct FieldBuilder {
    name: String,
    components: Vec<String>,
    count: usize,
    material_dependent: bool,
    step: Option<i64>,
    time: Option<f64>,
    data: BTreeMap<u64, Vec<f64>>,
    pending: Option<u64>,
}

/// Read supported ASCII short- or long-format FRD mesh and result records.
///
/// Higher-order elements, binary blocks, and material-dependent nodal records
/// fail explicitly. Use [`crate::formats::frd::write`] for an example roundtrip. The returned
/// [`Dataset`] contains only the documented supported subset.
///
/// # Errors
///
/// Returns an error for malformed or unsupported FRD records, invalid numeric
/// values, or incomplete mesh and field references.
#[allow(clippy::too_many_lines)] // FRD record state is resolved in one ordered pass.
pub fn read(source: &[u8]) -> Result<Dataset> {
    // Parse block records first, then resolve element and result node IDs
    // against the completed point list.
    let text = std::str::from_utf8(source).map_err(|_| err("non-UTF-8 or binary FRD block"))?;
    let mut mode = Mode::None;
    let mut points = Vec::new();
    let mut elements = Vec::<(u64, CellKind, Vec<u64>)>::new();
    let mut current: Option<(u64, CellKind, Vec<u64>)> = None;
    let mut fields = Vec::<FieldBuilder>::new();
    let mut field: Option<FieldBuilder> = None;
    for (line_no, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("2C") || trimmed.starts_with("3C") || trimmed.starts_with("100C") {
            // A new block closes the previous result builder and selects
            // short or long record widths from its header.
            if let Some(f) = field.take() {
                fields.push(f);
            }
            mode = if trimmed.starts_with("2C") {
                Mode::Nodes(header_format(line, 72)?)
            } else if trimmed.starts_with("3C") {
                Mode::Elements(header_format(line, 72)?)
            } else {
                let mut b = FieldBuilder::default();
                if line.len() >= 24 {
                    b.time = line.get(12..24).and_then(|s| s.trim().parse().ok());
                }
                if line.len() >= 63 {
                    b.step = line.get(58..63).and_then(|s| s.trim().parse().ok());
                }
                field = Some(b);
                Mode::Results(header_format(line, 75)?)
            };
            continue;
        }
        if trimmed.starts_with("9999") {
            break;
        }
        if trimmed.starts_with("-3") {
            // -3 terminates the active element or result block.
            if let Mode::Elements(_) = mode {
                if let Some(e) = current.take() {
                    elements.push(e);
                }
            }
            if let Mode::Results(_) = mode {
                if let Some(f) = field.take() {
                    fields.push(f);
                }
            }
            mode = Mode::None;
            continue;
        }
        match mode {
            Mode::Nodes(long) if trimmed.starts_with("-1") => {
                let (id, coords) = values(line, long, 3).map_err(|e| e.at(line_no + 1))?;
                points.push(Point {
                    id,
                    position: [coords[0], coords[1], coords[2]],
                });
            }
            Mode::Elements(long) if trimmed.starts_with("-1") => {
                if let Some(e) = current.take() {
                    elements.push(e);
                }
                let (id, element_type) =
                    header_pair(line, long, "element type").map_err(|e| e.at(line_no + 1))?;
                let type_code =
                    u32::try_from(element_type).map_err(|_| err("invalid element type"))?;
                current = Some((id, kind(type_code)?, Vec::new()));
            }
            Mode::Elements(long) if trimmed.starts_with("-2") => {
                current
                    .as_mut()
                    .ok_or_else(|| err("element nodes without header").at(line_no + 1))?
                    .2
                    .extend(integers(line, long).map_err(|e| e.at(line_no + 1))?);
            }
            Mode::Results(_) if trimmed.starts_with("-4") => {
                let t: Vec<&str> = trimmed.split_whitespace().collect();
                if t.len() < 4 {
                    return Err(err("truncated result header").at(line_no + 1));
                }
                let f = field
                    .as_mut()
                    .ok_or_else(|| err("result header without block"))?;
                f.name = t[1].to_string();
                f.count = n(t[2], "component count")?;
                f.material_dependent = match t.get(3).copied() {
                    Some("1") => false,
                    Some("2") => true,
                    _ => return Err(err("unsupported result location/type").at(line_no + 1)),
                };
            }
            Mode::Results(_) if trimmed.starts_with("-5") => {
                let t: Vec<&str> = trimmed.split_whitespace().collect();
                if t.len() < 2 {
                    return Err(err("truncated component header").at(line_no + 1));
                }
                field
                    .as_mut()
                    .ok_or_else(|| err("component without block"))?
                    .components
                    .push(t[1].to_string());
            }
            Mode::Results(long) if trimmed.starts_with("-1") => {
                let f = field.as_mut().ok_or_else(|| err("result without header"))?;

                // Material-dependent rows use a count header; only the
                // single-material case can map to one value tuple per node.
                if f.material_dependent {
                    let (id, materials) =
                        header_pair(line, long, "material count").map_err(|e| e.at(line_no + 1))?;
                    if materials != 1 {
                        return Err(err(
                            "material-dependent result requires exactly one material per node",
                        )
                        .at(line_no + 1));
                    }
                    if f.data.insert(id, Vec::new()).is_some() {
                        return Err(err("duplicate result node").at(line_no + 1));
                    }
                    f.pending = Some(id);
                    continue;
                }
                let available = f.count.min(6);
                let (id, vals) = values(line, long, available).map_err(|e| e.at(line_no + 1))?;
                if f.data.insert(id, vals).is_some() {
                    return Err(err("duplicate result node").at(line_no + 1));
                }
                f.pending = Some(id);
            }
            Mode::Results(long) if trimmed.starts_with("-2") => {
                // Continue the most recent node's tuple in groups of six.
                let f = field
                    .as_mut()
                    .ok_or_else(|| err("continuation without result"))?;
                let id = f.pending.ok_or_else(|| err("continuation without node"))?;
                let present = f.data[&id].len();
                let needed = (f.count - present).min(6);
                if needed == 0 {
                    return Err(err("extra result continuation").at(line_no + 1));
                }
                let vals = if f.material_dependent {
                    values(line, long, needed).map(|(_, values)| values)
                } else {
                    continuation_values(line, long, needed)
                }
                .map_err(|e| e.at(line_no + 1))?;
                f.data
                    .get_mut(&id)
                    .ok_or_else(|| err("continuation references unknown node"))?
                    .extend(vals);
            }
            Mode::None => {}
            _ => return Err(err("unexpected FRD record").at(line_no + 1)),
        }
    }
    if let Some(e) = current.take() {
        elements.push(e);
    }
    if let Some(f) = field.take() {
        fields.push(f);
    }
    if points.is_empty() || elements.is_empty() {
        return Err(err("FRD requires node and element blocks"));
    }

    // Convert original node tags to internal point indices for each cell.
    let index: BTreeMap<u64, usize> = points.iter().enumerate().map(|(i, p)| (p.id, i)).collect();
    let mut cells = Vec::new();
    for (id, kind, nodes) in elements {
        if nodes.len() != kind.node_count() {
            return Err(err(format!("element {id} has wrong node count")));
        }
        let connectivity = nodes
            .iter()
            .map(|v| {
                index
                    .get(v)
                    .copied()
                    .ok_or_else(|| err(format!("unknown node {v}")))
            })
            .collect::<Result<Vec<_>>>()?;
        cells.push(Cell {
            id,
            kind,
            connectivity,
            property_id: None,
        });
    }
    let mut dataset = Dataset {
        mesh: Mesh {
            points,
            cells,
            ..Mesh::default()
        },
        fields: Vec::new(),
    };

    // FRD result rows may be in any node order; emit entity-major field
    // values in the reconstructed mesh's point order.
    for f in fields {
        if f.name.is_empty() || f.count == 0 || f.components.len() != f.count {
            return Err(err("incomplete result field metadata"));
        }
        if f.data.len() != dataset.mesh.points.len() {
            return Err(err(format!(
                "field {} lacks values for some mesh nodes",
                f.name
            )));
        }
        let mut values = Vec::new();
        for p in &dataset.mesh.points {
            let row = f
                .data
                .get(&p.id)
                .ok_or_else(|| err(format!("field {} has no node {}", f.name, p.id)))?;
            if row.len() != f.count {
                return Err(err("incomplete result row"));
            }
            values.extend(row);
        }
        dataset.fields.push(Field {
            name: f.name,
            location: FieldLocation::Point,
            components: f.components,
            values,
            step: f.step,
            time: f.time,
        });
    }
    dataset.validate()?;
    Ok(dataset)
}
