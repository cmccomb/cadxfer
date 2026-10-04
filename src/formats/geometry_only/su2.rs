//! Bounded single-zone SU2 ASCII mesh with named boundary markers.
//!
//! Interior cells and marker facets share one [`Mesh`] while [`CellSet`] names
//! identify the boundary elements. Node and cell IDs are assigned in source
//! order because native SU2 connectivity uses zero-based array positions.

use crate::core::{Cell, CellKind, CellSet, Dataset, Error, Mesh, Point, Result};
use std::collections::BTreeSet;
use std::io::Write;

/// Geometry projected from one SU2 zone.
#[derive(Debug, Clone, PartialEq)]
pub struct Projection {
    /// Mesh with marker cells in named cell sets.
    pub dataset: Dataset,

    /// Input dimension, which can be useful when no elements are present.
    pub dimension: u8,
}

/// One unvalidated native connectivity record.
struct RawCell {
    /// Native VTK-style kind.
    kind: CellKind,

    /// Zero-based point positions in the later `NPOIN` section.
    indices: Vec<usize>,
}

/// Format-specific structural error.
fn err(message: impl Into<String>) -> Error {
    Error::new("E_SU2", message)
}

/// Parse the value of one required `KEY= value` line.
fn value<'a>(line: &'a str, key: &str) -> Result<&'a str> {
    let (actual, value) = line
        .split_once('=')
        .ok_or_else(|| err(format!("expected {key}=")))?;
    if !actual.trim().eq_ignore_ascii_case(key) {
        return Err(err(format!("expected {key}=")));
    }
    Ok(value.trim())
}

/// Advance to the next nonempty record, allowing `%` comment lines.
fn next<'a>(lines: &mut impl Iterator<Item = &'a str>, what: &str) -> Result<&'a str> {
    lines
        .find(|line| !line.trim().is_empty() && !line.trim_start().starts_with('%'))
        .map(str::trim)
        .ok_or_else(|| err(format!("missing {what}")))
}

/// Check a declared record count against the input byte length.
fn count(line: &str, key: &str, max: usize) -> Result<usize> {
    let count = value(line, key)?
        .parse::<usize>()
        .map_err(|_| err(format!("invalid {key}")))?;
    if count > max {
        return Err(err(format!("{key} exceeds input size")));
    }
    Ok(count)
}

/// Map SU2's VTK-style cell numbers to the seven linear kinds.
fn kind(number: u32) -> Result<CellKind> {
    match number {
        3 => Ok(CellKind::Line2),
        5 => Ok(CellKind::Triangle3),
        9 => Ok(CellKind::Quad4),
        10 => Ok(CellKind::Tet4),
        12 => Ok(CellKind::Hex8),
        13 => Ok(CellKind::Wedge6),
        14 => Ok(CellKind::Pyramid5),
        _ => Err(err(format!("unsupported SU2 element type {number}"))),
    }
}

