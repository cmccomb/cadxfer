//! Feature-selected engineering format adapters.
//!
//! BDF and VTU are separate crates; MSH, INP, FRD and the optional
//! pyNastran-backed OP2 adapter live in this facade.

#[cfg(feature = "bdf")]
pub use caexfer_bdf as bdf;
#[cfg(feature = "bdf")]
pub mod bdf_mesh;
pub use caexfer_core as core;
#[cfg(feature = "frd")]
pub mod frd;
#[cfg(feature = "inp")]
pub mod inp;
#[cfg(feature = "msh")]
pub mod msh;
#[cfg(feature = "op2")]
pub mod op2;
#[cfg(feature = "vtu")]
pub use caexfer_vtu as vtu;
