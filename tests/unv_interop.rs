//! Opt-in independent Gmsh reader gate, enabled with `CAEXFER_GMSH=/path/to/gmsh`.

use caexfer::{Options, convert, validate};
use std::fs;
use std::path::Path;
use std::process::Command;

/// Check Gmsh can import Rust-written UNV geometry.
#[test]
fn gmsh_imports_written_unv_geometry() {
    let Ok(gmsh) = std::env::var("CAEXFER_GMSH") else {
        return;
    };
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gmsh-six-kind.unv");
    let scratch = std::env::temp_dir().join(format!("caexfer-unv-{}", std::process::id()));
    fs::create_dir_all(&scratch).unwrap();
    let mesh = scratch.join("mesh.unv");
    let checked = scratch.join("checked.msh");
    convert(
        &source,
        &mesh,
        &Options {
            accept_all: true,
            ..Options::default()
        },
    )
    .unwrap();
    let output = Command::new(gmsh)
        .args([
            &mesh,
            Path::new("-0"),
            Path::new("-format"),
            Path::new("msh22"),
            Path::new("-o"),
            &checked,
        ])
        .output()
        .expect("launch Gmsh");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = validate(&checked, &Options::default()).unwrap();
    // Gmsh drops the fixture's unreferenced ninth point when it resaves.
    assert_eq!((report.points, report.cells), (8, 6));
    fs::remove_dir_all(scratch).unwrap();
}
