//! Bounded Nastran Bulk Data geometry exchange.
//!
//! [`mesh::read`] projects supported linear geometry and reports omitted
//! solver data. [`mesh::write`] emits a geometry deck. Neither operation
//! claims to read or write a complete Nastran model.
//!

pub mod mesh;
mod model;
mod number;
mod syntax;

#[cfg(test)]
mod tests;

pub use model::GeometryProjection;

use number::parse_real;
use syntax::{Card, ParseOptions, ParsedBdf};
