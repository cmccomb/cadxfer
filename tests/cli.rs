//! End-to-end CLI tests for exit codes, receipts, and no-clobber output.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

// Give concurrent CLI test scratch directories distinct process-local suffixes.
static COUNTER: AtomicU64 = AtomicU64::new(0);
/// Isolated directory for one CLI test invocation.
struct Scratch(PathBuf);
impl Scratch {
    /// Create one isolated working directory with a minimal BDF mesh.
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
    /// Run the published executable from this fixture directory.
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_caexfer"))
            .args(args)
            .current_dir(&self.0)
            .output()
            .unwrap()
    }
}
impl Drop for Scratch {
    /// Remove files made by the CLI during this test.
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Show an overview and command-specific help without requiring path arguments.
#[test]
fn help_and_version_work() {
    let s = Scratch::new();
    let overview = s.run(&["--help"]);
    assert!(overview.status.success());
    let overview = String::from_utf8(overview.stdout).unwrap();
    assert!(overview.contains("caexfer COMMAND [OPTIONS]"));
    assert!(overview.contains("caexfer COMMAND --help"));

    let convert = s.run(&["convert", "--help"]);
    assert!(convert.status.success());
    let convert = String::from_utf8(convert.stdout).unwrap();
    assert!(convert.contains("convert INPUT OUTPUT --accept-projection"));
    assert!(convert.contains("--mesh-out FILE"));
    assert!(!convert.contains("caexfer formats [--json]"));
    assert_eq!(
        convert,
        String::from_utf8(s.run(&["help", "convert"]).stdout).unwrap()
    );

    let validate = s.run(&["validate", "--help"]);
    assert!(validate.status.success());
    let validate = String::from_utf8(validate.stdout).unwrap();
    assert!(validate.contains("--strict"));
    assert!(!validate.contains("--mesh-out FILE"));

    assert_eq!(
        String::from_utf8(s.run(&["--version"]).stdout)
            .unwrap()
            .trim(),
        "caexfer 0.1.0"
    );
}

/// Expose schema version and projected BDF counts in JSON info.
#[test]
fn info_json_has_version_and_scope() {
    let result = Scratch::new().run(&["info", "mesh.bdf", "--json"]);
    assert!(result.status.success());
    let out = String::from_utf8(result.stdout).unwrap();
    assert!(out.contains("\"schema_version\":1"));
    assert!(out.contains("\"points\":2"));
    assert!(out.contains("\"cells\":1"));
}

/// Reject the removed roundtrip command without creating a file.
#[test]
fn retired_roundtrip_command_is_rejected() {
    let s = Scratch::new();
    assert_eq!(
        s.run(&["roundtrip", "mesh.bdf", "copy.bdf"]).status.code(),
        Some(2)
    );
    assert!(!s.0.join("copy.bdf").exists());
}

/// Require projection acknowledgement before creating an output file.
#[test]
fn refused_conversion_does_not_create_output() {
    let s = Scratch::new();
    assert_eq!(
        s.run(&["convert", "mesh.bdf", "mesh.vtu"]).status.code(),
        Some(2)
    );
    assert!(!s.0.join("mesh.vtu").exists());
}

/// Write VTU geometry and a machine-readable omission receipt.
#[test]
fn geometry_conversion_creates_vtu_and_reports_losses() {
    let s = Scratch::new();
    let result = s.run(&[
        "convert",
        "mesh.bdf",
        "mesh.vtu",
        "--accept-projection",
        "--json",
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    assert!(
        String::from_utf8(result.stdout)
            .unwrap()
            .contains("\"omissions\"")
    );
    assert!(
        std::fs::read_to_string(s.0.join("mesh.vtu"))
            .unwrap()
            .contains("nastran_node_id")
    );
}

/// Protect an input file even when selected as the output path.
#[test]
fn source_file_is_never_overwritten() {
    let s = Scratch::new();
    let before = std::fs::read(s.0.join("mesh.bdf")).unwrap();
    assert!(
        !s.run(&["convert", "mesh.bdf", "mesh.bdf", "--accept-projection"])
            .status
            .success()
    );
    assert_eq!(before, std::fs::read(s.0.join("mesh.bdf")).unwrap());
}

/// Require a suitable companion format only on Nastran result input.
#[test]
fn companion_mesh_option_is_only_for_op2_and_needs_a_mesh_format() {
    let s = Scratch::new();
    assert_eq!(
        s.run(&[
            "convert",
            "mesh.bdf",
            "mesh.vtu",
            "--accept-projection",
            "--mesh-out",
            "extra.bdf",
        ])
        .status
        .code(),
        Some(2)
    );
    assert_eq!(
        s.run(&[
            "convert",
            "mesh.bdf",
            "mesh.op2",
            "--accept-projection",
            "--mesh-out",
            "extra.op2",
        ])
        .status
        .code(),
        Some(2)
    );
    assert!(!s.0.join("mesh.op2").exists());
    assert!(!s.0.join("extra.op2").exists());
}

/// Treat omitted BDF material semantics as a strict validation failure.
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

/// Return a structured JSON diagnostic for absent input.
#[test]
fn missing_file_json_error() {
    let result = Scratch::new().run(&["info", "absent.bdf", "--json"]);
    assert_eq!(result.status.code(), Some(1));
    assert!(
        String::from_utf8(result.stdout)
            .unwrap()
            .contains("\"code\":\"E_IO\"")
    );
}

/// Reject OP2 result rows that do not match companion GRID IDs.
#[test]
fn op2_requires_matching_mesh() {
    let result = Scratch::new().run(&[
        "convert",
        "results.op2",
        "results.vtu",
        "--accept-projection",
        "--json",
    ]);
    assert_eq!(result.status.code(), Some(2));
    assert!(
        String::from_utf8(result.stdout)
            .unwrap()
            .contains("OP2 requires --mesh")
    );
}

/// Show PCH read-only capability in JSON and human formats output.
#[test]
fn formats_advertises_read_only_pch_in_both_presentations() {
    let s = Scratch::new();
    let machine = s.run(&["formats", "--json"]);
    assert!(machine.status.success());
    let machine = String::from_utf8(machine.stdout).unwrap();
    assert!(machine.contains("\"format\":\"pch\""));
    assert!(machine.contains("\"writable\":false"));
    let human = s.run(&["formats"]);
    assert!(human.status.success());
    assert!(
        String::from_utf8(human.stdout)
            .unwrap()
            .contains("pch  ASCII real SORT1 displacement, read-only")
    );
}

/// Explain projected counts and validation scope in human CLI output.
#[test]
fn human_inspection_and_validation_report_their_scopes() {
    // BDF and INP share the projected-data CLI path, but INP also supplies
    // a source omission that strict validation must reject.
    let s = Scratch::new();
    let bdf = s.run(&["info", "mesh.bdf"]);
    assert!(bdf.status.success());
    assert!(
        String::from_utf8(bdf.stdout)
            .unwrap()
            .contains("BDF: 2 points, 1 cells, 0 fields")
    );
    let validated = s.run(&["validate", "mesh.bdf"]);
    assert!(validated.status.success());
    assert!(
        String::from_utf8(validated.stdout)
            .unwrap()
            .contains("BDF supported-subset checks passed")
    );

    std::fs::write(
        s.0.join("omitted.inp"),
        "*NODE\n1,0,0,0\n2,1,0,0\n*ELEMENT, TYPE=T3D2\n10,1,2\n*MATERIAL, NAME=STEEL\n",
    )
    .unwrap();
    let info = s.run(&["info", "omitted.inp"]);
    assert!(info.status.success());
    assert!(
        String::from_utf8(info.stdout)
            .unwrap()
            .contains("INP: 2 points")
    );
    let relaxed = s.run(&["validate", "omitted.inp"]);
    assert!(relaxed.status.success());
    let strict = s.run(&["validate", "omitted.inp", "--strict", "--json"]);
    assert_eq!(strict.status.code(), Some(1));
    assert!(
        String::from_utf8(strict.stdout)
            .unwrap()
            .contains("\"passed\":false")
    );
}

/// Print omitted data and installed companion paths for human users.
#[test]
fn human_conversion_reports_omissions_and_companion_install() {
    // Test ordinary single-file output before the paired OP2/mesh path.
    let s = Scratch::new();
    let converted = s.run(&["convert", "mesh.bdf", "mesh.vtu", "--accept-projection"]);
    assert!(converted.status.success());
    assert!(
        String::from_utf8(converted.stdout)
            .unwrap()
            .contains("Wrote mesh.vtu: 2 points")
    );
    assert!(
        String::from_utf8(converted.stderr)
            .unwrap()
            .contains("Omission:")
    );

    let op2 = s.run(&[
        "convert",
        "mesh.bdf",
        "zeros.op2",
        "--accept-projection",
        "--assume-zero-displacement",
        "--mesh-out",
        "zeros.bdf",
    ]);
    assert!(
        op2.status.success(),
        "{}",
        String::from_utf8_lossy(&op2.stderr)
    );
    assert!(s.0.join("zeros.op2").is_file());
    assert!(s.0.join("zeros.bdf").is_file());
    assert!(
        String::from_utf8(op2.stdout)
            .unwrap()
            .contains("Wrote companion mesh zeros.bdf")
    );
    assert!(
        String::from_utf8(op2.stderr)
            .unwrap()
            .contains("SYNTHETIC ASSUMPTION")
    );
}

/// Reject over-limit input and wrong format options before output exists.
#[test]
fn bounded_and_explicit_format_input_fail_before_output_creation() {
    let s = Scratch::new();
    let bounded = s.run(&[
        "convert",
        "mesh.bdf",
        "small.vtu",
        "--max-bytes",
        "2",
        "--accept-projection",
        "--json",
    ]);
    assert_eq!(bounded.status.code(), Some(1));
    assert!(!s.0.join("small.vtu").exists());
    let bad_source = s.run(&[
        "convert",
        "mesh.bdf",
        "wrong.vtu",
        "--from",
        "vtu",
        "--accept-projection",
        "--json",
    ]);
    assert_eq!(bad_source.status.code(), Some(1));
    assert!(!s.0.join("wrong.vtu").exists());
}

/// Reject conflicting or malformed flags without touching destinations.
#[test]
fn malformed_cli_options_fail_without_writing() {
    let s = Scratch::new();
    for command in [
        vec!["info", "mesh.bdf", "--step", "bad"],
        vec!["info", "mesh.bdf", "--max-bytes", "0"],
        vec!["info", "mesh.bdf", "--max-bytes", "bad"],
        vec!["info", "mesh.bdf", "--unknown"],
        vec!["info", "mesh.bdf", "--from"],
        vec!["info", "mesh.bdf", "--assume-basic-frame"],
        vec!["info", "mesh.bdf", "--mesh", "mesh.bdf"],
        vec!["formats", "--mesh", "mesh.bdf"],
        vec![
            "convert",
            "mesh.bdf",
            "bad.vtu",
            "--accept-projection",
            "--zero-missing-rotations",
        ],
        vec![
            "convert",
            "mesh.bdf",
            "bad.vtu",
            "--accept-projection",
            "--assume-zero-displacement",
        ],
    ] {
        let result = s.run(&command);
        assert_eq!(result.status.code(), Some(2), "{command:?}");
    }
    let unknown_format = s.run(&["info", "mesh.bdf", "--from", "bogus", "--json"]);
    assert_eq!(unknown_format.status.code(), Some(1));
    assert!(
        String::from_utf8(unknown_format.stdout)
            .unwrap()
            .contains("\"code\":\"E_FORMAT\"")
    );
    assert!(!s.0.join("bad.vtu").exists());
}

/// Constrain synthetic OP2 to supported source and basic output frames.
#[test]
fn synthetic_result_rejects_nonbasic_output_frames_and_mesh_sources() {
    let s = Scratch::new();
    std::fs::write(
        s.0.join("nonbasic.bdf"),
        "GRID,1,,0,0,0,42\nGRID,2,,1,0,0\nCROD,1,7,1,2\n",
    )
    .unwrap();
    let result = s.run(&[
        "convert",
        "nonbasic.bdf",
        "bad.op2",
        "--accept-projection",
        "--assume-zero-displacement",
        "--json",
    ]);
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8(result.stdout).unwrap().contains("E_OP2"));
    assert!(!s.0.join("bad.op2").exists());

    let converted = s.run(&["convert", "mesh.bdf", "mesh.vtu", "--accept-projection"]);
    assert!(converted.status.success());
    let result = s.run(&[
        "convert",
        "mesh.vtu",
        "bad.op2",
        "--accept-projection",
        "--assume-zero-displacement",
        "--json",
    ]);
    assert_eq!(result.status.code(), Some(2));
    assert!(!s.0.join("bad.op2").exists());
}

/// Surface BDF geometry errors and entry help in human CLI output.
#[test]
fn entry_help_and_bdf_diagnostics_are_visible_to_human_users() {
    let s = Scratch::new();
    assert!(s.run(&[]).status.success());
    assert!(s.run(&["info", "mesh.bdf", "--help"]).status.success());
    assert_eq!(
        s.run(&["info", "mesh.bdf", "--accept-projection"])
            .status
            .code(),
        Some(2)
    );
    std::fs::write(s.0.join("invalid.bdf"), "GRID,1,42,0,0,0\n").unwrap();
    let info = s.run(&["info", "invalid.bdf"]);
    assert_eq!(info.status.code(), Some(1));
    assert!(
        String::from_utf8(info.stderr)
            .unwrap()
            .contains("E_COORDINATE_SYSTEM")
    );
}

/// Stage a synthetic OP2 result with an explicitly selected MSH 2.2 mesh.
#[test]
fn synthetic_op2_can_stage_an_explicit_msh22_companion() {
    let s = Scratch::new();
    let result = s.run(&[
        "convert",
        "mesh.bdf",
        "zeros.op2",
        "--accept-projection",
        "--assume-zero-displacement",
        "--mesh-out",
        "companion.msh",
        "--msh-version",
        "2.2",
        "--json",
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    assert!(
        String::from_utf8(result.stdout)
            .unwrap()
            .contains("\"format\":\"msh\"")
    );
    assert!(
        std::fs::read_to_string(s.0.join("companion.msh"))
            .unwrap()
            .starts_with("$MeshFormat\n2.2 0 8")
    );
}
