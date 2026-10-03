//! Run: cargo run -p cadxfer-formats --features bdf,vtu --example project_geometry
use cadxfer_formats::{bdf::Document, vtu};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let doc = Document::parse("GRID,1,,0,0,0\nGRID,2,,1,0,0\nCROD,10,7,1,2\n")?;
    let projection = doc.geometry()?;
    for omission in &projection.omissions {
        eprintln!("{}: {}", omission.category, omission.detail);
    }
    vtu::write(&projection.mesh, std::io::stdout())?;
    Ok(())
}
