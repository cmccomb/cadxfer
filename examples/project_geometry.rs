//! Run: cargo run --example project_geometry
use caexfer::{bdf::Document, core::Result, vtu};

fn main() -> Result<()> {
    // Keep the parsed document available while projecting only its geometry.
    let doc = Document::parse("GRID,1,,0,0,0\nGRID,2,,1,0,0\nCROD,10,7,1,2\n")?;
    let projection = doc.geometry()?;

    // Surface losses before writing the projected mesh to stdout.
    for omission in &projection.omissions {
        eprintln!("{}: {}", omission.category, omission.detail);
    }
    vtu::write(&projection.mesh, std::io::stdout())?;
    Ok(())
}
