//! Flat Abaqus/CalculiX INP mesh subset. Solver cards are reported as omissions.
use crate::core::{Cell, CellKind, Error, Mesh, Point, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

fn err(message: impl Into<String>) -> Error {
    Error::new("E_INP", message)
}
fn kind(name: &str) -> Result<CellKind> {
    match name {
        "B31" | "T3D2" => Ok(CellKind::Line2),
        "S3" | "CPS3" | "CPE3" => Ok(CellKind::Triangle3),
        "S4" | "CPS4" | "CPE4" => Ok(CellKind::Quad4),
        "C3D4" => Ok(CellKind::Tet4),
        "C3D5" => Ok(CellKind::Pyramid5),
        "C3D6" => Ok(CellKind::Wedge6),
        "C3D8" => Ok(CellKind::Hex8),
        _ => Err(err(format!("unsupported element type {name}"))),
    }
}
fn name(kind: CellKind) -> &'static str {
    match kind {
        CellKind::Line2 => "B31",
        CellKind::Triangle3 => "S3",
        CellKind::Quad4 => "S4",
        CellKind::Tet4 => "C3D4",
        CellKind::Pyramid5 => "C3D5",
        CellKind::Wedge6 => "C3D6",
        CellKind::Hex8 => "C3D8",
    }
}
#[derive(Debug, Clone)]
pub struct Inspection {
    pub mesh: Mesh,
    pub omitted_keywords: BTreeSet<String>,
}

/// Read global *NODE and *ELEMENT blocks. Part instances, includes and generated elements require expansion and fail.
pub fn read(source: &str) -> Result<Inspection> {
    enum Mode {
        None,
        Node,
        Element(CellKind),
    }
    let mut mode = Mode::None;
    let mut mesh = Mesh::default();
    let mut omitted = BTreeSet::new();
    let mut node_ids = BTreeMap::new();
    let mut pending = Vec::<(u64, CellKind, Vec<u64>)>::new();
    let mut seen_node = false;
    let mut seen_element = false;
    for (line_no, line) in source.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("**") {
            continue;
        }
        if let Some(stripped) = line.strip_prefix('*') {
            let mut parts = stripped.split(',').map(str::trim);
            let keyword = parts.next().unwrap_or("").to_ascii_uppercase();
            if matches!(
                keyword.as_str(),
                "INCLUDE"
                    | "PART"
                    | "END PART"
                    | "ASSEMBLY"
                    | "INSTANCE"
                    | "END INSTANCE"
                    | "SYSTEM"
            ) {
                return Err(
                    err(format!("{keyword} needs scoped/expanded geometry")).at(line_no + 1)
                );
            }
            mode = match keyword.as_str() {
                "NODE" => {
                    seen_node = true;
                    Mode::Node
                }
                "ELEMENT" => {
                    seen_element = true;
                    let type_value = parts
                        .find_map(|part| {
                            part.split_once('=')
                                .filter(|(key, _)| key.trim().eq_ignore_ascii_case("TYPE"))
                                .map(|(_, v)| v.trim().to_ascii_uppercase())
                        })
                        .ok_or_else(|| err("ELEMENT requires TYPE").at(line_no + 1))?;
                    Mode::Element(kind(&type_value).map_err(|e| e.at(line_no + 1))?)
                }
                _ => {
                    omitted.insert(keyword);
                    Mode::None
                }
            };
            continue;
        }
        let values: Vec<&str> = line
            .split(',')
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .collect();
        match mode {
            Mode::Node => {
                if values.len() != 4 {
                    return Err(err("NODE needs ID,X,Y,Z").at(line_no + 1));
                }
                let id = values[0]
                    .parse::<u64>()
                    .map_err(|_| err("invalid node ID").at(line_no + 1))?;
                let mut position = [0.; 3];
                for i in 0..3 {
                    position[i] = values[i + 1]
                        .parse()
                        .map_err(|_| err("invalid coordinate").at(line_no + 1))?;
                }
                if node_ids.insert(id, mesh.points.len()).is_some() {
                    return Err(err("duplicate node ID").at(line_no + 1));
                }
                mesh.points.push(Point { id, position });
            }
            Mode::Element(cell_kind) => {
                if values.len() != cell_kind.node_count() + 1 {
                    return Err(
                        err("element node count mismatch or unsupported continuation")
                            .at(line_no + 1),
                    );
                }
                let id = values[0]
                    .parse()
                    .map_err(|_| err("invalid element ID").at(line_no + 1))?;
                let nodes = values[1..]
                    .iter()
                    .map(|v| {
                        v.parse::<u64>()
                            .map_err(|_| err("invalid element node").at(line_no + 1))
                    })
                    .collect::<Result<Vec<_>>>()?;
                pending.push((id, cell_kind, nodes));
            }
            Mode::None => {}
        }
    }
    if !seen_node || !seen_element {
        return Err(err("both NODE and ELEMENT blocks are required"));
    }
    for (id, kind, nodes) in pending {
        let connectivity = nodes
            .into_iter()
            .map(|node| {
                node_ids
                    .get(&node)
                    .copied()
                    .ok_or_else(|| err(format!("unknown node {node}")))
            })
            .collect::<Result<Vec<_>>>()?;
        mesh.cells.push(Cell {
            id,
            kind,
            connectivity,
            property_id: None,
        });
    }
    mesh.validate()?;
    Ok(Inspection {
        mesh,
        omitted_keywords: omitted,
    })
}

/// Emit mesh-only INP. The caller must acknowledge omission of solver semantics and result fields.
pub fn write(mesh: &Mesh, mut writer: impl Write) -> Result<()> {
    mesh.validate()?;
    if mesh.cells.iter().any(|c| c.property_id.is_some()) {
        return Err(err(
            "property IDs have no lossless INP mapping in this writer",
        ));
    }
    writeln!(
        writer,
        "** caexfer geometry projection; no sections, materials, loads, or units\n*NODE"
    )?;
    for p in &mesh.points {
        writeln!(
            writer,
            "{}, {}, {}, {}",
            p.id, p.position[0], p.position[1], p.position[2]
        )?;
    }
    let mut groups: BTreeMap<&str, Vec<&Cell>> = BTreeMap::new();
    for c in &mesh.cells {
        groups.entry(name(c.kind)).or_default().push(c);
    }
    for (element_type, cells) in groups {
        writeln!(writer, "*ELEMENT, TYPE={element_type}")?;
        for c in cells {
            write!(writer, "{}", c.id)?;
            for &idx in &c.connectivity {
                write!(writer, ", {}", mesh.points[idx].id)?;
            }
            writeln!(writer)?;
        }
    }
    Ok(())
}
