//! Shared diagnostics, mesh geometry, and explicitly located numeric fields.
//!
//! This is not a universal solver model. Materials, loads, constraints, units,
//! and result fields do not silently become properties of a mesh.

use std::collections::BTreeSet;
use std::fmt;

/// An actionable failure with a stable machine-readable code.
///
/// Match on [`Self::code`] rather than the human-readable message. Source
/// locations, when known, use one-based physical line numbers.
///
/// # Examples
///
/// ```
/// use caexfer::core::Error;
/// let error = Error::new("E_SAMPLE", "bad field").at(4);
/// assert_eq!(error.code, "E_SAMPLE");
/// assert_eq!(error.line, Some(4));
/// assert_eq!(error.to_string(), "E_SAMPLE at line 4: bad field");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    /// Stable diagnostic identifier; callers should branch on this, not `message`.
    pub code: &'static str,

    /// Human-readable explanation; wording may change between releases.
    pub message: String,

    /// One-based physical source line, when available.
    pub line: Option<usize>,
}

impl Error {
    /// Build an error without a source location.
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            line: None,
        }
    }

    /// Attach a one-based physical source line.
    #[must_use]
    pub fn at(mut self, line: usize) -> Self {
        self.line = Some(line);
        self
    }
}

impl fmt::Display for Error {
    /// Render the stable code, optional physical line, and human message.
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
    /// Retain an I/O failure's explanation under caexfer's `E_IO` code.
    fn from(value: std::io::Error) -> Self {
        Self::new("E_IO", value.to_string())
    }
}

/// Result returned by caexfer operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Severity of a scoped validation finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Information was omitted or could not be validated in this scope.
    Warning,

    /// The requested scoped operation cannot proceed.
    Error,
}

/// One finding from scoped validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Whether the finding blocks the scoped operation.
    pub severity: Severity,

    /// Stable diagnostic identifier.
    pub code: &'static str,

    /// Human-readable explanation.
    pub message: String,

    /// One-based physical source line, when available.
    pub line: Option<usize>,
}

impl From<Error> for Diagnostic {
    /// Convert a blocking error into one scoped validation finding.
    fn from(error: Error) -> Self {
        Self {
            severity: Severity::Error,
            code: error.code,
            message: error.message,
            line: error.line,
        }
    }
}

/// Findings from validation of a documented subset, not solver correctness.
/// Warnings leave a report valid *within its stated scope*; errors do not.
/// The default report has no findings.
///
/// # Examples
///
/// ```
/// use caexfer::core::{Diagnostic, Severity, ValidationReport};
/// let mut report = ValidationReport::default();
/// report.diagnostics.push(Diagnostic {
///     severity: Severity::Warning,
///     code: "W_OPAQUE",
///     message: "material card not interpreted".into(),
///     line: Some(2),
/// });
/// assert!(report.valid_in_scope());
/// assert_eq!(report.warning_count(), 1);
/// report.diagnostics[0].severity = Severity::Error;
/// assert!(!report.valid_in_scope());
/// ```
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ValidationReport {
    /// Findings in source order.
    pub diagnostics: Vec<Diagnostic>,
}

impl ValidationReport {
    /// True only for the explicitly documented geometry validation scope.
    #[must_use]
    pub fn valid_in_scope(&self) -> bool {
        !self
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }

    /// Number of warning findings.
    #[must_use]
    pub fn warning_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .count()
    }

    /// Number of error findings.
    #[must_use]
    pub fn error_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count()
    }
}

/// One mesh point with its original, positive identifier.
#[derive(Debug, Clone, PartialEq)]
pub struct Point {
    /// Original node ID; never an index into the points array.
    pub id: u64,

    /// Coordinates in the source mesh's represented frame. No inferred units.
    pub position: [f64; 3],
}

/// Supported linear cell topology.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellKind {
    /// Two-node line.
    Line2,

    /// Three-node triangle.
    Triangle3,

    /// Four-node quadrilateral.
    Quad4,

    /// Four-node tetrahedron.
    Tet4,

    /// Eight-node hexahedron.
    Hex8,

    /// Six-node wedge.
    Wedge6,

    /// Five-node pyramid.
    Pyramid5,
}

impl CellKind {
    /// Required number of point indices for this topology.
    ///
    /// ```
    /// use caexfer::core::CellKind;
    /// assert_eq!(CellKind::Triangle3.node_count(), 3);
    /// assert_eq!(CellKind::Hex8.node_count(), 8);
    /// ```
    #[must_use]
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

/// One linear element connected to points in its parent [`Mesh`].
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    /// Original positive element ID.
    pub id: u64,

    /// Linear element topology.
    pub kind: CellKind,

    /// Zero-based indices into `Mesh::points`, not Nastran grid IDs.
    pub connectivity: Vec<usize>,

    /// Source property ID if present; no property definition is stored.
    pub property_id: Option<u64>,
}

/// Geometry only. Original IDs are retained; physical units are unspecified.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Mesh {
    /// Points indexed by each cell's `connectivity`.
    pub points: Vec<Point>,

    /// Linear cells in this mesh.
    pub cells: Vec<Cell>,
}

