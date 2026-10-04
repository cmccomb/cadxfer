//! Bounded UNV 2411/2412 geometry exchange.
//!
//! This adapter preserves node and element labels for the six linear element
//! families verified against Gmsh's UNV reader. It reports other datasets and
//! element header metadata as omissions. Linear pyramids have no verified UNV
//! record in that reader and are rejected on output.

use crate::core::{Cell, CellKind, Dataset, Error, Mesh, Point, Result};
use std::collections::BTreeMap;
use std::io::Write;

/// Geometry and source information not retained by the projection.
#[derive(Debug, Clone, PartialEq)]
pub struct Projection {
    /// Validated geometry with original node and element IDs.
    pub dataset: Dataset,

    /// Other UNV dataset numbers skipped after their delimiters were checked.
    pub omitted_datasets: Vec<i64>,

    /// Number of element headers with nonzero entity, physical, or color tags.
    pub tagged_elements: usize,
}

/// Build a format-specific structural error.
fn err(message: impl Into<String>) -> Error {
    Error::new("E_UNV", message)
}

/// Parse exactly the requested number of whitespace-delimited integer words.
fn integers(line: &str, count: usize) -> Result<Vec<i64>> {
    let words = line.split_whitespace().collect::<Vec<_>>();
    if words.len() != count {
        return Err(err(format!("expected {count} integer values")));
    }
    words
        .into_iter()
        .map(|word| word.parse().map_err(|_| err("invalid UNV integer")))
        .collect()
}

/// Convert a positive native label without changing its integer value.
fn label(number: i64) -> Result<u64> {
    u64::try_from(number)
        .ok()
        .filter(|&number| number > 0)
        .ok_or_else(|| err("UNV labels must be positive"))
}

/// Accept either Fortran `D` or ordinary `E` exponents in coordinates.
fn coordinate(word: &str) -> Result<f64> {
    word.replace(['D', 'd'], "E")
        .parse::<f64>()
        .ok()
        .filter(|number| number.is_finite())
        .ok_or_else(|| err("invalid UNV coordinate"))
}

/// Return the supported linear topology for a UNV element descriptor.
fn kind(descriptor: i64) -> Result<CellKind> {
    match descriptor {
        11 | 21 | 22 | 31 => Ok(CellKind::Line2),
        41 | 51 | 61 | 74 | 81 | 91 => Ok(CellKind::Triangle3),
        44 | 54 | 64 | 71 | 84 | 94 => Ok(CellKind::Quad4),
        111 => Ok(CellKind::Tet4),
        101 | 112 => Ok(CellKind::Wedge6),
        104 | 115 => Ok(CellKind::Hex8),
        _ => Err(err(format!(
            "unsupported UNV element descriptor {descriptor}"
        ))),
    }
}

/// Map supported cells to the descriptors emitted by Gmsh.
fn descriptor(kind: CellKind) -> Result<i64> {
    match kind {
        CellKind::Line2 => Ok(21),
        CellKind::Triangle3 => Ok(91),
        CellKind::Quad4 => Ok(94),
        CellKind::Tet4 => Ok(111),
        CellKind::Wedge6 => Ok(112),
        CellKind::Hex8 => Ok(115),
        CellKind::Pyramid5 => Err(err("linear pyramids have no supported UNV descriptor")),
    }
}

/// Advance one physical line or report a truncated record.
fn next<'a>(lines: &mut impl Iterator<Item = &'a str>, what: &str) -> Result<&'a str> {
    lines.next().ok_or_else(|| err(format!("truncated {what}")))
}

/// Read node records through the closing `-1` delimiter.
fn nodes<'a>(lines: &mut impl Iterator<Item = &'a str>, mesh: &mut Mesh, max: usize) -> Result<()> {
    loop {
        let header = next(lines, "2411 node section")?.trim();
        if header == "-1" {
            break;
        }
        let header = integers(header, 4)?;
        let id = label(header[0])?;
        let values = next(lines, "UNV node coordinates")?
            .split_whitespace()
            .map(coordinate)
            .collect::<Result<Vec<_>>>()?;
        if values.len() != 3 {
            return Err(err("UNV node needs three coordinates"));
        }
        if mesh.points.len() >= max {
            return Err(err("node count exceeds input size"));
        }
        mesh.points.push(Point {
            id,
            position: [values[0], values[1], values[2]],
        });
    }
    Ok(())
}

