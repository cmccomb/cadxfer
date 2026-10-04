//! Write long-format ASCII FRD geometry and nodal results.

use super::read::err;
use crate::core::{CellKind, Dataset, FieldLocation, Result};
use std::io::Write;

/// Encode linear topology, including FRD's unsupported pyramid case.
pub(super) fn type_code(kind: CellKind) -> Result<u32> {
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
/// use caexfer::core::Dataset;
/// use caexfer::formats::{frd, inp};
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
    dataset.mesh.require_no_sets("E_FRD")?;
    if dataset
        .mesh
        .cells
        .iter()
        .any(|cell| cell.property_id.is_some())
    {
        return Err(err("FRD cannot encode property IDs"));
    }
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
