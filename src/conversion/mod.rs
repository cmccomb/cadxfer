//! Format-aware conversion with explicit source and destination omissions.
//!
//! [`convert_path`] reads a supported source and writes its supported projection
//! to a caller-owned stream. The returned [`ConversionReport`] describes what
//! was carried and what was omitted. File persistence and overwrite policy
//! belong to the caller. Use [`read_path`] followed by [`convert`] when the
//! projected dataset needs inspection or modification between those steps.

mod format;
mod read;
mod report;
mod write;

pub use format::{Format, Options};
pub use read::read_path;
pub use report::{ConversionReport, Omission, ReadResult, Stage};
pub use write::{convert, convert_path};
