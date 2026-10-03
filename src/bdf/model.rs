use std::collections::{btree_map::Entry, BTreeMap, BTreeSet};

use super::{parse_real, Card, Document};
use crate::core::{
    Cell, CellKind, Diagnostic, Error, Mesh, Point, Result, Severity, ValidationReport,
};

/// Parsed GRID values in the native coordinate frame of the source card.
#[derive(Debug, Clone, PartialEq)]
pub struct Grid {
    /// Positive GRID identifier.
    pub id: u64,
    /// Coordinate frame identifier; zero means the basic frame.
    pub cp: u64,
    /// Native CP-frame coordinates; `geometry()` requires CP=0.
    pub coordinates: [f64; 3],
    /// Output coordinate frame identifier.
    pub cd: i64,
    /// Permanent single-point constraint digits, if any.
    pub ps: String,
    /// Superelement identifier.
    pub seid: u64,
    /// One-based physical line containing the GRID card.
    pub line: usize,
}

/// One category of information excluded from a geometry projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Omission {
    /// Kind of omitted information.
    pub category: String,
    /// Number of affected source records.
    pub count: usize,
    /// Human-readable explanation.
    pub detail: String,
}

/// Geometry extracted from a BDF plus a report of discarded semantics.
#[derive(Debug, Clone)]
pub struct GeometryProjection {
    /// Supported points and linear cells with their original IDs.
    pub mesh: Mesh,
    /// Information intentionally absent from this geometry-only projection.
    pub omissions: Vec<Omission>,
}

/// Parse a required positive BDF ID with the originating physical line.
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

/// Parse an optional nonnegative BDF integer; a blank means the defined zero.
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

/// Recognize element cards that have a supported linear mesh projection.
/// Other cards remain in the original document and are reported as omissions.
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
    /// Decode one GRID's native values without resolving external defaults.
    /// GRDSET and INCLUDE make those defaults ambiguous and block typed access.
    pub(crate) fn parse_grid(&self, card: &Card) -> Result<Grid> {
        // A native GRID cannot be interpreted while external defaults may
        // change its coordinate or output frame.
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

        // Blank coordinate fields default to zero on GRID cards.
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

        // PS is a set of constraint digits, so repeated digits are invalid.
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

    /// Iterate over GRID cards in source order, reporting a parse error per card.
    /// Coordinates remain in each GRID's native CP frame. A blank CP is zero;
    /// an unresolved GRDSET or INCLUDE blocks typed interpretation.
    ///
    /// # Examples
    ///
    /// ```
    /// use caexfer::bdf::Document;
    /// let doc = Document::parse("GRID,42,,1.0,2.0,3.0\n")?;
    /// let grid = doc.grids().next().unwrap()?;
    /// assert_eq!(grid.id, 42);
    /// assert_eq!(grid.cp, 0);
    /// assert_eq!(grid.coordinates, [1.0, 2.0, 3.0]);
    /// # Ok::<(), caexfer::core::Error>(())
    /// ```
    pub fn grids(&self) -> impl Iterator<Item = Result<Grid>> + '_ {
        self.cards
            .iter()
            .filter(|card| card.name() == "GRID")
            .map(|card| self.parse_grid(card))
    }

    /// Read one supported element's ID, explicit property, and GRID references.
    /// Extra solid-element fields are rejected to avoid silently linearizing
    /// higher-order cells; BDF source bytes are never modified here.
    fn parse_element(&self, card: &Card, kind: CellKind) -> Result<NativeElement> {
        // CONROD has no property field; other supported cards require an
        // explicit PID so dialect-specific defaults are never guessed.
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

        // Extra solid-card fields may indicate higher-order geometry, which
        // cannot be reduced to a linear cell without losing nodes.
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

    /// Project all supported GRID and element cards into a mesh and findings.
    /// Keeps collecting scoped diagnostics so callers can inspect multiple
    /// omissions or errors from one document in a single pass.
    fn analyze_geometry(&self) -> (ValidationReport, Mesh) {
        // Collect source diagnostics and native IDs before forming indexed
        // connectivity; this lets one validation pass report multiple issues.
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
                "ENDDATA" => {},
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

        // Opaque nongeometry cards are counted by type for the scoped report.
        for (name, count) in self.card_counts() {
            if opaque_nongeometry(&name) {
                report.diagnostics.push(Diagnostic {
                    severity: Severity::Warning, code: "W_OPAQUE_CARD",
                    message: format!("{count} {name} card(s) preserved but not semantically validated; absent from geometry export"),
                    line: None,
                });
            }
        }

        // BTreeMap iteration gives deterministic ID order in the projected
        // points array, independent of source-card order.
        let mut mesh = Mesh::default();
        let mut node_indices = BTreeMap::new();
        for (id, grid) in points {
            node_indices.insert(id, mesh.points.len());
            mesh.points.push(Point {
                id,
                position: grid.coordinates,
            });
        }

        // Translate native GRID IDs to point indices, recording missing and
        // repeated references while retaining other usable cells.
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

    /// Check the documented geometry subset, **not** solver correctness.
    /// Unimplemented geometry-affecting cards, defaults, and coordinate frames
    /// cause errors. Materials, properties, and loads are opaque warnings.
    /// Call this before [`Self::geometry`] when all diagnostics are needed.
    ///
    /// # Examples
    ///
    /// ```
    /// use caexfer::bdf::Document;
    /// let doc = Document::parse("GRID,1,,0,0,0\nMAT1,5,1.0\n")?;
    /// let report = doc.validate_geometry();
    /// assert!(report.valid_in_scope());
    /// assert!(report.warning_count() > 0); // MAT1 is outside the geometry view
    /// # Ok::<(), caexfer::core::Error>(())
    /// ```
    pub fn validate_geometry(&self) -> ValidationReport {
        self.analyze_geometry().0
    }

    /// Explicitly lossy projection. Original IDs survive; materials, loads,
    /// constraints, section details, element offsets/orientations, GRID CD/PS,
    /// comments, and source formatting do not. Always inspect `omissions`.
    /// Geometry requiring unresolved coordinate frames or unknown elements
    /// returns an error instead of a partial mesh.
    ///
    /// # Examples
    ///
    /// ```
    /// use caexfer::bdf::Document;
    /// let doc = Document::parse(
    ///     "GRID,10,,0,0,0\nGRID,20,,1,0,0\nCROD,30,7,10,20\n"
    /// )?;
    /// let projection = doc.geometry()?;
    /// assert_eq!(projection.mesh.points[0].id, 10);
    /// assert_eq!(projection.mesh.cells[0].id, 30);
    /// assert_eq!(projection.mesh.cells[0].connectivity, [0, 1]);
    /// assert!(!projection.omissions.is_empty());
    /// # Ok::<(), caexfer::core::Error>(())
    /// ```
    pub fn geometry(&self) -> Result<GeometryProjection> {
        // Refuse a partial mesh if scoped validation found any blocking error.
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

        // The projection always loses the original document representation,
        // with additional omissions for full-deck and opaque card content.
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
