//! Native 32-bit OP2 real displacement adapter.
//! A matching mesh with original node IDs is required. Coordinates and
//! displacements must be in the basic frame; unsupported tables fail explicitly.

mod binary;
mod displacement;

pub use displacement::{read_displacements, write_displacements};
