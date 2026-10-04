//! Run: `cargo run --example project_geometry`
use caexfer::core::Result;
use caexfer::formats::{bdf, vtu};

fn main() -> Result<()> {
    let projection = bdf::mesh::read("GRID,1,,0,0,0\nGRID,2,,1,0,0\nCROD,10,7,1,2\n")?;

    // Surface losses before writing the projected mesh to stdout.
    for omission in &projection.omissions {
        eprintln!("{}: {}", omission.category, omission.detail);
    }

    // Stream the projected geometry.
    vtu::write(&projection.mesh, std::io::stdout())?;
    Ok(())
}
