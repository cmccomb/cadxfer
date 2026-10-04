use std::collections::{BTreeMap, BTreeSet};

use super::{Error, Result};

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
    /// ```ignore
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

    /// Topological dimension of this linear cell.
    #[must_use]
    pub fn dimension(self) -> u8 {
        match self {
            Self::Line2 => 1,
            Self::Triangle3 | Self::Quad4 => 2,
            Self::Tet4 | Self::Hex8 | Self::Wedge6 | Self::Pyramid5 => 3,
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

/// Named selection of original point IDs, with overlapping membership allowed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeSet {
    /// Selection name from the source, without solver-boundary meaning.
    pub name: String,

    /// Original IDs of selected points.
    pub point_ids: Vec<u64>,
}

/// Named selection of original cell IDs at one topological dimension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellSet {
    /// Selection name from the source, without inferred material or load meaning.
    pub name: String,

    /// Topological dimension of every selected cell.
    pub dimension: u8,

    /// Original IDs of selected cells; different sets may overlap.
    pub cell_ids: Vec<u64>,
}

/// Geometry only. Original IDs are retained; physical units are unspecified.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Mesh {
    /// Points indexed by each cell's `connectivity`.
    pub points: Vec<Point>,

    /// Linear cells in this mesh.
    pub cells: Vec<Cell>,

    /// Named node selections tied to original point IDs.
    pub node_sets: Vec<NodeSet>,

    /// Named cell selections tied to original cell IDs and dimension.
    pub cell_sets: Vec<CellSet>,
}

impl Mesh {
    /// Reject named selections in a writer that cannot encode them.
    pub(crate) fn require_no_sets(&self, code: &'static str) -> Result<()> {
        if !self.node_sets.is_empty() || !self.cell_sets.is_empty() {
            return Err(Error::new(
                code,
                "named node/cell sets have no mapping in this writer",
            ));
        }
        Ok(())
    }

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
    /// ```ignore
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
    ///     ..Mesh::default()
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
        let mut cells = BTreeMap::new();
        for cell in &self.cells {
            if cell.id == 0 || cells.insert(cell.id, cell.kind.dimension()).is_some() {
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
        let mut set_names = BTreeSet::new();
        for set in &self.node_sets {
            if set.name.is_empty() || !set_names.insert(set.name.as_str()) {
                return Err(Error::new(
                    "E_SET",
                    "node set names must be nonempty and unique",
                ));
            }
            let mut members = BTreeSet::new();
            for &id in &set.point_ids {
                if !points.contains(&id) || !members.insert(id) {
                    return Err(Error::new(
                        "E_SET",
                        format!(
                            "node set {} has invalid or repeated point ID {id}",
                            set.name
                        ),
                    ));
                }
            }
        }
        let mut cell_set_names = BTreeSet::new();
        for set in &self.cell_sets {
            if set.name.is_empty()
                || !(1..=3).contains(&set.dimension)
                || !cell_set_names.insert((set.dimension, set.name.as_str()))
            {
                return Err(Error::new(
                    "E_SET",
                    "cell set name/dimension must be unique and valid",
                ));
            }
            let mut members = BTreeSet::new();
            for &id in &set.cell_ids {
                if cells.get(&id) != Some(&set.dimension) || !members.insert(id) {
                    return Err(Error::new(
                        "E_SET",
                        format!(
                            "cell set {} has invalid, repeated, or wrong-dimension cell ID {id}",
                            set.name
                        ),
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

    /// Allow an empty mesh as a structurally valid container.
    #[test]
    fn empty_mesh_is_structurally_valid() {
        Mesh::default().validate().unwrap();
    }

    /// Reject repeated point IDs before any connectivity can become ambiguous.
    #[test]
    fn rejects_duplicate_point_ids() {
        let p = Point {
            id: 1,
            position: [0.0; 3],
        };
        assert_eq!(
            Mesh {
                points: vec![p.clone(), p],
                cells: vec![],
                ..Mesh::default()
            }
            .validate()
            .unwrap_err()
            .code,
            "E_POINT_ID"
        );
    }

    /// Reject NaN coordinates in the shared mesh validator.
    #[test]
    fn rejects_nonfinite_points() {
        let mesh = Mesh {
            points: vec![Point {
                id: 1,
                position: [f64::NAN, 0.0, 0.0],
            }],
            cells: vec![],
            ..Mesh::default()
        };
        assert_eq!(mesh.validate().unwrap_err().code, "E_NONFINITE");
    }

    /// Reject cell connectivity that points outside the point array.
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
            ..Mesh::default()
        };
        assert_eq!(mesh.validate().unwrap_err().code, "E_CONNECTIVITY");
    }

    /// Permit overlapping named sets while checking member IDs and dimensions.
    #[test]
    fn named_sets_allow_overlap_but_require_real_matching_ids() {
        let mut mesh = Mesh {
            points: vec![
                Point {
                    id: 10,
                    position: [0.0; 3],
                },
                Point {
                    id: 20,
                    position: [1.0, 0.0, 0.0],
                },
            ],
            cells: vec![Cell {
                id: 30,
                kind: CellKind::Line2,
                connectivity: vec![0, 1],
                property_id: None,
            }],
            node_sets: vec![NodeSet {
                name: "fixed".into(),
                point_ids: vec![10],
            }],
            cell_sets: vec![
                CellSet {
                    name: "edge-a".into(),
                    dimension: 1,
                    cell_ids: vec![30],
                },
                CellSet {
                    name: "edge-b".into(),
                    dimension: 1,
                    cell_ids: vec![30],
                },
            ],
        };
        mesh.validate().unwrap();
        mesh.cell_sets[1].dimension = 2;
        assert_eq!(mesh.validate().unwrap_err().code, "E_SET");
        mesh.cell_sets[1].dimension = 1;
        mesh.node_sets[0].point_ids.push(999);
        assert_eq!(mesh.validate().unwrap_err().code, "E_SET");
    }
}
