//! Small-scope writer for ASCII VTK XML UnstructuredGrid geometry.
//!
//! No reader, compression, simulation results, or higher-order cells in v0.1.
//! Original IDs use UInt64 arrays rather than lossy floating-point attributes.

use std::io::Write;
use caxifer_core::{CellKind, Error, Mesh, Result};

fn vtk_type(kind: CellKind) -> u8 {
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

/// Write geometry and original node/element/property IDs.
/// Topology is validated before any bytes are written. A writer I/O failure may
/// still leave partial output; the CLI uses a staged no-clobber file operation.
/// Input node ordering must be VTK-compatible for the specified linear cells.
pub fn write(mesh: &Mesh, mut writer: impl Write) -> Result<()> {
    mesh.validate()?;
    let mut entries = 0usize;
    for cell in &mesh.cells {
        entries = entries.checked_add(cell.connectivity.len())
            .ok_or_else(|| Error::new("E_LIMIT", "connectivity size overflows usize"))?;
    }
    i64::try_from(entries).map_err(|_| Error::new("E_LIMIT", "VTU offsets exceed Int64"))?;
    writeln!(writer, "<?xml version=\"1.0\"?>")?;
    writeln!(writer, "<VTKFile type=\"UnstructuredGrid\" version=\"0.1\" byte_order=\"LittleEndian\">")?;
    writeln!(writer, "<!-- caxifer 0.1.0: geometry only; units unspecified -->")?;
    writeln!(writer, "<UnstructuredGrid><Piece NumberOfPoints=\"{}\" NumberOfCells=\"{}\">", mesh.points.len(), mesh.cells.len())?;
    writeln!(writer, "<PointData><DataArray type=\"UInt64\" Name=\"nastran_node_id\" format=\"ascii\">")?;
    for point in &mesh.points { writeln!(writer, "{}", point.id)?; }
    writeln!(writer, "</DataArray></PointData><CellData>")?;
    writeln!(writer, "<DataArray type=\"UInt64\" Name=\"nastran_element_id\" format=\"ascii\">")?;
    for cell in &mesh.cells { writeln!(writer, "{}", cell.id)?; }
    writeln!(writer, "</DataArray><DataArray type=\"UInt64\" Name=\"nastran_property_id\" format=\"ascii\">")?;
    for cell in &mesh.cells { writeln!(writer, "{}", cell.property_id.unwrap_or(0))?; }
    writeln!(writer, "</DataArray></CellData><Points><DataArray type=\"Float64\" NumberOfComponents=\"3\" format=\"ascii\">")?;
    for point in &mesh.points {
        writeln!(writer, "{} {} {}", point.position[0], point.position[1], point.position[2])?;
    }
    writeln!(writer, "</DataArray></Points><Cells><DataArray type=\"Int64\" Name=\"connectivity\" format=\"ascii\">")?;
    for cell in &mesh.cells {
        for index in &cell.connectivity { write!(writer, "{index} ")?; }
        writeln!(writer)?;
    }
    writeln!(writer, "</DataArray><DataArray type=\"Int64\" Name=\"offsets\" format=\"ascii\">")?;
    let mut offset = 0;
    for cell in &mesh.cells { offset += cell.connectivity.len(); writeln!(writer, "{offset}")?; }
    writeln!(writer, "</DataArray><DataArray type=\"UInt8\" Name=\"types\" format=\"ascii\">")?;
    for cell in &mesh.cells { writeln!(writer, "{}", vtk_type(cell.kind))?; }
    writeln!(writer, "</DataArray></Cells></Piece></UnstructuredGrid></VTKFile>")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use caxifer_core::{Cell, Point};

    fn triangle() -> Mesh {
        Mesh {
            points: vec![Point { id: 10, position: [0., 0., 0.] }, Point { id: 20, position: [1., 0., 0.] }, Point { id: 30, position: [0., 1., 0.] }],
            cells: vec![Cell { id: 50, kind: CellKind::Triangle3, connectivity: vec![0, 1, 2], property_id: Some(7) }],
        }
    }

    #[test]
    fn writes_ids_connectivity_and_type() {
        let mut output = Vec::new();
        write(&triangle(), &mut output).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("NumberOfPoints=\"3\" NumberOfCells=\"1\""));
        assert!(text.contains("nastran_node_id"));
        assert!(text.contains("0 1 2 "));
        assert!(text.contains("format=\"ascii\">\n5\n"));
    }

    #[test]
    fn invalid_mesh_writes_nothing() {
        let mut mesh = triangle();
        mesh.cells[0].connectivity[0] = 100;
        let mut output = Vec::new();
        assert!(write(&mesh, &mut output).is_err());
        assert!(output.is_empty());
    }

    #[test]
    fn all_linear_cell_numbers() {
        assert_eq!([CellKind::Line2, CellKind::Triangle3, CellKind::Quad4, CellKind::Tet4, CellKind::Hex8, CellKind::Wedge6, CellKind::Pyramid5].map(vtk_type), [3, 5, 9, 10, 12, 13, 14]);
    }

    #[test]
    fn preserves_large_ids_as_integers() {
        let mut mesh = triangle();
        mesh.points[0].id = 9_007_199_254_740_993;
        let mut output = Vec::new();
        write(&mesh, &mut output).unwrap();
        assert!(String::from_utf8(output).unwrap().contains("9007199254740993"));
    }

    #[test]
    fn propagates_writer_failure() {
        struct Broken;
        impl Write for Broken {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> { Err(std::io::Error::other("test failure")) }
            fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
        }
        assert_eq!(write(&triangle(), Broken).unwrap_err().code, "E_IO");
    }
}
