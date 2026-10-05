//! Convert and validate supported engineering mesh, voxel, and result files.
//!
//! [`convert`] writes a new output file and returns a typed report of omissions
//! and assumptions. [`validate`] checks the supported source subset without
//! writing a file. Format adapters and the intermediate mesh model are internal.
//! See the [library guide] for supported routes and options.
//!
//! [library guide]: https://github.com/cmccomb/caexfer/blob/main/docs/LIBRARY.md
//!
//! ```
//! use caexfer::{Options, validate};
//! let path = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/plate.bdf");
//! let report = validate(path, &Options::default())?;
//! assert!(report.passed);
//! assert_eq!(report.points, 4);
//! # Ok::<(), caexfer::Error>(())
//! ```
#![warn(missing_docs)]

mod conversion;
mod core;
mod formats;

mod api;
mod file_output;

pub use api::{ValidationReport, convert, validate};
pub use conversion::{
    AssumptionKind, CompanionReport, ConversionReport, Format, Omission, Options, Stage,
};
pub use core::{Error, Result};
pub use formats::msh::Version as MshVersion;
