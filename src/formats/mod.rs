//! Format adapters for supported mesh, geometry, and result subsets.
//!
//! Each adapter appears directly under `formats`, regardless of its source
//! directory. Source directories group adapters by role for maintainers.

#[path = "solver_inputs/bdf/mod.rs"]
pub mod bdf;
#[path = "solver_inputs/inp.rs"]
pub mod inp;

#[path = "geometry_only/stl.rs"]
pub mod stl;
#[path = "geometry_only/su2.rs"]
pub mod su2;
#[path = "geometry_only/unv.rs"]
pub mod unv;

#[path = "mesh_datasets/exodus.rs"]
pub mod exodus;
#[path = "mesh_datasets/frd.rs"]
pub mod frd;
#[path = "mesh_datasets/msh.rs"]
pub mod msh;
#[path = "mesh_datasets/vtk.rs"]
pub mod vtk;
#[path = "mesh_datasets/vtu.rs"]
pub mod vtu;

#[path = "companion_results/nastran_result.rs"]
mod nastran_result;
#[path = "companion_results/op2/mod.rs"]
pub mod op2;
#[path = "companion_results/pch.rs"]
pub mod pch;
