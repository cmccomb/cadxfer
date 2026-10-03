//! Shared diagnostics and a deliberately small *geometry* representation.
//!
//! This is not a universal solver model. Materials, loads, constraints, units,
//! and result fields do not silently become properties of a mesh.

use std::collections::BTreeSet;
use std::fmt;

/// An actionable failure with a stable machine-readable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub code: &'static str,
    pub message: String,
    /// One-based physical source line, when available.
    pub line: Option<usize>,
}

impl Error {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            line: None,
        }
    }

    pub fn at(mut self, line: usize) -> Self {
        self.line = Some(line);
        self
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.code)?;
        if let Some(line) = self.line {
            write!(f, " at line {line}")?;
        }
        write!(f, ": {}", self.message)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self::new("E_IO", value.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
    pub line: Option<usize>,
}

impl From<Error> for Diagnostic {
    fn from(error: Error) -> Self {
        Self {
            severity: Severity::Error,
            code: error.code,
            message: error.message,
            line: error.line,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct ValidationReport {
    pub diagnostics: Vec<Diagnostic>,
}

impl ValidationReport {
    /// True only for the explicitly documented geometry validation scope.
    pub fn valid_in_scope(&self) -> bool {
        !self
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }

    pub fn warning_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .count()
    }

    pub fn error_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Point {
    pub id: u64,
    /// Coordinates in the BDF basic frame. No inferred physical units.
    pub position: [f64; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellKind {
    Line2,
    Triangle3,
    Quad4,
    Tet4,
    Hex8,
    Wedge6,
    Pyramid5,
}

impl CellKind {
    pub fn node_count(self) -> usize {
        match self {
            Self::Line2 => 2,
            Self::Triangle3 => 3,
            Self::Quad4 | Self::Tet4 => 4,
            Self::Pyramid5 => 5,
            Self::Wedge6 => 6,
            Self::Hex8 => 8,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub id: u64,
    pub kind: CellKind,
    /// Zero-based indices into `Mesh::points`, not Nastran grid IDs.
    pub connectivity: Vec<usize>,
    pub property_id: Option<u64>,
}

/// Geometry only. Original IDs are retained; physical units are unspecified.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Mesh {
    pub points: Vec<Point>,
    pub cells: Vec<Cell>,
}

impl Mesh {
    /// Check memory/index invariants before a writer emits any bytes.
    /// This does not check Jacobians, inverted cells, or physical adequacy.
    pub fn validate(&self) -> Result<()> {
        let mut points = BTreeSet::new();
        for point in &self.points {
            if point.id == 0 || !points.insert(point.id) {
                return Err(Error::new(
                    "E_POINT_ID",
                    format!("invalid/duplicate point ID {}", point.id),
                ));
            }
            if !point.position.iter().all(|x| x.is_finite()) {
                return Err(Error::new(
                    "E_NONFINITE",
                    format!("point {} is not finite", point.id),
                ));
            }
        }
        let mut cells = BTreeSet::new();
        for cell in &self.cells {
            if cell.id == 0 || !cells.insert(cell.id) {
                return Err(Error::new(
                    "E_CELL_ID",
                    format!("invalid/duplicate cell ID {}", cell.id),
                ));
            }
            if cell.connectivity.len() != cell.kind.node_count() {
                return Err(Error::new(
                    "E_CONNECTIVITY",
                    format!("cell {} has the wrong node count", cell.id),
                ));
            }
            let mut used = BTreeSet::new();
            for &index in &cell.connectivity {
                if index >= self.points.len() {
                    return Err(Error::new(
                        "E_CONNECTIVITY",
                        format!("cell {} references absent point index {index}", cell.id),
                    ));
                }
                if !used.insert(index) {
                    return Err(Error::new(
                        "E_DEGENERATE_CONNECTIVITY",
                        format!("cell {} repeats a point", cell.id),
                    ));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_mesh_is_structurally_valid() {
        Mesh::default().validate().unwrap();
    }

    #[test]
    fn rejects_duplicate_point_ids() {
        let p = Point {
            id: 1,
            position: [0.0; 3],
        };
        assert_eq!(
            Mesh {
                points: vec![p.clone(), p],
                cells: vec![]
            }
            .validate()
            .unwrap_err()
            .code,
            "E_POINT_ID"
        );
    }

    #[test]
    fn rejects_nonfinite_points() {
        let mesh = Mesh {
            points: vec![Point {
                id: 1,
                position: [f64::NAN, 0.0, 0.0],
            }],
            cells: vec![],
        };
        assert_eq!(mesh.validate().unwrap_err().code, "E_NONFINITE");
    }

    #[test]
    fn rejects_bad_indices() {
        let mesh = Mesh {
            points: vec![],
            cells: vec![Cell {
                id: 1,
                kind: CellKind::Line2,
                connectivity: vec![0, 1],
                property_id: None,
            }],
        };
        assert_eq!(mesh.validate().unwrap_err().code, "E_CONNECTIVITY");
    }

    #[test]
    fn warnings_do_not_claim_full_validation() {
        let mut report = ValidationReport::default();
        report.diagnostics.push(Diagnostic {
            severity: Severity::Warning,
            code: "W_TEST",
            message: "not checked".into(),
            line: None,
        });
        assert!(report.valid_in_scope());
        assert_eq!(report.warning_count(), 1);
    }

    #[test]
    fn error_display_carries_line() {
        assert_eq!(
            Error::new("E_TEST", "bad field").at(3).to_string(),
            "E_TEST at line 3: bad field"
        );
    }
}
