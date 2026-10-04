//! ASCII and binary STL triangle surfaces without inferred node sharing.
//!
//! STL stores independent facets, not mesh identifiers or result fields. Reading
//! assigns one-based IDs in facet order and gives each facet three distinct
//! points. Callers can explicitly weld coincident coordinates later if desired.

use crate::core::{Cell, CellKind, Dataset, Error, Mesh, Point, Result};
use std::io::Write;

/// A triangle-surface projection and details absent from the shared mesh.
#[derive(Debug, Clone, PartialEq)]
pub struct Projection {
    /// Facets with deterministic IDs and distinct facet-local vertices.
    pub dataset: Dataset,

    /// Number of source facet normals not represented as mesh fields.
    pub normals: usize,

    /// Number of binary facets with nonzero, nonstandard attribute bytes.
    pub attributed_facets: usize,
}

/// Make one STL-specific error.
fn err(message: impl Into<String>) -> Error {
    Error::new("E_STL", message)
}

/// Append a facet, allocating deterministic one-based IDs without welding.
fn add_facet(mesh: &mut Mesh, vertices: [[f64; 3]; 3]) -> Result<()> {
    let cell_id = u64::try_from(mesh.cells.len() + 1).map_err(|_| err("too many STL facets"))?;
    let start = mesh.points.len();
    let end = start
        .checked_add(3)
        .ok_or_else(|| err("too many STL vertices"))?;
    for (offset, position) in vertices.into_iter().enumerate() {
        if !position.iter().all(|value| value.is_finite()) {
            return Err(err("nonfinite STL coordinate"));
        }
        let id = u64::try_from(start + offset + 1).map_err(|_| err("too many STL vertices"))?;
        mesh.points.push(Point { id, position });
    }
    mesh.cells.push(Cell {
        id: cell_id,
        kind: CellKind::Triangle3,
        connectivity: (start..end).collect(),
        property_id: None,
    });
    Ok(())
}

/// Read a finite little-endian float32 from one binary facet.
fn float(bytes: &[u8]) -> Result<f64> {
    let array: [u8; 4] = bytes.try_into().map_err(|_| err("truncated STL facet"))?;
    let value = f32::from_le_bytes(array);
    if !value.is_finite() {
        return Err(err("nonfinite STL value"));
    }
    Ok(f64::from(value))
}

/// Decode a binary STL whose declared facet count matches its exact byte size.
fn read_binary(bytes: &[u8], count: usize) -> Result<Projection> {
    let mut mesh = Mesh {
        points: Vec::with_capacity(
            count
                .checked_mul(3)
                .ok_or_else(|| err("STL vertex count overflows"))?,
        ),
        cells: Vec::with_capacity(count),
        ..Mesh::default()
    };
    let mut attributed_facets = 0;
    for chunk in bytes[84..].chunks_exact(50) {
        for component in chunk[..12].chunks_exact(4) {
            let _ = float(component)?;
        }
        let mut vertices = [[0.0; 3]; 3];
        for (vertex, encoded) in vertices.iter_mut().zip(chunk[12..48].chunks_exact(12)) {
            for (component, word) in vertex.iter_mut().zip(encoded.chunks_exact(4)) {
                *component = float(word)?;
            }
        }
        attributed_facets += usize::from(chunk[48] != 0 || chunk[49] != 0);
        add_facet(&mut mesh, vertices)?;
    }
    mesh.validate()?;
    Ok(Projection {
        dataset: Dataset {
            mesh,
            fields: vec![],
        },
        normals: count,
        attributed_facets,
    })
}

/// Parse exactly three finite ASCII coordinates.
fn triple<'a>(words: impl Iterator<Item = &'a str>, what: &str) -> Result<[f64; 3]> {
    let values = words
        .map(|word| {
            word.parse::<f64>()
                .map_err(|_| err(format!("invalid {what}")))
        })
        .collect::<Result<Vec<_>>>()?;
    let values: [f64; 3] = values
        .try_into()
        .map_err(|_| err(format!("{what} requires three values")))?;
    if !values.iter().all(|value| value.is_finite()) {
        return Err(err(format!("nonfinite {what}")));
    }
    Ok(values)
}

