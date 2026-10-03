//! Source-preserving Nastran Bulk Data documents.
//!
//! Parsing indexes the original bytes. Unknown cards and comments remain in the
//! document; writing it reproduces the input byte-for-byte.
//! Semantic access and geometry projection are separate, fallible operations.
//! For a geometry-only exchange, use [`read_geometry`] and [`write_geometry`].
//! Use [`Document`] when the original BDF bytes or native cards matter.
//!
//! ```
//! use caexfer::bdf::Document;
//! let original = b"$ original\r\nGRID,1,,0.,0.,0.\r\n";
//! let doc = Document::parse(original).unwrap();
//! assert_eq!(doc.to_bytes(), original);
//! assert_eq!(doc.grids().next().unwrap().unwrap().coordinates, [0.0, 0.0, 0.0]);
//! let mut copy = Vec::new();
//! doc.write_to(&mut copy).unwrap();
//! assert_eq!(copy, original);
//! ```

mod mesh;
mod model;
mod number;
mod syntax;

pub use crate::core::{Error, Result};
pub use mesh::write as write_geometry;
pub use model::{GeometryProjection, Grid, Omission};
pub use number::parse_real;
pub use syntax::{Card, Document, ParseOptions};

/// Parse BDF bytes and project supported linear geometry with an omission report.
///
/// This convenience entry point is for mesh exchange. It rejects unresolved
/// geometry and does not return the source document. Use [`Document::parse`]
/// when comments, unknown cards, and exact source bytes must remain available.
///
/// # Examples
///
/// ```
/// use caexfer::bdf;
/// let source = b"$ original comment\nGRID,10,,0,0,0\nGRID,20,,1,0,0\nCROD,30,7,10,20\n";
/// let projection = bdf::read_geometry(source)?;
/// assert_eq!(projection.mesh.points.len(), 2);
/// let mut output = Vec::new();
/// bdf::write_geometry(&projection.mesh, &mut output)?;
/// assert_ne!(output, source); // The exchange deck is a projection, not a byte copy.
/// # Ok::<(), caexfer::core::Error>(())
/// ```
pub fn read_geometry(input: impl AsRef<[u8]>) -> Result<GeometryProjection> {
    Document::parse(input)?.geometry()
}
