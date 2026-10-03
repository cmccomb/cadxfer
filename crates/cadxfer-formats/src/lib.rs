//! Feature-selected format libraries. No parser implementation lives here.
//!
//! Enable `bdf` and/or `vtu`. There is intentionally no `op2` feature yet:
//! format detection or record framing is not equivalent to decoding results.

#[cfg(feature = "bdf")]
pub use cadxfer_bdf as bdf;
pub use cadxfer_core as core;
#[cfg(feature = "vtu")]
pub use cadxfer_vtu as vtu;