/// Read the conventional line-oriented ASCII STL grammar.
fn read_ascii(bytes: &[u8]) -> Result<Projection> {
    let source =
        std::str::from_utf8(bytes).map_err(|_| err("STL is neither binary nor UTF-8 ASCII"))?;
    let mut lines = source
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    let header = lines
        .next()
        .ok_or_else(|| err("missing ASCII STL solid header"))?;
    if !header.eq_ignore_ascii_case("solid") && !header.to_ascii_lowercase().starts_with("solid ") {
        return Err(err("missing ASCII STL solid header"));
    }
    let mut mesh = Mesh::default();
    let mut closed = false;
    while let Some(line) = lines.next() {
        if line.eq_ignore_ascii_case("endsolid")
            || line.to_ascii_lowercase().starts_with("endsolid ")
        {
            closed = true;
            break;
        }
        let mut parts = line.split_whitespace();
        if !parts
            .next()
            .is_some_and(|word| word.eq_ignore_ascii_case("facet"))
            || !parts
                .next()
                .is_some_and(|word| word.eq_ignore_ascii_case("normal"))
        {
            return Err(err("expected facet normal"));
        }
        let _normal = triple(parts, "facet normal")?;
        if !lines
            .next()
            .is_some_and(|line| line.eq_ignore_ascii_case("outer loop"))
        {
            return Err(err("expected outer loop"));
        }
        let mut vertices = [[0.0; 3]; 3];
        for vertex in &mut vertices {
            let line = lines
                .next()
                .ok_or_else(|| err("truncated ASCII STL facet"))?;
            let mut words = line.split_whitespace();
            if !words
                .next()
                .is_some_and(|word| word.eq_ignore_ascii_case("vertex"))
            {
                return Err(err("expected vertex"));
            }
            *vertex = triple(words, "vertex")?;
        }
        if !lines
            .next()
            .is_some_and(|line| line.eq_ignore_ascii_case("endloop"))
            || !lines
                .next()
                .is_some_and(|line| line.eq_ignore_ascii_case("endfacet"))
        {
            return Err(err("expected endloop and endfacet"));
        }
        add_facet(&mut mesh, vertices)?;
    }
    if !closed || lines.next().is_some() {
        return Err(err("missing endsolid or trailing STL data"));
    }
    mesh.validate()?;
    let normals = mesh.cells.len();
    Ok(Projection {
        dataset: Dataset {
            mesh,
            fields: vec![],
        },
        normals,
        attributed_facets: 0,
    })
}

/// Read ASCII or binary STL within a caller-supplied byte limit.
///
/// A binary file is recognized by an exact `84 + 50 * count` size, even when
/// its header begins with `solid`. No coordinate welding or unit inference is
/// performed.
///
/// # Errors
///
/// Returns `E_STL` for malformed structure, invalid counts, or nonfinite data.
pub fn read_projection(bytes: &[u8]) -> Result<Projection> {
    if bytes.len() >= 84 {
        let count_bytes: [u8; 4] = bytes[80..84]
            .try_into()
            .map_err(|_| err("truncated STL facet count"))?;
        let count = u32::from_le_bytes(count_bytes);
        let count =
            usize::try_from(count).map_err(|_| err("STL facet count exceeds platform size"))?;
        if count.checked_mul(50).and_then(|size| size.checked_add(84)) == Some(bytes.len()) {
            return read_binary(bytes, count);
        }
    }
    read_ascii(bytes)
}

