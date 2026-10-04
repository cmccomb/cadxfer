//! ASCII VTK XML `UnstructuredGrid` linear mesh and numeric-field I/O.
//!
//! Binary, compressed, appended, parallel and multi-piece layouts are outside
//! this bounded reader/writer.

mod read;
mod write;

#[cfg(test)]
mod tests;

pub use read::{Projection, read, read_projection};
pub use write::{write, write_data};
