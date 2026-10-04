//! ASCII `CalculiX` FRD geometry and supported nodal result records.

mod read;
mod write;

#[cfg(test)]
mod tests;

pub use read::read;
pub use write::write;
