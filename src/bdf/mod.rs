//! Source-preserving Nastran Bulk Data documents.
//!
//! Parsing indexes the original bytes. Unknown cards and comments remain in the
//! document; writing an unedited document reproduces the input byte-for-byte.
//! Semantic access and geometry projection are separate, fallible operations.
//!
//! ```
//! use caexfer::bdf::Document;
//! let original = b"$ original\r\nGRID,1,,0.,0.,0.\r\n";
//! let mut doc = Document::parse(original).unwrap();
//! assert_eq!(doc.to_bytes(), original);
//! doc.set_grid_coordinates(1, [1.0, 2.0, 3.0]).unwrap();
//! assert_eq!(doc.grids().next().unwrap().unwrap().coordinates, [1.0, 2.0, 3.0]);
//! ```

pub mod mesh;
mod model;
mod number;
mod syntax;

pub use crate::core::{Error, Result};
pub use model::{GeometryProjection, Grid, Omission};
pub use number::parse_real;
pub use syntax::{Card, Document, ParseOptions};