/// Check that every cell is a triangle and convert coordinates to float32.
#[allow(clippy::cast_possible_truncation)] // Binary STL requires float32; bounds are checked below.
fn triangles(dataset: &Dataset) -> Result<Vec<[[f32; 3]; 3]>> {
    dataset.validate()?;
    dataset.mesh.require_no_sets("E_STL")?;
    if !dataset.fields.is_empty() {
        return Err(err("STL cannot encode numeric fields"));
    }
    let mut output = Vec::with_capacity(dataset.mesh.cells.len());
    for cell in &dataset.mesh.cells {
        if cell.kind != CellKind::Triangle3 {
            return Err(err("STL output requires triangle surface cells"));
        }
        let mut vertices = [[0.0; 3]; 3];
        for (position, &index) in vertices.iter_mut().zip(&cell.connectivity) {
            for (component, &value) in position
                .iter_mut()
                .zip(&dataset.mesh.points[index].position)
            {
                let rounded = value as f32;
                if !rounded.is_finite() || (value != 0.0 && rounded == 0.0) {
                    return Err(err("STL float32 coordinate overflows or underflows"));
                }
                *component = rounded;
            }
        }
        output.push(vertices);
    }
    Ok(output)
}

/// Unit normal computed from the coordinates actually written to STL.
fn normal64(vertices: &[[f64; 3]; 3]) -> Result<[f64; 3]> {
    let [first, second, third] = *vertices;
    let edge_one = [
        second[0] - first[0],
        second[1] - first[1],
        second[2] - first[2],
    ];
    let edge_two = [
        third[0] - first[0],
        third[1] - first[1],
        third[2] - first[2],
    ];
    let cross = [
        edge_one[1] * edge_two[2] - edge_one[2] * edge_two[1],
        edge_one[2] * edge_two[0] - edge_one[0] * edge_two[2],
        edge_one[0] * edge_two[1] - edge_one[1] * edge_two[0],
    ];
    let length = cross[0].hypot(cross[1]).hypot(cross[2]);
    if !length.is_finite() || length == 0.0 {
        return Err(err("degenerate STL triangle has no finite normal"));
    }
    Ok(cross.map(|component| component / length))
}

/// Compute the normal of a float32-encoded facet for binary output.
#[allow(clippy::cast_possible_truncation)] // Unit normal components lie within [-1, 1].
fn normal(vertices: &[[f32; 3]; 3]) -> Result<[f32; 3]> {
    Ok(normal64(&vertices.map(|vertex| vertex.map(f64::from)))?.map(|value| value as f32))
}

/// Write a binary STL triangle surface to a caller-owned stream.
///
/// Geometry and facet winding are written. Numeric fields and named sets are
/// rejected; use the conversion API to report and omit them. IDs and properties
/// have no STL representation.
///
/// # Errors
///
/// Returns `E_STL` for nontriangle cells, unrepresentable float32 coordinates,
/// degenerate triangles, or more than `u32::MAX` facets, and `E_IO` on output
/// failure. A writer error may leave partial bytes in the stream.
pub fn write_data(dataset: &Dataset, mut writer: impl Write) -> Result<()> {
    let facets = triangles(dataset)?;
    let count = u32::try_from(facets.len()).map_err(|_| err("too many STL facets"))?;
    let mut header = [0_u8; 80];
    header[..18].copy_from_slice(b"caexfer binary STL");
    writer.write_all(&header)?;
    writer.write_all(&count.to_le_bytes())?;
    for facet in &facets {
        for value in normal(facet)? {
            writer.write_all(&value.to_le_bytes())?;
        }
        for vertex in facet {
            for value in vertex {
                writer.write_all(&value.to_le_bytes())?;
            }
        }
        writer.write_all(&[0, 0])?;
    }
    Ok(())
}

