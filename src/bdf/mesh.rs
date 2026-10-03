//! Geometry-only BDF exporter. Output is a mesh exchange deck, not a solver-ready model.
use crate::core::{CellKind, Error, Mesh, Result};
use std::io::Write;

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
pub fn write(mesh: &Mesh, mut writer: impl Write) -> Result<()> {
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