/// Reverse map for supported linear cells.
fn number(kind: CellKind) -> u8 {
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

/// Parse a connectivity row, ignoring only a validated optional row index.
fn connectivity(line: &str, expected_dimension: u8, ordinal: usize) -> Result<RawCell> {
    let words = line.split_whitespace().collect::<Vec<_>>();
    let number = words
        .first()
        .ok_or_else(|| err("empty SU2 element"))?
        .parse::<u32>()
        .map_err(|_| err("invalid SU2 element type"))?;
    let kind = kind(number)?;
    if kind.dimension() != expected_dimension {
        return Err(err("SU2 element has wrong dimension for its section"));
    }
    let arity = kind.node_count();
    if words.len() != arity + 1 && words.len() != arity + 2 {
        return Err(err("SU2 element has wrong connectivity width"));
    }
    let indices = words[1..=arity]
        .iter()
        .map(|word| {
            word.parse::<usize>()
                .map_err(|_| err("invalid SU2 point index"))
        })
        .collect::<Result<Vec<_>>>()?;
    if words.len() == arity + 2 {
        let explicit = words[arity + 1]
            .parse::<usize>()
            .map_err(|_| err("invalid SU2 element index"))?;
        if explicit != ordinal {
            return Err(err("SU2 element index differs from row order"));
        }
    }
    Ok(RawCell { kind, indices })
}

/// Add one validated raw cell using deterministic one-based IDs.
fn add_cell(mesh: &mut Mesh, raw: RawCell) -> Result<u64> {
    if raw.indices.iter().any(|&index| index >= mesh.points.len()) {
        return Err(err("SU2 element references missing point"));
    }
    let id = mesh
        .cells
        .len()
        .checked_add(1)
        .and_then(|value| u64::try_from(value).ok())
        .ok_or_else(|| err("too many SU2 elements"))?;
    mesh.cells.push(Cell {
        id,
        kind: raw.kind,
        connectivity: raw.indices,
        property_id: None,
    });
    Ok(id)
}

/// Read a single-zone ASCII SU2 mesh, including named boundary markers.
///
/// The file's zero-based connectivity is projected into deterministic one-based
/// point/cell IDs. Neither units nor boundary-condition values are inferred.
///
/// # Errors
///
/// Returns `E_SU2` for unsupported zones, malformed sections, excessive
/// counts, invalid connectivity, or markers with invalid names or facets.
pub fn read_projection(source: &str) -> Result<Projection> {
    let mut lines = source.lines();
    let maximum = source.len();
    let dimension = value(next(&mut lines, "NDIME")?, "NDIME")?
        .parse::<u8>()
        .map_err(|_| err("invalid NDIME"))?;
    if !matches!(dimension, 2 | 3) {
        return Err(err("only 2D and 3D SU2 meshes are supported"));
    }
    let elements = count(next(&mut lines, "NELEM")?, "NELEM", maximum)?;
    let mut interior = Vec::with_capacity(elements);
    for ordinal in 0..elements {
        interior.push(connectivity(
            next(&mut lines, "interior element")?,
            dimension,
            ordinal,
        )?);
    }
    let points = count(next(&mut lines, "NPOIN")?, "NPOIN", maximum)?;
    let mut mesh = Mesh {
        points: Vec::with_capacity(points),
        cells: Vec::with_capacity(elements),
        ..Mesh::default()
    };
    for ordinal in 0..points {
        let words = next(&mut lines, "point")?
            .split_whitespace()
            .collect::<Vec<_>>();
        if words.len() != usize::from(dimension) && words.len() != usize::from(dimension) + 1 {
            return Err(err("SU2 point has wrong coordinate width"));
        }
        let mut position = [0.0_f64; 3];
        for (coordinate, word) in position.iter_mut().zip(&words) {
            *coordinate = word
                .parse::<f64>()
                .map_err(|_| err("invalid SU2 coordinate"))?;
            if !coordinate.is_finite() {
                return Err(err("nonfinite SU2 coordinate"));
            }
        }
        if words.len() == usize::from(dimension) + 1 {
            let explicit = words[usize::from(dimension)]
                .parse::<usize>()
                .map_err(|_| err("invalid SU2 point index"))?;
            if explicit != ordinal {
                return Err(err("SU2 point index differs from row order"));
            }
        }
        mesh.points.push(Point {
            id: u64::try_from(ordinal + 1).map_err(|_| err("too many SU2 points"))?,
            position,
        });
    }
    for raw in interior {
        add_cell(&mut mesh, raw)?;
    }
    let markers = count(next(&mut lines, "NMARK")?, "NMARK", maximum)?;
    for _ in 0..markers {
        let name = value(next(&mut lines, "MARKER_TAG")?, "MARKER_TAG")?.to_owned();
        if name.is_empty() || name.contains(['\n', '\r', '=']) {
            return Err(err("invalid SU2 marker name"));
        }
        let facets = count(next(&mut lines, "MARKER_ELEMS")?, "MARKER_ELEMS", maximum)?;
        let mut cell_ids = Vec::with_capacity(facets);
        for ordinal in 0..facets {
            let raw = connectivity(next(&mut lines, "marker element")?, dimension - 1, ordinal)?;
            cell_ids.push(add_cell(&mut mesh, raw)?);
        }
        mesh.cell_sets.push(CellSet {
            name,
            dimension: dimension - 1,
            cell_ids,
        });
    }
    if lines.any(|line| !line.trim().is_empty() && !line.trim_start().starts_with('%')) {
        return Err(err("trailing or unsupported SU2 sections"));
    }
    mesh.validate()?;
    Ok(Projection {
        dataset: Dataset {
            mesh,
            fields: vec![],
        },
        dimension,
    })
}

/// Write one complete cell connectivity row using SU2 point positions.
fn write_cell(cell: &Cell, writer: &mut impl Write) -> Result<()> {
    write!(writer, "{}", number(cell.kind))?;
    for index in &cell.connectivity {
        write!(writer, " {index}")?;
    }
    writeln!(writer)?;
    Ok(())
}

/// Write a single-zone ASCII SU2 mesh with named lower-dimensional markers.
///
/// Every boundary cell must belong to at least one named cell set. Other cell
/// sets, node sets, source IDs, and fields have no native mapping; conversion
/// reports those losses. A 2D mesh must lie in z=0.
///
/// # Errors
///
/// Returns `E_SU2` when there is no 2D/3D interior, a boundary cell is
/// unmarked, a marker name is invalid, or the mesh is not representable.
pub fn write_data(dataset: &Dataset, mut writer: impl Write) -> Result<()> {
    dataset.validate()?;
    if !dataset.fields.is_empty() {
        return Err(err("SU2 cannot encode numeric fields"));
    }
    let mesh = &dataset.mesh;
    if mesh.cells.iter().any(|cell| cell.property_id.is_some()) {
        return Err(err("SU2 cannot encode property IDs"));
    }
    let dimension = mesh
        .cells
        .iter()
        .map(|cell| cell.kind.dimension())
        .max()
        .ok_or_else(|| err("SU2 requires interior cells"))?;
    if !matches!(dimension, 2 | 3) {
        return Err(err("SU2 requires 2D or 3D interior cells"));
    }
    if !mesh.node_sets.is_empty()
        || mesh
            .cell_sets
            .iter()
            .any(|set| set.dimension != dimension - 1)
    {
        return Err(err("SU2 writer accepts named boundary cell sets only"));
    }
    if dimension == 2 && mesh.points.iter().any(|point| point.position[2] != 0.0) {
        return Err(err("2D SU2 cannot encode nonzero z coordinates"));
    }
    let markers = mesh
        .cell_sets
        .iter()
        .filter(|set| set.dimension == dimension - 1)
        .collect::<Vec<_>>();
    let mut marker_ids = BTreeSet::<u64>::new();
    for marker in &markers {
        if marker.name.contains(['\n', '\r', '=']) {
            return Err(err("invalid SU2 marker name"));
        }
        marker_ids.extend(marker.cell_ids.iter().copied());
    }
    if mesh
        .cells
        .iter()
        .any(|cell| cell.kind.dimension() < dimension && !marker_ids.contains(&cell.id))
    {
        return Err(err("lower-dimensional SU2 cell has no named marker"));
    }
    let interior = mesh
        .cells
        .iter()
        .filter(|cell| cell.kind.dimension() == dimension)
        .collect::<Vec<_>>();
    writeln!(writer, "NDIME= {dimension}")?;
    writeln!(writer, "NELEM= {}", interior.len())?;
    for cell in interior {
        write_cell(cell, &mut writer)?;
    }
    writeln!(writer, "NPOIN= {}", mesh.points.len())?;
    for point in &mesh.points {
        write!(writer, "{} {}", point.position[0], point.position[1])?;
        if dimension == 3 {
            write!(writer, " {}", point.position[2])?;
        }
        writeln!(writer)?;
    }
    writeln!(writer, "NMARK= {}", markers.len())?;
    for marker in markers {
        writeln!(writer, "MARKER_TAG= {}", marker.name)?;
        writeln!(writer, "MARKER_ELEMS= {}", marker.cell_ids.len())?;
        for id in &marker.cell_ids {
            let cell = mesh
                .cells
                .iter()
                .find(|cell| cell.id == *id)
                .ok_or_else(|| err("marker references missing cell"))?;
            write_cell(cell, &mut writer)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_names_and_oriented_edges_roundtrip() {
        let source = "NDIME= 2\nNELEM= 1\n5 0 1 2\nNPOIN= 3\n0 0\n1 0\n0 1\nNMARK= 2\nMARKER_TAG= inlet\nMARKER_ELEMS= 1\n3 0 1\nMARKER_TAG= wall\nMARKER_ELEMS= 2\n3 1 2\n3 2 0\n";
        let first = read_projection(source).unwrap();
        assert_eq!(first.dataset.mesh.cell_sets[0].name, "inlet");
        assert_eq!(first.dataset.mesh.cell_sets[1].cell_ids.len(), 2);
        let mut bytes = Vec::new();
        write_data(&first.dataset, &mut bytes).unwrap();
        let second = read_projection(std::str::from_utf8(&bytes).unwrap()).unwrap();
        assert_eq!(second.dataset, first.dataset);
    }

    #[test]
    fn bad_counts_and_unmarked_boundaries_fail() {
        assert_eq!(
            read_projection("NDIME= 2\nNELEM= 99999999\n")
                .unwrap_err()
                .code,
            "E_SU2"
        );
        let source = "NDIME= 2\nNELEM= 1\n5 0 1 2\nNPOIN= 3\n0 0\n1 0\n0 1\nNMARK= 1\nMARKER_TAG= inlet\nMARKER_ELEMS= 1\n3 0 1\n";
        let mut data = read_projection(source).unwrap().dataset;
        data.mesh.cell_sets.clear();
        assert_eq!(write_data(&data, Vec::new()).unwrap_err().code, "E_SU2");
    }

    #[test]
    fn three_dimensional_linear_cells_round_trip() {
        let kinds = [
            CellKind::Tet4,
            CellKind::Hex8,
            CellKind::Wedge6,
            CellKind::Pyramid5,
        ];
        let points = (0..8)
            .map(|index| Point {
                id: u64::try_from(index + 1).unwrap(),
                position: [f64::from(u32::try_from(index).unwrap()), 0.0, 0.0],
            })
            .collect();
        let cells = kinds
            .into_iter()
            .enumerate()
            .map(|(index, kind)| Cell {
                id: u64::try_from(index + 1).unwrap(),
                kind,
                connectivity: (0..kind.node_count()).collect(),
                property_id: None,
            })
            .collect();
        let dataset = Dataset {
            mesh: Mesh {
                points,
                cells,
                ..Mesh::default()
            },
            fields: Vec::new(),
        };
        let mut output = Vec::new();
        write_data(&dataset, &mut output).unwrap();
        let parsed = read_projection(std::str::from_utf8(&output).unwrap()).unwrap();
        assert_eq!(parsed.dimension, 3);
        assert_eq!(parsed.dataset, dataset);
    }

    #[test]
    fn positional_indices_and_unrecognized_types_fail() {
        let base = "NDIME= 2\nNELEM= 1\n5 0 1 2 0\nNPOIN= 3\n0 0 0\n1 0 1\n0 1 2\nNMARK= 0\n";
        assert_eq!(read_projection(base).unwrap().dataset.mesh.cells.len(), 1);
        for bad in [
            base.replace("5 0 1 2 0", "5 0 1 2 9"),
            base.replace("0 1 2\n", "0 1 9\n"),
            base.replace("5 0 1 2 0", "99 0 1 2 0"),
        ] {
            assert_eq!(read_projection(&bad).unwrap_err().code, "E_SU2");
        }
        let quad = "NDIME= 2\nNELEM= 1\n9 0 1 2 3\nNPOIN= 4\n0 0\n1 0\n1 1\n0 1\nNMARK= 0\n";
        let data = read_projection(quad).unwrap().dataset;
        let mut output = Vec::new();
        write_data(&data, &mut output).unwrap();
        assert_eq!(
            read_projection(std::str::from_utf8(&output).unwrap())
                .unwrap()
                .dataset,
            data
        );
    }
}
