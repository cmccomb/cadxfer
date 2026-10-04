//! Format-aware conversion with explicit source and destination omissions.
//!
//! [`read_path`] loads a supported source; [`convert`] writes its supported
//! projection to a caller-owned stream. The returned [`ConversionReport`]
//! describes what was carried and what was omitted.

mod format;
mod read;
mod report;
mod write;

#[cfg(test)]
mod tests;

pub use format::{Format, Options};
pub use read::read_path;
pub use report::{AssumptionKind, CompanionReport, ConversionReport, Omission, ReadResult, Stage};
pub use write::convert;
