//! ASCII `CalculiX` FRD geometry and supported nodal result records.
use crate::core::{Cell, CellKind, Dataset, Error, Field, FieldLocation, Mesh, Point, Result};
use std::collections::BTreeMap;
use std::io::Write;
/// Construct an FRD-specific diagnostic for a rejected record or value.
fn err(message: impl Into<String>) -> Error {
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
/// Encode linear topology, including FRD's unsupported pyramid case.
fn type_code(kind: CellKind) -> Result<u32> {
    match kind {
        CellKind::Hex8 => Ok(1),
        CellKind::Wedge6 => Ok(2),
        CellKind::Tet4 => Ok(3),
        CellKind::Triangle3 => Ok(7),
        CellKind::Quad4 => Ok(9),
        CellKind::Line2 => Ok(11),
        CellKind::Pyramid5 => Err(err("FRD has no supported five-node pyramid type")),
    }
}
/// Format a finite value in the fixed E12.5 field used by long FRD records.
/// Values outside the two-digit exponent range fail instead of overflowing.
fn ascii_number(value: f64) -> Result<String> {
    // FRD's 12-column ASCII number has room for only a signed two-digit exponent.
    let scientific = format!("{value:.5E}");
    let (mantissa, exponent) = scientific
        .split_once('E')
        .ok_or_else(|| err("cannot format FRD number"))?;
    let exponent: i32 = exponent.parse().map_err(|_| err("invalid FRD exponent"))?;
    if !(-99..=99).contains(&exponent) {
        return Err(err("number exceeds FRD ASCII E12.5 range"));
    }
    let result = format!("{mantissa}E{exponent:+03}");
    if result.len() > 12 {
        return Err(err("number exceeds FRD ASCII E12.5 width"));
    }
    Ok(format!("{result:>12}"))
}
/// Ensure an ID fits the ten-column long-format FRD field.
fn identifier(value: u64) -> Result<u64> {
    if value > 9_999_999_999 {
        return Err(err("FRD long-format IDs must fit ten digits"));
    }
    Ok(value)
}
/// Check the printable no-space label bounds imposed by FRD headers.
fn label(value: &str, limit: usize, what: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > limit
        || !value.bytes().all(|b| b.is_ascii_graphic() && b != b' ')
    {
        return Err(err(format!(
            "{what} must be 1–{limit} printable ASCII characters without spaces"
        )));
    }
    Ok(())
}

/// Write long-format ASCII FRD mesh and complete nodal fields.
///
/// Values are rounded to six significant digits by FRD's E12.5 records.
/// Five-node pyramids, cell fields, oversized IDs, and unsupported labels
/// fail rather than being dropped.
///
/// # Errors
///
/// Returns an error for unsupported or malformed data, or when the output
/// stream rejects bytes.
///
/// # Examples
///
/// ```
/// use caexfer::{core::Dataset, frd, inp};
/// let mesh = inp::read("*NODE\n1,0,0,0\n2,1,0,0\n*ELEMENT, TYPE=T3D2\n10,1,2\n")?.mesh;
/// let mut bytes = Vec::new();
/// frd::write(&Dataset { mesh, fields: vec![] }, &mut bytes)?;
/// let decoded = frd::read(&bytes)?;
/// assert_eq!(decoded.mesh.cells[0].id, 10);
/// # Ok::<(), caexfer::core::Error>(())
/// ```
#[allow(clippy::too_many_lines)] // Fixed-width records are emitted in format order.
pub fn write(dataset: &Dataset, mut output: impl Write) -> Result<()> {
    // Check the entire dataset against FRD's fixed-width constraints before
    // writing any bytes to the caller's stream.
    dataset.validate()?;
    if dataset.mesh.points.is_empty() || dataset.mesh.cells.is_empty() {
        return Err(err("FRD requires nodes and elements"));
    }
    for point in &dataset.mesh.points {
        identifier(point.id)?;
        for value in point.position {
            ascii_number(value)?;
        }
    }
    for cell in &dataset.mesh.cells {
        identifier(cell.id)?;
        type_code(cell.kind)?;
    }
    for field in &dataset.fields {
        if field.location != FieldLocation::Point {
            return Err(err("FRD writer accepts complete nodal fields only"));
        }
        label(&field.name, 8, "FRD field name")?;
        for component in &field.components {
            label(component, 8, "FRD component name")?;
        }
        if field.components.len() > 99_999 {
            return Err(err("FRD component count exceeds five-digit field"));
        }
        if field.step.is_some_and(|v| !(0..=99_999).contains(&v)) {
            return Err(err("FRD step must fit a nonnegative five-digit field"));
        }
        ascii_number(field.time.unwrap_or(0.0))?;
        for value in &field.values {
            ascii_number(*value)?;
        }
    }

    // The node and element blocks retain original IDs; connectivity is
    // translated from internal point indices back to node IDs.
    writeln!(output, "  1Ccaexfr")?;
    writeln!(
        output,
        "  2C{:18}{:>12}{:37}1",
        "",
        dataset.mesh.points.len(),
        ""
    )?;
    for point in &dataset.mesh.points {
        write!(output, " -1{:>10}", point.id)?;
        for value in point.position {
            write!(output, "{}", ascii_number(value)?)?;
        }
        writeln!(output)?;
    }
    writeln!(output, " -3")?;
    writeln!(
        output,
        "  3C{:18}{:>12}{:37}1",
        "",
        dataset.mesh.cells.len(),
        ""
    )?;
    for cell in &dataset.mesh.cells {
        writeln!(
            output,
            " -1{:>10}{:>5}{:>5}{:>5}",
            cell.id,
            type_code(cell.kind)?,
            0,
            0
        )?;
        write!(output, " -2")?;
        for &index in &cell.connectivity {
            write!(output, "{:>10}", dataset.mesh.points[index].id)?;
        }
        writeln!(output)?;
    }
    writeln!(output, " -3")?;

    // Each nodal field gets its own result header and component descriptors.
    for field in &dataset.fields {
        writeln!(
            output,
            "  100C{:<6}{}{:>12}{:<20}{:>2}{:>5}{:<10}{:>2}",
            "",
            ascii_number(field.time.unwrap_or(0.0))?,
            dataset.mesh.points.len(),
            "",
            i32::from(field.time.is_some()),
            field.step.unwrap_or(0),
            "",
            1
        )?;
        writeln!(
            output,
            " -4  {:<8}{:>5}{:>5}",
            field.name,
            field.components.len(),
            1
        )?;
        for (i, component) in field.components.iter().enumerate() {
            let kind = if field.components.len() == 3 { 2 } else { 1 };
            writeln!(
                output,
                " -5  {:<8}{:>5}{:>5}{:>5}{:>5}{:>5}{:8}",
                component,
                1,
                kind,
                i + 1,
                0,
                0,
                ""
            )?;
        }
        let components = field.components.len();

        // FRD carries at most six values per record; remaining components
        // continue on -2 records for the same node.
        for (i, point) in dataset.mesh.points.iter().enumerate() {
            for (chunk_index, chunk) in field.values[i * components..(i + 1) * components]
                .chunks(6)
                .enumerate()
            {
                if chunk_index == 0 {
                    write!(output, " -1{:>10}", point.id)?;
                } else {
                    write!(output, " -2{:>10}", "")?;
                }
                for value in chunk {
                    write!(output, "{}", ascii_number(*value)?)?;
                }
                writeln!(output)?;
            }
        }
        writeln!(output, " -3")?;
    }
    writeln!(output, " 9999")?;
    Ok(())
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
    None,
    Nodes(bool),
    Elements(bool),
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
/// fail explicitly. Use [`write()`] for an example roundtrip. The returned
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
        mesh: Mesh { points, cells },
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write as _;

    #[test]
    fn long_fixed_width_ids_do_not_merge_with_record_keys() {
        let nodes = [1_234_567_890, 1_234_567_891, 1_234_567_892];
        let mut source = format!(" 2C{:68}1\n", "");
        for (i, id) in nodes.iter().enumerate() {
            writeln!(
                source,
                " -1{id:>10}{:>12.5E}{:>12.5E}{:>12.5E}",
                [0.0, 1.0, 2.0][i],
                0.,
                0.
            )
            .unwrap();
        }
        write!(source, " -3\n 3C{:68}1\n", "").unwrap();
        writeln!(
            source,
            " -1{:>10}{:>5}{:>5}{:>5}",
            1_234_567_899u64, 7, 0, 0
        )
        .unwrap();
        write!(
            source,
            " -2{:>10}{:>10}{:>10}\n -3\n 9999\n",
            nodes[0], nodes[1], nodes[2]
        )
        .unwrap();
        let dataset = read(source.as_bytes()).unwrap();
        assert_eq!(
            dataset.mesh.points.iter().map(|p| p.id).collect::<Vec<_>>(),
            nodes
        );
        assert_eq!(dataset.mesh.cells[0].connectivity, vec![0, 1, 2]);
    }

    #[test]
    fn ascii_writer_preserves_mesh_and_nodal_values_with_continuation() {
        let mut dataset = read(include_bytes!("../../tests/fixtures/linear-results.frd")).unwrap();
        dataset.fields.push(Field {
            name: "EXTRA".into(),
            location: FieldLocation::Point,
            components: (1..=7).map(|index| format!("C{index}")).collect(),
            values: (0..21).map(f64::from).collect(),
            step: Some(2),
            time: Some(0.25),
        });
        let mut bytes = Vec::new();
        write(&dataset, &mut bytes).unwrap();
        let decoded = read(&bytes).unwrap();
        assert_eq!(decoded.mesh.points, dataset.mesh.points);
        assert_eq!(decoded.mesh.cells, dataset.mesh.cells);
        assert_eq!(decoded.fields[0].values, dataset.fields[0].values);
        assert_eq!(decoded.fields[2].values, dataset.fields[2].values);
        assert_eq!(decoded.fields[2].step, Some(2));
        assert_eq!(decoded.fields[2].time, Some(0.25));
    }

    #[test]
    fn ascii_writer_rejects_unsupported_pyramid() {
        assert_eq!(type_code(CellKind::Pyramid5).unwrap_err().code, "E_FRD");
    }

    #[test]
    fn all_supported_linear_topologies_roundtrip_in_one_file() {
        let mut mesh =
            crate::bdf::Document::parse(include_bytes!("../../tests/fixtures/mixed-linear.bdf"))
                .unwrap()
                .geometry()
                .unwrap()
                .mesh;
        mesh.cells.retain(|cell| cell.kind != CellKind::Pyramid5);
        let expected = mesh.cells.iter().map(|cell| cell.kind).collect::<Vec<_>>();
        let mut encoded = Vec::new();
        write(
            &Dataset {
                mesh,
                fields: vec![],
            },
            &mut encoded,
        )
        .unwrap();
        let decoded = read(&encoded).unwrap();
        assert_eq!(
            decoded
                .mesh
                .cells
                .iter()
                .map(|cell| cell.kind)
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(decoded.mesh.cells.len(), 6);
    }

    #[test]
    fn malformed_result_and_connectivity_records_fail_explicitly() {
        let source = include_str!("../../tests/fixtures/linear-results.frd");
        for (old, new) in [
            ("-2 1 2 3", "-2 1 2 99"),
            ("-2 1 2 3", "-2 1 2"),
            ("-1 1 1\n -2 1 1.0", "-1 1 2\n -2 1 1.0"),
            ("-1 3 0.0 0.2 0.0", "-1 3 0.0 0.2"),
            ("-4 DISP 3 1", "-4 DISP 4 1"),
            ("-1 3 0.0 0.2 0.0", "-1 2 0.0 0.2 0.0"),
            ("-1 1 0.0 0.0 0.0", "-1 1 not-a-number 0.0 0.0"),
        ] {
            assert!(source.contains(old));
            let changed = source.replacen(old, new, 1);
            assert_eq!(read(changed.as_bytes()).unwrap_err().code, "E_FRD", "{new}");
        }
    }

    #[test]
    fn writer_preflights_fixed_width_limits_before_writing() {
        let baseline = read(include_bytes!("../../tests/fixtures/linear-results.frd")).unwrap();
        let mut cases = Vec::new();
        let mut oversized_id = baseline.clone();
        oversized_id.mesh.cells[0].id = 10_000_000_000;
        cases.push(oversized_id);
        let mut oversized_value = baseline.clone();
        oversized_value.fields[0].values[0] = 1e100;
        cases.push(oversized_value);
        let mut invalid_label = baseline.clone();
        invalid_label.fields[0].name = "TOO LONG FIELD".into();
        cases.push(invalid_label);
        let mut invalid_step = baseline;
        invalid_step.fields[0].step = Some(-1);
        cases.push(invalid_step);
        for dataset in cases {
            let mut encoded = Vec::new();
            assert_eq!(write(&dataset, &mut encoded).unwrap_err().code, "E_FRD");
            assert!(encoded.is_empty());
        }
    }
}
