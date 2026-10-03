use std::collections::{btree_map::Entry, BTreeMap, BTreeSet};

use crate::{parse_real, Card, Document};
use cadxfer_core::{
    Cell, CellKind, Diagnostic, Error, Mesh, Point, Result, Severity, ValidationReport,
};

#[derive(Debug, Clone, PartialEq)]
pub struct Grid {
    pub id: u64,
    pub cp: u64,
    /// Native CP-frame coordinates; `geometry()` requires CP=0.
    pub coordinates: [f64; 3],
    pub cd: i64,
    pub ps: String,
    pub seid: u64,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Omission {
    pub category: String,
    pub count: usize,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub struct GeometryProjection {
    pub mesh: Mesh,
    /// Information intentionally absent from this geometry-only projection.
    pub omissions: Vec<Omission>,
}

fn positive(input: &str, field: &str, line: usize) -> Result<u64> {
    let value = input.parse::<u64>().map_err(|_| {
        Error::new(
            "E_INTEGER",
            format!("{field} must be a positive integer, got {input:?}"),
        )
        .at(line)
    })?;
    if value == 0 {
        return Err(Error::new("E_INTEGER", format!("{field} must be positive")).at(line));
    }
    Ok(value)
}

fn nonnegative(input: &str, field: &str, line: usize) -> Result<u64> {
    if input.is_empty() {
        return Ok(0);
    }
    input.parse::<u64>().map_err(|_| {
        Error::new(
            "E_INTEGER",
            format!("{field} must be nonnegative, got {input:?}"),
        )
        .at(line)
    })
}

/// These cards can be excluded from a *geometry-only* view. They are preserved
/// as source, not claimed to be semantically supported or solver-validated.
fn opaque_nongeometry(name: &str) -> bool {
    matches!(
        name,
        "MAT1"
            | "MAT2"
            | "MAT3"
            | "MAT4"
            | "MAT5"
            | "MAT8"
            | "MAT9"
            | "PSHELL"
            | "PSOLID"
            | "PLSOLID"
            | "PROD"
            | "PBAR"
            | "PBARL"
            | "PBEAM"
            | "PBEAML"
            | "PCOMP"
            | "PCOMPG"
            | "PARAM"
            | "FORCE"
            | "FORCE1"
            | "FORCE2"
            | "MOMENT"
            | "MOMENT1"
            | "MOMENT2"
            | "PLOAD"
            | "PLOAD1"
            | "PLOAD2"
            | "PLOAD4"
            | "GRAV"
            | "LOAD"
            | "SPC"
            | "SPC1"
            | "SPCADD"
            | "SPCD"
            | "MPC"
            | "MPCADD"
            | "SET1"
            | "SET3"
            | "EIGR"
            | "EIGRL"
            | "TSTEP"
            | "TSTEPNL"
            | "FREQ"
            | "FREQ1"
            | "FREQ2"
            | "DLOAD"
            | "RLOAD1"
            | "RLOAD2"
            | "TLOAD1"
            | "TLOAD2"
            | "TABLED1"
            | "TABLED2"
            | "TABLED3"
            | "TABLED4"
            | "CORD1R"
            | "CORD1C"
            | "CORD1S"
            | "CORD2R"
            | "CORD2C"
            | "CORD2S"
    )
}

fn element_kind(name: &str) -> Option<CellKind> {
    match name {
        "CROD" | "CONROD" | "CBAR" | "CBEAM" => Some(CellKind::Line2),
        "CTRIA3" => Some(CellKind::Triangle3),
        "CQUAD4" => Some(CellKind::Quad4),
        "CTETRA" => Some(CellKind::Tet4),
        "CHEXA" => Some(CellKind::Hex8),
        "CPENTA" => Some(CellKind::Wedge6),
        "CPYRAM" => Some(CellKind::Pyramid5),
        _ => None,
    }
}

struct NativeElement {
    id: u64,
    property_id: Option<u64>,
    nodes: Vec<u64>,
    kind: CellKind,
    line: usize,
}

impl Document {
    pub(crate) fn parse_grid(&self, card: &Card) -> Result<Grid> {
        if self.has_grdset {
            return Err(Error::new(
                "E_GRDSET",
                "GRDSET defaults are unresolved; native GRID values cannot be interpreted safely",
            )
            .at(card.line));
        }
        if !self.include_lines.is_empty() {
            return Err(Error::new("E_INCLUDE_UNRESOLVED", "INCLUDE may supply GRID defaults; typed GRID interpretation is disabled until the deck is flattened").at(card.line));
        }
        let f = |i| self.card_text(card, i);
        let id = positive(f(0), "GRID ID", card.line)?;
        let cp = nonnegative(f(1), "GRID CP", card.line)?;
        let mut coordinates = [0.0; 3];
        for (axis, value) in coordinates.iter_mut().enumerate() {
            if !f(axis + 2).is_empty() {
                *value = parse_real(f(axis + 2)).map_err(|e| e.at(card.line))?;
            }
        }
        let cd = if f(5).is_empty() {
            0
        } else {
            f(5).parse::<i64>()
                .map_err(|_| Error::new("E_INTEGER", "invalid GRID CD").at(card.line))?
        };
        if cd < -1 {
            return Err(Error::new("E_INTEGER", "GRID CD must be >= -1").at(card.line));
        }
        let ps = f(6).to_string();
        let mut seen = BTreeSet::new();
        if !ps
            .bytes()
            .all(|byte| (b'1'..=b'6').contains(&byte) && seen.insert(byte))
        {
            return Err(Error::new(
                "E_GRID_PS",
                "GRID PS must contain unique digits 1 through 6",
            )
            .at(card.line));
        }
        let seid = nonnegative(f(7), "GRID SEID", card.line)?;
        if (8..card.fields.len()).any(|i| !f(i).is_empty()) {
            return Err(
                Error::new("E_GRID_FIELDS", "unexpected data after GRID SEID").at(card.line),
            );
        }
        Ok(Grid {
            id,
            cp,
            coordinates,
            cd,
            ps,
            seid,
            line: card.line,
        })
    }

    pub fn grids(&self) -> impl Iterator<Item = Result<Grid>> + '_ {
        self.cards
            .iter()
            .filter(|card| card.name() == "GRID")
            .map(|card| self.parse_grid(card))
    }

    fn parse_element(&self, card: &Card, kind: CellKind) -> Result<NativeElement> {
        let f = |i| self.card_text(card, i);
        let id = positive(f(0), "element ID", card.line)?;
        let conrod = card.name() == "CONROD";
        let property_id = if conrod {
            None
        } else {
            // Some dialects default PID to EID. Do not guess that rule here.
            Some(positive(
                f(1),
                "element PID (explicit value required in v0.1)",
                card.line,
            )?)
        };
        let first_node = if conrod { 1 } else { 2 };
        let mut nodes = Vec::new();
        for index in 0..kind.node_count() {
            nodes.push(positive(
                f(first_node + index),
                "element GRID reference",
                card.line,
            )?);
        }
        if matches!(card.name(), "CTETRA" | "CHEXA" | "CPENTA" | "CPYRAM") {
            let first_extra = first_node + kind.node_count();
            if (first_extra..card.fields.len()).any(|i| !f(i).is_empty()) {
                return Err(Error::new("E_HIGH_ORDER", format!("{} contains extra nodes/data; higher-order cells are not reduced to linear cells", card.name())).at(card.line));
            }
        }
        Ok(NativeElement {
            id,
            property_id,
            nodes,
            kind,
            line: card.line,
        })
    }

    fn analyze_geometry(&self) -> (ValidationReport, Mesh) {
        let mut report = ValidationReport::default();
        let mut points = BTreeMap::new();
        let mut native_cells = BTreeMap::new();
        for card in &self.cards {
            match card.name() {
                "GRID" => match self.parse_grid(card) {
                    Ok(grid) => {
                        if grid.cp != 0 {
                            report.diagnostics.push(Error::new("E_COORDINATE_SYSTEM", format!("GRID {} uses CP={}; v0.1 does not resolve coordinate systems", grid.id, grid.cp)).at(grid.line).into());
                        }
                        if grid.seid != 0 {
                            report.diagnostics.push(Error::new("E_SUPERELEMENT", format!("GRID {} uses SEID={}; superelements are unsupported", grid.id, grid.seid)).at(grid.line).into());
                        }
                        match points.entry(grid.id) {
                            Entry::Vacant(entry) => { entry.insert(grid); }
                            Entry::Occupied(_) => report.diagnostics.push(Error::new("E_DUPLICATE_GRID", format!("duplicate GRID {}", grid.id)).at(grid.line).into()),
                        }
                    }
                    Err(error) => report.diagnostics.push(error.into()),
                },
                "INCLUDE" => report.diagnostics.push(Error::new("E_INCLUDE_UNRESOLVED", "INCLUDE is preserved but never followed; flatten the deck with a trusted tool before geometry projection").at(card.line).into()),
                "GRDSET" => report.diagnostics.push(Error::new("E_GRDSET", "GRID defaults from GRDSET are not implemented; no coordinate defaults will be guessed").at(card.line).into()),
                name if element_kind(name).is_some() => {
                    // Guard above establishes the match; no user input can make this None.
                    if let Some(kind) = element_kind(name) {
                        match self.parse_element(card, kind) {
                            Ok(element) => {
                                match native_cells.entry(element.id) {
                                    Entry::Vacant(entry) => { entry.insert(element); }
                                    Entry::Occupied(_) => report.diagnostics.push(Error::new("E_DUPLICATE_ELEMENT", format!("duplicate element {}", element.id)).at(element.line).into()),
                                }
                            }
                            Err(error) => report.diagnostics.push(error.into()),
                        }
                    }
                }
                name if opaque_nongeometry(name) => {
                    // Counted once per card type below, not once per large deck record.
                }
                name => report.diagnostics.push(Error::new("E_UNSUPPORTED_CARD", format!("{name} is preserved, but its effect on geometry is unknown; projection is refused")).at(card.line).into()),
            }
        }
        for (name, count) in self.card_counts() {
            if opaque_nongeometry(&name) {
                report.diagnostics.push(Diagnostic {
                    severity: Severity::Warning, code: "W_OPAQUE_CARD",
                    message: format!("{count} {name} card(s) preserved but not semantically validated; absent from geometry export"),
                    line: None,
                });
            }
        }
        let mut mesh = Mesh::default();
        let mut node_indices = BTreeMap::new();
        for (id, grid) in points {
            node_indices.insert(id, mesh.points.len());
            mesh.points.push(Point {
                id,
                position: grid.coordinates,
            });
        }
        for (_, element) in native_cells {
            let mut connectivity = Vec::new();
            let mut seen = BTreeSet::new();
            let mut valid = true;
            for id in element.nodes {
                if !seen.insert(id) {
                    report.diagnostics.push(
                        Error::new(
                            "E_DEGENERATE_CONNECTIVITY",
                            format!("element {} repeats GRID {id}", element.id),
                        )
                        .at(element.line)
                        .into(),
                    );
                    valid = false;
                }
                match node_indices.get(&id) {
                    Some(&index) => connectivity.push(index),
                    None => {
                        report.diagnostics.push(
                            Error::new(
                                "E_MISSING_GRID",
                                format!("element {} references missing GRID {id}", element.id),
                            )
                            .at(element.line)
                            .into(),
                        );
                        valid = false;
                    }
                }
            }
            if valid {
                mesh.cells.push(Cell {
                    id: element.id,
                    kind: element.kind,
                    connectivity,
                    property_id: element.property_id,
                });
            }
        }
        if mesh.points.is_empty() {
            report
                .diagnostics
                .push(Error::new("E_EMPTY_GEOMETRY", "no usable GRID points found").into());
        }
        (report, mesh)
    }

    /// Check the documented geometry subset, NOT solver correctness.
    /// Unimplemented geometry-affecting cards, defaults and coordinate frames
    /// cause errors. Materials/properties/loads are opaque and cause warnings.
    pub fn validate_geometry(&self) -> ValidationReport {
        self.analyze_geometry().0
    }

    /// Explicitly lossy projection. Original IDs survive; materials, loads,
    /// constraints, section details, element offsets/orientations, GRID CD/PS,
    /// comments, and source formatting do not. Always inspect `omissions`.
    pub fn geometry(&self) -> Result<GeometryProjection> {
        let (report, mesh) = self.analyze_geometry();
        if let Some(diagnostic) = report
            .diagnostics
            .iter()
            .find(|d| d.severity == Severity::Error)
        {
            return Err(Error {
                code: diagnostic.code,
                message: diagnostic.message.clone(),
                line: diagnostic.line,
            });
        }
        mesh.validate()?;
        let mut omissions = Vec::new();
        omissions.push(Omission {
            category: "document".into(), count: 1,
            detail: "geometry-only: source text, comments, GRID CD/PS, element orientations/offsets/section data, and solver semantics are not represented; units remain unspecified".into(),
        });
        if self.full_deck {
            omissions.push(Omission {
                category: "control-sections".into(),
                count: 1,
                detail: "executive and case-control sections are not exported".into(),
            });
        }
        for (name, count) in self.card_counts() {
            if opaque_nongeometry(&name) {
                omissions.push(Omission {
                    category: name,
                    count,
                    detail: "opaque nongeometry card(s) not exported".into(),
                });
            }
        }
        Ok(GeometryProjection { mesh, omissions })
    }
}
