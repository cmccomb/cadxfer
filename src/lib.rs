//! Preserve engineering documents and explicitly project supported geometry and fields.
//!
//! Start with [`bdf::Document`] for byte-preserving BDF inspection or GRID edits.
//! Call [`bdf::Document::geometry`] only when a mesh projection is intended,
//! and inspect its omissions. [`core::Mesh`] holds geometry and original IDs;
//! [`core::Dataset`] adds complete numeric fields. Format modules expose their
//! scoped readers and writers; see the [library guide] for a format-by-format
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
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! Library writers accept caller-owned streams; they can leave partial output
//! after an I/O failure. The CLI provides staged, no-clobber file output.
#![warn(missing_docs)]

pub mod bdf;
pub mod core;
pub mod frd;
pub mod inp;
pub mod msh;
pub mod op2;
pub mod vtu;
