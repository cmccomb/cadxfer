//! Geometry-only BDF exporter. Output is a mesh exchange deck, not a solver-ready model.
use crate::core::{CellKind, Error, Mesh, Result};
use std::io::Write;

/// Map a linear topology to the BDF geometry card emitted by this writer.
fn name(kind: CellKind) -> &'static str {
    match kind {
        CellKind::Line2 => "CROD",
        CellKind::Triangle3 => "CTRIA3",
        CellKind::Quad4 => "CQUAD4",
        CellKind::Tet4 => "CTETRA",
        CellKind::Hex8 => "CHEXA",
        CellKind::Wedge6 => "CPENTA",
        CellKind::Pyramid5 => "CPYRAM",
    }
}

/// Write only basic-frame GRID and linear element cards. PID is retained when
/// present, otherwise a placeholder PID 1 is used; no property cards are made.
/// The result is a mesh exchange deck, not a runnable solver model.
///
/// # Errors
///
/// Returns an error for invalid mesh connectivity, unrepresentable fields, or
/// a failure in the caller's output stream.
///
/// # Examples
///
/// ```
/// use caexfer::bdf::{write_geometry, Document};
/// let source = Document::parse(
///     "GRID,10,,0,0,0\nGRID,20,,1,0,0\nCROD,30,7,10,20\n"
/// )?;
/// // Export a new geometry deck, then parse it independently.
/// let mut bytes = Vec::new();
/// write_geometry(&source.geometry()?.mesh, &mut bytes)?;
/// let exported = Document::parse(&bytes)?;
/// assert_eq!(exported.geometry()?.mesh.cells[0].id, 30);
/// assert_ne!(exported.to_bytes(), source.to_bytes()); // projection is not a byte copy
/// # Ok::<(), caexfer::core::Error>(())
/// ```
pub fn write(mesh: &Mesh, mut writer: impl Write) -> Result<()> {
    // Verify indices before writing, since output resolves each to a GRID ID.
    mesh.validate()?;
    writeln!(
        writer,
        "$ caexfer geometry projection; no solver properties, loads, constraints, or units"
    )?;
    writeln!(writer, "BEGIN BULK")?;
    for p in &mesh.points {
        writeln!(
            writer,
            "GRID,{},,{},{},{}",
            p.id, p.position[0], p.position[1], p.position[2]
        )?;
    }

    // BDF element cards use original node IDs; a missing PID gets placeholder
    // 1 because this geometry-only deck has no property definitions.
    for c in &mesh.cells {
        let mut fields = vec![
            name(c.kind).to_string(),
            c.id.to_string(),
            c.property_id.unwrap_or(1).to_string(),
        ];
        fields.extend(
            c.connectivity
                .iter()
                .map(|&index| mesh.points[index].id.to_string()),
        );
        if fields.iter().any(|field| field.contains([',', '\n'])) {
            return Err(Error::new("E_BDF", "unrepresentable field"));
        }

        // Free-field lines contain at most eight data fields; continuation is explicit.
        if fields.len() <= 9 {
            writeln!(writer, "{}", fields.join(","))?;
        } else {
            writeln!(writer, "{}", fields[..9].join(","))?;
            writeln!(writer, "+,{}", fields[9..].join(","))?;
        }
    }
    writeln!(writer, "ENDDATA")?;
    Ok(())
}