/// Write conventional ASCII STL with full f64 coordinate precision.
///
/// # Errors
///
/// Returns `E_STL` for nontriangle or degenerate cells and `E_IO` on output
/// failure. A writer error may leave partial bytes in the stream.
pub fn write_ascii(dataset: &Dataset, mut writer: impl Write) -> Result<()> {
    dataset.validate()?;
    dataset.mesh.require_no_sets("E_STL")?;
    if !dataset.fields.is_empty() {
        return Err(err("STL cannot encode numeric fields"));
    }
    if dataset
        .mesh
        .cells
        .iter()
        .any(|cell| cell.kind != CellKind::Triangle3)
    {
        return Err(err("STL output requires triangle surface cells"));
    }
    writeln!(writer, "solid caexfer")?;
    for cell in &dataset.mesh.cells {
        let vertices = cell
            .connectivity
            .clone()
            .try_into()
            .map_err(|_| err("invalid triangle connectivity"))?;
        let vertices: [usize; 3] = vertices;
        let positions = vertices.map(|index| dataset.mesh.points[index].position);
        let n = normal64(&positions)?;
        writeln!(writer, "  facet normal {} {} {}", n[0], n[1], n[2])?;
        writeln!(writer, "    outer loop")?;
        for &index in &cell.connectivity {
            let p = dataset.mesh.points[index].position;
            writeln!(writer, "      vertex {} {} {}", p[0], p[1], p[2])?;
        }
        writeln!(writer, "    endloop")?;
        writeln!(writer, "  endfacet")?;
    }
    writeln!(writer, "endsolid caexfer")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two adjacent facets remain distinct because STL has no node identity.
    #[test]
    fn ascii_facets_do_not_infer_shared_nodes() {
        let source = b"solid sample\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nfacet normal 0 0 1\nouter loop\nvertex 1 0 0\nvertex 1 1 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid sample\n";
        let read = read_projection(source).unwrap();
        assert_eq!(read.dataset.mesh.points.len(), 6);
        assert_eq!(read.dataset.mesh.cells.len(), 2);
        assert_eq!(read.dataset.mesh.cells[1].connectivity, [3, 4, 5]);
        assert_eq!(read.normals, 2);
        let mut binary = Vec::new();
        write_data(&read.dataset, &mut binary).unwrap();
        assert_eq!(binary.len(), 184);
        assert_eq!(read_projection(&binary).unwrap().dataset, read.dataset);
    }

    /// A binary STL may begin with `solid` and still be binary.
    #[test]
    fn binary_header_does_not_determine_dialect() {
        let dataset = read_projection(b"solid sample\nendsolid sample\n")
            .unwrap()
            .dataset;
        let mut bytes = Vec::new();
        write_data(&dataset, &mut bytes).unwrap();
        bytes[..5].copy_from_slice(b"solid");
        assert_eq!(read_projection(&bytes).unwrap().dataset, dataset);
    }

    /// Tiny files cannot induce large allocations through their facet count.
    #[test]
    fn invalid_count_and_malformed_ascii_fail() {
        let mut bytes = [0_u8; 84];
        bytes[80..].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(read_projection(&bytes).unwrap_err().code, "E_STL");
        assert_eq!(
            read_projection(b"solid s\nfacet normal 0 0 1\n")
                .unwrap_err()
                .code,
            "E_STL"
        );
    }

    /// STL writing must reject volume cells and geometry that changes to infinity.
    #[test]
    fn unsupported_geometry_fails() {
        let mut dataset = read_projection(b"solid s\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid s\n").unwrap().dataset;
        dataset.mesh.cells[0].kind = CellKind::Tet4;
        dataset.mesh.cells[0].connectivity.push(0);
        assert!(write_data(&dataset, Vec::new()).is_err());
        dataset.mesh.cells[0].kind = CellKind::Triangle3;
        dataset.mesh.cells[0].connectivity.pop();
        dataset.mesh.points[0].position[0] = f64::MAX;
        assert_eq!(write_data(&dataset, Vec::new()).unwrap_err().code, "E_STL");
        assert!(write_ascii(&dataset, Vec::new()).is_ok());
    }
}
