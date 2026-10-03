//! Source-preserving Nastran Bulk Data documents.
//!
//! Parsing indexes the original bytes. Unknown cards and comments remain in the
//! document; writing it reproduces the input byte-for-byte.
//! Semantic access and geometry projection are separate, fallible operations.
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

pub mod mesh;
mod model;
mod number;
mod syntax;

pub use crate::core::{Error, Result};
pub use model::{GeometryProjection, Grid, Omission};
pub use number::parse_real;
pub use syntax::{Card, Document, ParseOptions};
