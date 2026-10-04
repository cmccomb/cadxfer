//! Geometry-only BDF exchange. Output is a mesh deck, not a solver-ready model.
use super::{GeometryProjection, ParseOptions, ParsedBdf};
use crate::core::{CellKind, Error, Mesh, Result};
use std::io::{Read, Write};

/// Parse BDF bytes and project supported linear geometry with an omission report.
///
/// It rejects unresolved geometry. Source formatting and solver cards are
/// outside the projected mesh and appear in the omission report.
///
/// # Errors
///
/// Returns a BDF parse error or a blocking geometry projection diagnostic.
///
#[cfg(test)]
pub fn read(input: impl AsRef<[u8]>) -> Result<GeometryProjection> {
    ParsedBdf::parse(input)?.geometry()
}

/// Read geometry from a stream with an explicit byte limit.
///
/// # Errors
///
/// Returns a read, limit, syntax, or geometry error.
pub fn read_from(input: impl Read, max_bytes: usize) -> Result<GeometryProjection> {
    let limit = max_bytes
        .checked_add(1)
        .ok_or_else(|| Error::new("E_LIMIT", "max_bytes is too large"))?;
    let mut bytes = Vec::new();
    input.take(limit as u64).read_to_end(&mut bytes)?;
    ParsedBdf::parse_with_options(
        bytes,
        ParseOptions {
            max_bytes,
            ..ParseOptions::default()
        },
    )?
    .geometry()
}

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
pub fn write(mesh: &Mesh, mut writer: impl Write) -> Result<()> {
    // Verify indices before writing, since output resolves each to a GRID ID.
    mesh.validate()?;
    mesh.require_no_sets("E_BDF")?;
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
