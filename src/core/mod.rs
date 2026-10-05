//! Shared diagnostics, mesh geometry, and explicitly located numeric fields.
//!
//! This is not a universal solver model. Materials, loads, constraints, units,
//! and result fields do not silently become properties of a mesh.

mod error;
mod field;
mod mesh;
mod voxel;

pub use error::{Diagnostic, Error, Result, Severity, ValidationReport};
pub use field::{Dataset, Field, FieldLocation};
pub use mesh::{Cell, CellKind, CellSet, Mesh, NodeSet, Point};
pub(crate) use voxel::smooth_surface;
pub use voxel::{VoxelGrid, boundary_surface};
