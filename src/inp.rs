//! Flat Abaqus/CalculiX INP mesh subset. Solver cards are reported as omissions.
use crate::core::{Cell, CellKind, Error, Mesh, Point, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

/// Construct an INP-specific diagnostic without a source line.
fn err(message: impl Into<String>) -> Error {
    Error::new("E_INP", message)
}
/// Resolve supported INP element names to a linear topology.
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
/// Choose the canonical INP element name for mesh-only output.
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
/// Supported INP mesh and names of solver keywords omitted during inspection.
///
/// Inspect [`Self::omitted_keywords`] before projecting the mesh to another
/// format; these cards are not represented by [`Mesh`].
#[derive(Debug, Clone)]
pub struct Inspection {
    /// Global nodes and linear elements from supported blocks.
    pub mesh: Mesh,
    /// Other recognized keyword names whose contents were not interpreted.
    pub omitted_keywords: BTreeSet<String>,
}

/// Read global `*NODE` and `*ELEMENT` blocks into a linear mesh.
///
/// Part instances, includes, coordinate systems, and generated elements need
/// expansion and fail explicitly. Other solver keywords are recorded in
/// [`Inspection::omitted_keywords`] without interpreting their contents.
///
/// # Examples
///
/// ```
/// use caexfer::inp;
/// let input = "*NODE\n1,0,0,0\n2,1,0,0\n*ELEMENT, TYPE=T3D2\n10,1,2\n*MATERIAL, NAME=STEEL\n";
/// let inspected = inp::read(input)?;
/// assert_eq!(inspected.mesh.cells[0].id, 10);
/// assert!(inspected.omitted_keywords.contains("MATERIAL"));
/// # Ok::<(), caexfer::core::Error>(())
/// ```
pub fn read(source: &str) -> Result<Inspection> {
    // Keyword lines switch how subsequent data lines are interpreted.
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
            // Scoped geometry requires expansion that this flat reader cannot
            // perform, so fail before projecting misleading global nodes.
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
                    // Preserve the keyword name as an omission while ignoring
                    // its uninterpreted data lines until the next keyword.
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
                // Record each original node ID and its internal point index.
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
                // Resolve references after all blocks are read; node blocks
                // need not precede element blocks in this flat projection.
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

    // Convert original node IDs to the point indices used by Mesh.
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

/// Write a mesh-only INP deck with global nodes and linear elements.
///
/// Materials, sections, loads, constraints, and result fields are absent from
/// [`Mesh`] and therefore cannot be emitted. Property IDs are rejected because
/// no lossless INP mapping exists here. The caller owns the output stream;
/// write errors can leave partial bytes.
pub fn write(mesh: &Mesh, mut writer: impl Write) -> Result<()> {
    // Property IDs have no mapping here, so reject them before any output.
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

    // INP element blocks are homogeneous by element keyword.
    let mut groups: BTreeMap<&str, Vec<&Cell>> = BTreeMap::new();
    for c in &mesh.cells {
        groups.entry(name(c.kind)).or_default().push(c);
    }
    for (element_type, cells) in groups {
        writeln!(writer, "*ELEMENT, TYPE={element_type}")?;
        for c in cells {
            write!(writer, "{}", c.id)?;
            for &idx in &c.connectivity {
                // Restore original node IDs from the internal connectivity.
                write!(writer, ", {}", mesh.points[idx].id)?;
            }
            writeln!(writer)?;
        }
    }
    Ok(())
}
