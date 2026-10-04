//! ASCII Gmsh MSH 4.1 and 2.2 linear mesh and numeric data blocks.

mod read;
mod write;

#[cfg(test)]
mod interop_tests;
#[cfg(test)]
mod tests;

pub use read::{Projection, read, read_projection};
pub use write::{Version, write, write_22, write_version};