/// Parse element records while resolving node labels to array positions.
fn elements<'a>(
    lines: &mut impl Iterator<Item = &'a str>,
    mesh: &mut Mesh,
    max: usize,
) -> Result<usize> {
    let positions = mesh
        .points
        .iter()
        .enumerate()
        .map(|(index, point)| (point.id, index))
        .collect::<BTreeMap<_, _>>();
    let mut tagged = 0;
    loop {
        let header = next(lines, "2412 element section")?.trim();
        if header == "-1" {
            break;
        }
        let header = integers(header, 6)?;
        let id = label(header[0])?;
        let kind = kind(header[1])?;
        let arity = usize::try_from(header[5]).map_err(|_| err("invalid UNV node count"))?;
        if arity != kind.node_count() {
            return Err(err("UNV descriptor and node count disagree"));
        }
        if header[2..5].iter().any(|&tag| tag != 0) {
            tagged += 1;
        }
        if matches!(header[1], 11 | 21 | 22 | 31) {
            integers(next(lines, "UNV beam orientation")?, 3)?;
        }
        let mut connectivity = Vec::with_capacity(arity);
        while connectivity.len() < arity {
            let words = next(lines, "UNV connectivity")?
                .split_whitespace()
                .collect::<Vec<_>>();
            if words.is_empty() || words.len() > arity - connectivity.len() {
                return Err(err("invalid UNV connectivity row width"));
            }
            for word in words {
                let id = label(word.parse().map_err(|_| err("invalid UNV node label"))?)?;
                connectivity.push(
                    *positions
                        .get(&id)
                        .ok_or_else(|| err(format!("element references missing node {id}")))?,
                );
            }
        }
        if mesh.cells.len() >= max {
            return Err(err("element count exceeds input size"));
        }
        mesh.cells.push(Cell {
            id,
            kind,
            connectivity,
            property_id: None,
        });
    }
    Ok(tagged)
}

/// Read supported geometry datasets from an ASCII UNV file.
///
/// Unknown datasets are skipped only at their documented `-1` boundaries and
/// their numbers are returned for the conversion report. The 2411 node section
/// must precede 2412 elements. Groups and results are not projected.
///
/// # Errors
///
/// Returns `E_UNV` for malformed sections, unsupported element descriptors,
/// dangling connectivity, or count claims beyond the input size.
pub fn read_projection(source: &str) -> Result<Projection> {
    let mut lines = source.lines();
    let mut mesh = Mesh::default();
    let mut seen_nodes = false;
    let mut seen_elements = false;
    let mut omitted_datasets = Vec::new();
    let mut tagged_elements = 0;
    while let Some(line) = lines.next() {
        if line.trim() != "-1" {
            return Err(err("expected UNV dataset delimiter -1"));
        }
        let number = next(&mut lines, "UNV dataset number")?
            .trim()
            .parse::<i64>()
            .map_err(|_| err("invalid UNV dataset number"))?;
        match number {
            2411 if !seen_nodes && !seen_elements => {
                nodes(&mut lines, &mut mesh, source.len() / 2)?;
                seen_nodes = true;
            }
            2412 if seen_nodes && !seen_elements => {
                tagged_elements = elements(&mut lines, &mut mesh, source.len() / 2)?;
                seen_elements = true;
            }
            2411 | 2412 => return Err(err("duplicate or out-of-order UNV geometry section")),
            _ => {
                omitted_datasets.push(number);
                loop {
                    if next(&mut lines, "UNV dataset")?.trim() == "-1" {
                        break;
                    }
                }
            }
        }
    }
    if !seen_nodes || !seen_elements {
        return Err(err("UNV requires 2411 nodes and 2412 elements"));
    }
    let dataset = Dataset {
        mesh,
        fields: Vec::new(),
    };
    dataset.validate()?;
    Ok(Projection {
        dataset,
        omitted_datasets,
        tagged_elements,
    })
}

