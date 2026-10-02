//! Feature-selected format libraries. No parser implementation lives here.
//!
//! Enable `bdf` and/or `vtu`. There is intentionally no `op2` feature yet:
//! format detection or record framing is not equivalent to decoding results.

pub use caxifer_core as core;
#[cfg(feature = "bdf")]
pub use caxifer_bdf as bdf;
#[cfg(feature = "vtu")]
pub use caxifer_vtu as vtu;
