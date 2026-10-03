//! ASCII CalculiX FRD geometry and supported nodal result records.
use caexfer_core::{Cell, CellKind, Dataset, Error, Field, FieldLocation, Mesh, Point, Result};
use std::collections::BTreeMap;
fn err(message: impl Into<String>) -> Error {
    Error::new("E_FRD", message)
}
fn n<T: std::str::FromStr>(text: &str, what: &str) -> Result<T> {
    text.trim()
        .replace('D', "E")
        .parse()
        .map_err(|_| err(format!("invalid {what}")))
}
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
fn values(line: &str, long: bool, expected: usize) -> Result<(u64, Vec<f64>)> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    if tokens.len() >= expected + 2 {
        let id = n(tokens[1], "entity ID")?;
        let vals = tokens[2..2 + expected]
            .iter()
            .map(|v| n(v, "value"))
            .collect::<Result<Vec<_>>>()?;
        return Ok((id, vals));
    }
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
fn integers(line: &str, long: bool) -> Result<Vec<u64>> {
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
fn header_pair(line: &str, long: bool, second: &str) -> Result<(u64, u64)> {
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
fn header_format(line: &str, column: usize) -> Result<bool> {
    if line.len() < column {
        return Ok(false);
    }
    match line.as_bytes().get(column - 1).copied().unwrap_or(b'0') {
        b'0' => Ok(false),
        b'1' => Ok(true),
        b'2' | b'3' => Err(err("binary FRD block is unsupported")),
        _ => Ok(false),
    }
}
#[derive(Clone, Copy)]
enum Mode {
    None,
    Nodes(bool),
    Elements(bool),
    Results(bool),
}
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

/// Read the documented ASCII short/long FRD records. Higher-order elements,
/// binary blocks and material-dependent nodal records fail explicitly.
pub fn read(source: &[u8]) -> Result<Dataset> {
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
                f.material_dependent = match t[3] {
                    "1" => false,
                    "2" => true,
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
                let f = field
                    .as_mut()
                    .ok_or_else(|| err("continuation without result"))?;
                let id = f.pending.ok_or_else(|| err("continuation without node"))?;
                let present = f.data[&id].len();
                let needed = (f.count - present).min(6);
                if needed == 0 {
                    return Err(err("extra result continuation").at(line_no + 1));
                }
                let (_, vals) = values(line, long, needed).map_err(|e| e.at(line_no + 1))?;
                f.data.get_mut(&id).unwrap().extend(vals);
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
        mesh: Mesh { points, cells },
        fields: Vec::new(),
    };
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_fixed_width_ids_do_not_merge_with_record_keys() {
        let nodes = [1_234_567_890, 1_234_567_891, 1_234_567_892];
        let mut source = format!(" 2C{:68}1\n", "");
        for (i, id) in nodes.iter().enumerate() {
            source.push_str(&format!(
                " -1{id:>10}{:>12.5E}{:>12.5E}{:>12.5E}\n",
                i as f64, 0., 0.
            ));
        }
        source.push_str(&format!(" -3\n 3C{:68}1\n", ""));
        source.push_str(&format!(
            " -1{:>10}{:>5}{:>5}{:>5}\n",
            1_234_567_899u64, 7, 0, 0
        ));
        source.push_str(&format!(
            " -2{:>10}{:>10}{:>10}\n -3\n 9999\n",
            nodes[0], nodes[1], nodes[2]
        ));
        let dataset = read(source.as_bytes()).unwrap();
        assert_eq!(
            dataset.mesh.points.iter().map(|p| p.id).collect::<Vec<_>>(),
            nodes
        );
        assert_eq!(dataset.mesh.cells[0].connectivity, vec![0, 1, 2]);
    }
}
