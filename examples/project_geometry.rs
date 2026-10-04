//! Run: `cargo run --example project_geometry -- OUTPUT.vtu`
use caexfer::{Options, Result, convert};

fn main() -> Result<()> {
    let output = std::env::args_os()
        .nth(1)
        .ok_or_else(|| caexfer::Error::new("E_USAGE", "supply a new output .vtu path"))?;
    let report = convert(
        "examples/plate.bdf",
        &output,
        &Options {
            accept_omissions: true,
            ..Options::default()
        },
    )?;
    for omission in report.omissions {
        eprintln!("{}: {}", omission.stage.name(), omission.detail);
    }
    Ok(())
}
