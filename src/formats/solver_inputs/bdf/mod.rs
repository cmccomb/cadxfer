//! Bounded Nastran Bulk Data geometry exchange.
//!
//! [`mesh::read`] projects supported linear geometry and reports omitted
//! solver data. [`mesh::write`] emits a geometry deck. Neither operation
//! claims to read or write a complete Nastran model.
//!
//! ```
//! use caexfer::formats::bdf;
//! let source = b"GRID,1,,0.,0.,0.\n";
//! assert_eq!(bdf::mesh::read(source).unwrap().mesh.points.len(), 1);
//! ```

pub mod mesh;
mod model;
mod number;
mod syntax;

pub use model::{GeometryProjection, Omission};

use number::parse_real;
use syntax::{Card, ParseOptions, ParsedBdf};
