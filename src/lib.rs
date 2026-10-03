//! Preserve engineering documents and explicitly project their supported geometry.
//!
//! [`bdf`] provides byte-preserving document editing. The other format modules
//! read or write the documented mesh and result subsets. Shared geometry,
//! fields, and diagnostics live in [`core`].

pub mod bdf;
pub mod core;
pub mod frd;
pub mod inp;
pub mod msh;
pub mod op2;
pub mod vtu;
