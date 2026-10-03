use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "caexfer-cli-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        std::fs::write(
            path.join("mesh.bdf"),
            b"GRID,1,,0.,0.,0.\nGRID,2,,1.,0.,0.\nCROD,1,7,1,2\n",
        )
        .unwrap();
        Self(path)
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_caexfer"))
            .args(args)
            .current_dir(&self.0)
            .output()
            .unwrap()
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn help_and_version_work() {
    let s = Scratch::new();
    assert!(s.run(&["--help"]).status.success());
    assert_eq!(
        String::from_utf8(s.run(&["--version"]).stdout)
            .unwrap()
            .trim(),
        "caexfer 0.1.0"
    );
}

#[test]
fn info_json_has_version_and_scope() {
    let result = Scratch::new().run(&["info", "mesh.bdf", "--json"]);
    assert!(result.status.success());
    let out = String::from_utf8(result.stdout).unwrap();
    assert!(out.contains("\"schema_version\":1"));
    assert!(out.contains("\"full_solver_validation\":false"));
}

#[test]
fn roundtrip_really_matches_original() {
    let s = Scratch::new();
    assert!(s
        .run(&["roundtrip", "mesh.bdf", "copy.bdf"])
        .status
        .success());
    assert_eq!(
        std::fs::read(s.0.join("mesh.bdf")).unwrap(),
        std::fs::read(s.0.join("copy.bdf")).unwrap()
    );
}

#[test]
fn refused_conversion_does_not_create_output() {
    let s = Scratch::new();
    assert_eq!(
        s.run(&["convert", "mesh.bdf", "mesh.vtu"]).status.code(),
        Some(2)
    );
    assert!(!s.0.join("mesh.vtu").exists());
}

#[test]
fn geometry_conversion_creates_vtu_and_reports_losses() {
    let s = Scratch::new();
    let result = s.run(&[
        "convert",
        "mesh.bdf",
        "mesh.vtu",
        "--geometry-only",
        "--json",
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    assert!(String::from_utf8(result.stdout)
        .unwrap()
        .contains("\"omissions\""));
    assert!(std::fs::read_to_string(s.0.join("mesh.vtu"))
        .unwrap()
        .contains("nastran_node_id"));
}

#[test]
fn source_file_is_never_overwritten() {
    let s = Scratch::new();
    let before = std::fs::read(s.0.join("mesh.bdf")).unwrap();
    assert!(!s
        .run(&["roundtrip", "mesh.bdf", "mesh.bdf"])
        .status
        .success());
    assert_eq!(before, std::fs::read(s.0.join("mesh.bdf")).unwrap());
}

#[test]
fn strict_validation_fails_on_opaque_material() {
    let s = Scratch::new();
    std::fs::write(s.0.join("opaque.bdf"), "GRID,1\nMAT1,7,2.1+11\n").unwrap();
    assert!(s.run(&["validate", "opaque.bdf"]).status.success());
    assert_eq!(
        s.run(&["validate", "opaque.bdf", "--strict"]).status.code(),
        Some(1)
    );
}

#[test]
fn missing_file_json_error() {
    let result = Scratch::new().run(&["info", "absent.bdf", "--json"]);
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8(result.stdout)
        .unwrap()
        .contains("\"code\":\"E_IO\""));
}

#[test]
fn unsupported_result_format_is_honest() {
    let result = Scratch::new().run(&["info", "results.op2", "--json"]);
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8(result.stdout)
        .unwrap()
        .contains("E_FORMAT"));
}

#[test]
fn grid_edit_changes_only_requested_coordinates() {
    let s = Scratch::new();
    assert!(s
        .run(&[
            "set-grid",
            "mesh.bdf",
            "edited.bdf",
            "--id",
            "1",
            "--xyz",
            "-1",
            "2",
            "3"
        ])
        .status
        .success());
    let text = std::fs::read_to_string(s.0.join("edited.bdf")).unwrap();
    assert!(text.starts_with("GRID,1,,-1.,2.,3.\n"));
    assert!(text.contains("GRID,2,,1.,0.,0.\nCROD,1,7,1,2\n"));
}
