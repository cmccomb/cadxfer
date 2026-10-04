//! Bounded ASCII legacy VTK `UNSTRUCTURED_GRID` mesh and numeric fields.
//!
//! This adapter writes version 2.0 syntax. Binary files, other dataset types,
//! lookup tables, and nonlinear cells fail explicitly.

mod read;
mod write;

#[cfg(test)]
mod tests;

#[cfg(test)]
pub use read::read;
pub use read::read_projection;
pub use write::write_data;
