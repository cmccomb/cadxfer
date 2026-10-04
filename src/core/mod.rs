//! Shared diagnostics, mesh geometry, and explicitly located numeric fields.
//!
//! This is not a universal solver model. Materials, loads, constraints, units,
//! and result fields do not silently become properties of a mesh.

mod error;
mod field;
mod mesh;

pub use error::{Diagnostic, Error, Result, Severity, ValidationReport};
pub use field::{Dataset, Field, FieldLocation};
pub use mesh::{Cell, CellKind, CellSet, Mesh, NodeSet, Point};

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
                cells: vec![],
                ..Mesh::default()
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
            ..Mesh::default()
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
            ..Mesh::default()
        };
        assert_eq!(mesh.validate().unwrap_err().code, "E_CONNECTIVITY");
    }

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
