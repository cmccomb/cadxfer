//! File-level conversion and validation shared by the library and CLI.

mod convert;
mod validate;

pub use convert::convert;
pub use validate::{ValidationReport, validate};
