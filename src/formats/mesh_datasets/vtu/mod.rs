//! ASCII VTK XML `UnstructuredGrid` linear mesh and numeric-field I/O.
//!
//! Binary, compressed, appended, parallel and multi-piece layouts are outside
//! this bounded reader/writer.

mod read;
mod write;

#[cfg(test)]
mod tests;

#[cfg(test)]
pub use read::read;
pub use read::read_projection;
#[cfg(test)]
pub use write::write;
pub use write::write_data;
