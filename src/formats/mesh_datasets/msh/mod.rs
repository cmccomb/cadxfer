//! ASCII Gmsh MSH 4.1 and 2.2 linear mesh and numeric data blocks.

mod read;
mod write;

#[cfg(test)]
mod interop_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
pub use read::read;
pub use read::read_projection;
pub use write::{Version, write_version};
#[cfg(test)]
pub use write::{write, write_22};