/// Write geometry as UNV 2411 and 2412 ASCII datasets.
///
/// This direct writer rejects named sets, numeric fields, property IDs,
/// pyramids, and labels beyond signed 32-bit range. The conversion API reports
/// supported omissions before calling it.
///
/// # Errors
///
/// Returns a structural or representability error, or an output I/O error.
pub fn write_data(dataset: &Dataset, mut writer: impl Write) -> Result<()> {
    dataset.validate()?;
    dataset.mesh.require_no_sets("E_UNV")?;
    if !dataset.fields.is_empty() {
        return Err(err("UNV geometry writer cannot encode numeric fields"));
    }
    if dataset
        .mesh
        .cells
        .iter()
        .any(|cell| cell.property_id.is_some())
    {
        return Err(err("UNV geometry writer cannot encode property IDs"));
    }
    for point in &dataset.mesh.points {
        i32::try_from(point.id).map_err(|_| err("node label exceeds Int32"))?;
    }
    for cell in &dataset.mesh.cells {
        i32::try_from(cell.id).map_err(|_| err("element label exceeds Int32"))?;
        descriptor(cell.kind)?;
    }
    writeln!(writer, "    -1\n  2411")?;
    for point in &dataset.mesh.points {
        writeln!(writer, "{:10}{:10}{:10}{:10}", point.id, 1, 1, 11)?;
        writeln!(
            writer,
            "{:25.16E}{:25.16E}{:25.16E}",
            point.position[0], point.position[1], point.position[2]
        )?;
    }
    writeln!(writer, "    -1\n    -1\n  2412")?;
    for cell in &dataset.mesh.cells {
        writeln!(
            writer,
            "{:10}{:10}{:10}{:10}{:10}{:10}",
            cell.id,
            descriptor(cell.kind)?,
            0,
            0,
            7,
            cell.kind.node_count()
        )?;
        if cell.kind == CellKind::Line2 {
            writeln!(writer, "         0         0         0")?;
        }
        for chunk in cell.connectivity.chunks(8) {
            for &index in chunk {
                write!(writer, "{:10}", dataset.mesh.points[index].id)?;
            }
            writeln!(writer)?;
        }
    }
    writeln!(writer, "    -1")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{read_projection, write_data};
    use crate::core::CellKind;

    /// Round-trip the supported UNV families using a Gmsh-generated fixture.
    #[test]
    fn gmsh_six_kind_fixture_round_trips() {
        let source = include_str!("../../../tests/fixtures/gmsh-six-kind.unv");
        let read = read_projection(source).unwrap();
        assert_eq!(read.dataset.mesh.points.len(), 9);
        assert_eq!(read.dataset.mesh.cells.len(), 6);
        assert_eq!(read.omitted_datasets, vec![2477]);
        assert_eq!(read.dataset.mesh.cells[0].kind, CellKind::Line2);
        let mut output = Vec::new();
        write_data(&read.dataset, &mut output).unwrap();
        let second = read_projection(std::str::from_utf8(&output).unwrap()).unwrap();
        assert_eq!(second.dataset, read.dataset);
    }

    /// Reject malformed UNV datasets and unsupported element records.
    #[test]
    fn rejects_malformed_and_unsupported_records() {
        assert!(read_projection("    -1\n  2411\n1 1 1 11\n0 0 0\n").is_err());
        let source = include_str!("../../../tests/fixtures/gmsh-six-kind.unv");
        assert!(read_projection(&source.replace("       111", "       999")).is_err());
        let missing_node = source.replacen("         1         2", "         1       999", 1);
        assert_ne!(missing_node, source);
        assert!(read_projection(&missing_node).is_err());
    }
}
