//! Exchange supported engineering meshes and results with explicit projection reports.
//!
//! [`conversion::convert_path`] provides format selection and a typed report of
//! omissions and assumptions. [`core::Mesh`] holds geometry and original IDs;
//! [`core::Dataset`] adds complete numeric fields. [`formats`] exposes scoped
//! readers and writers. [`formats::bdf::mesh::read`] projects supported BDF
//! geometry. See the [library guide] for format contracts and installation.
//!
//! [library guide]: https://github.com/cmccomb/caexfer/blob/main/docs/LIBRARY.md
//!
//! ```
//! use caexfer::formats::{bdf, vtu};
//! let projection = bdf::mesh::read("GRID,1,,0,0,0\nGRID,2,,1,0,0\nCROD,10,7,1,2\n")?;
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

pub mod conversion;
pub mod core;
pub mod formats;
