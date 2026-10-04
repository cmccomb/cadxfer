//! Opt-in native SU2 reader gate, enabled with `CAEXFER_SU2=/path/to/SU2_CFD`.

use caexfer::conversion::{self, Format, Options};
use std::fs;
use std::path::Path;
use std::process::Command;

/// Check SU2_CFD loads the named marker transferred from a Gmsh mesh.
#[test]
fn su2_loads_named_markers_from_gmsh_mesh() {
    let Ok(su2) = std::env::var("CAEXFER_SU2") else {
        return;
    };
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/su2-triangle.msh");
    let scratch = std::env::temp_dir().join(format!("caexfer-su2-{}", std::process::id()));
    fs::create_dir_all(&scratch).unwrap();
    let mesh = scratch.join("triangle.su2");
    let config = scratch.join("check.cfg");
    let mut bytes = Vec::new();
    let report =
        conversion::convert_path(&source, Format::Su2, &Options::default(), &mut bytes).unwrap();
    assert_eq!(report.cells, 6);
    fs::write(&mesh, bytes).unwrap();
    fs::write(
        &config,
        "SOLVER= EULER\nMESH_FILENAME= triangle.su2\nMESH_FORMAT= SU2\nMARKER_EULER= ( wall )\nCONV_NUM_METHOD_FLOW= ROE\nMACH_NUMBER= 0.5\nITER= 0\nOUTPUT_FILES= ( RESTART )\n",
    ).unwrap();
    let output = Command::new(su2)
        .arg("check.cfg")
        .args(["--threads", "1"])
        .current_dir(&scratch)
        .output()
        .expect("launch SU2_CFD");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "SU2 rejected mesh: {stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains("Marker = wall"));
    fs::remove_dir_all(scratch).unwrap();
}
