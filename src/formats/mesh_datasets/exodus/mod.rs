//! Pure Rust adapter for the NetCDF-3 classic subset of Exodus II.
//!
//! Exodus blocks partition the native element order. Their identifiers are
//! reported, not reinterpreted as solver property IDs. Node and element maps
//! carry original IDs. Complete scalar nodal and element variables become
//! numeric fields at selected time steps.

mod read;
mod write;

#[cfg(test)]
mod tests;

pub use read::{Projection, read_projection};
pub use write::write_data;