impl Mesh {
    /// Check positive unique IDs, finite coordinates, and valid connectivity.
    /// Connectivity contains **indices into `points`**, never original point
    /// IDs. This does not check Jacobians, inverted cells, or physical adequacy.
    ///
    /// # Errors
    ///
    /// Returns an error for nonpositive or duplicate IDs, nonfinite coordinates,
    /// or connectivity with missing, repeated, or the wrong number of points.
    ///
    /// # Examples
    ///
    /// ```
    /// use caexfer::core::{Cell, CellKind, Mesh, Point};
    /// let mut mesh = Mesh {
    ///     points: vec![
    ///         Point { id: 10, position: [0.0, 0.0, 0.0] },
    ///         Point { id: 20, position: [1.0, 0.0, 0.0] },
    ///     ],
    ///     cells: vec![Cell {
    ///         id: 100, kind: CellKind::Line2,
    ///         connectivity: vec![0, 1], property_id: None,
    ///     }],
    /// };
    /// // Connectivity holds point positions, not the original IDs 10 and 20.
    /// mesh.validate()?;
    /// mesh.cells[0].connectivity = vec![10, 20];
    /// assert_eq!(mesh.validate().unwrap_err().code, "E_CONNECTIVITY");
    /// # Ok::<(), caexfer::core::Error>(())
    /// ```
    pub fn validate(&self) -> Result<()> {
        // Original IDs identify entities independently of vector position.
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

        // Connectivity uses zero-based vector indices; each cell must have
        // the topology's exact arity and distinct, existing point indices.
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

/// Association of a numeric field with mesh entities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldLocation {
    /// One value tuple per mesh point.
    Point,

    /// One value tuple per mesh cell.
    Cell,
}

/// One field at one step. Values are interleaved by entity and component.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    /// Field name, such as `DISP`.
    pub name: String,

    /// Entity type to which values belong.
    pub location: FieldLocation,

    /// Ordered component names, such as `T1`, `T2`, `T3`.
    pub components: Vec<String>,

    /// Entity-major values: all components for entity 0, then entity 1, etc.
    pub values: Vec<f64>,

    /// Optional source step number; interpretation depends on the format.
    pub step: Option<i64>,

    /// Optional source time; units are not inferred.
    pub time: Option<f64>,
}

/// Mesh plus fields. Solver loads, constraints and material laws are outside this type.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Dataset {
    /// Geometry and original node/element identifiers.
    pub mesh: Mesh,

    /// Complete numeric fields associated with points or cells.
    pub fields: Vec<Field>,
}

impl Dataset {
    /// Check mesh invariants, field tuple lengths, and finite numeric values.
    /// Values are grouped by entity, with components in the declared order.
    /// This does not validate solver physics or format-specific representability.
    ///
    /// # Errors
    ///
    /// Returns a mesh validation error, or an error for incomplete, unnamed,
    /// or nonfinite numeric fields.
    ///
    /// # Examples
    ///
    /// ```
    /// use caexfer::core::{Dataset, Field, FieldLocation, Mesh, Point};
    /// let mut data = Dataset {
    ///     mesh: Mesh {
    ///         points: vec![Point { id: 7, position: [0.0; 3] }],
    ///         cells: vec![],
    ///     },
    ///     fields: vec![Field {
    ///         name: "DISP".into(), location: FieldLocation::Point,
    ///         components: vec!["T1".into(), "T2".into(), "T3".into()],
    ///         values: vec![0.1, 0.0, 0.0], step: None, time: None,
    ///     }],
    /// };
    /// // One point with three components requires exactly three values.
    /// data.validate()?;
    /// data.fields[0].values.pop();
    /// assert_eq!(data.validate().unwrap_err().code, "E_FIELD");
    /// # Ok::<(), caexfer::core::Error>(())
    /// ```
    pub fn validate(&self) -> Result<()> {
        // A field is meaningful only against a structurally valid mesh.
        self.mesh.validate()?;
        for field in &self.fields {
            if field.name.is_empty() || field.components.is_empty() {
                return Err(Error::new(
                    "E_FIELD",
                    "field name and components must be nonempty",
                ));
            }

            // Values are stored entity-major, so tuple width times the
            // location's entity count determines the only valid array length.
            let entities = match field.location {
                FieldLocation::Point => self.mesh.points.len(),
                FieldLocation::Cell => self.mesh.cells.len(),
            };
            if field.values.len() != entities.saturating_mul(field.components.len()) {
                return Err(Error::new(
                    "E_FIELD",
                    format!("field {} has an invalid value count", field.name),
                ));
            }

            // Reject nonfinite payloads and time metadata before serialization.
            if !field.values.iter().all(|value| value.is_finite())
                || field.time.is_some_and(|time| !time.is_finite())
            {
                return Err(Error::new(
                    "E_NONFINITE",
                    format!("field {} contains a nonfinite value", field.name),
                ));
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
