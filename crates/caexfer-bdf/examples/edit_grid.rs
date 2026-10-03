//! Run: cargo run -p caexfer-bdf --example edit_grid
use caexfer_bdf::Document;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = b"$ retain this exact comment\r\nGRID,42,,0.,0.,0.\r\n";
    let mut document = Document::parse(source)?;
    document.set_grid_coordinates(42, [1.0, 2.0, 3.0])?;
    document.write_to(std::io::stdout())?;
    Ok(())
}
