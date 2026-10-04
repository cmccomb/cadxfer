//! Preserve engineering documents and explicitly project supported geometry and fields.
//!
//! Start with [`bdf::Document`] for byte-preserving BDF inspection and copying.
//! Call [`bdf::Document::geometry`] only when a mesh projection is intended,
//! and inspect its omissions. [`core::Mesh`] holds geometry and original IDs;
//! [`core::Dataset`] adds complete numeric fields. [`conversion::convert_path`]
//! provides format selection and a typed omission report; format modules expose
//! their scoped readers and writers. See the [library guide] for a format-by-format
//! map and installation instructions.
//!
//! [library guide]: https://github.com/cmccomb/caexfer/blob/main/docs/LIBRARY.md
//!
//! ```
//! use caexfer::{bdf::Document, vtu};
//! let doc = Document::parse("GRID,1,,0,0,0\nGRID,2,,1,0,0\nCROD,10,7,1,2\n")?;
//! let projection = doc.geometry()?;
//! for omission in &projection.omissions {
//!     eprintln!("{}: {}", omission.category, omission.detail);
//! }
//! let mut bytes = Vec::new();
//! vtu::write(&projection.mesh, &mut bytes)?;
//! let dataset = vtu::read(std::str::from_utf8(&bytes).unwrap())?;
//! assert_eq!(dataset.mesh.points.len(), 2);
//! # Ok::<(), caexfer::core::Error>(())
//! ```
//!
//! Library writers accept caller-owned streams; they can leave partial output
//! after an I/O failure. The CLI provides staged, no-clobber file output.
#![warn(missing_docs)]

pub mod bdf;
pub mod conversion;
pub mod core;
#[path = "mesh_formats/frd.rs"]
pub mod frd;
#[path = "mesh_formats/inp.rs"]
pub mod inp;
#[path = "mesh_formats/msh.rs"]
pub mod msh;
#[path = "nastran_results/nastran_result.rs"]
mod nastran_result;
#[path = "nastran_results/op2.rs"]
pub mod op2;
#[path = "nastran_results/op2_binary.rs"]
mod op2_binary;
#[path = "nastran_results/pch.rs"]
pub mod pch;
#[path = "mesh_formats/stl.rs"]
pub mod stl;
#[path = "mesh_formats/su2.rs"]
pub mod su2;
#[path = "mesh_formats/unv.rs"]
pub mod unv;
#[path = "mesh_formats/vtk.rs"]
pub mod vtk;
#[path = "mesh_formats/vtu.rs"]
pub mod vtu;
