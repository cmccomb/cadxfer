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
    assert!(overview.contains("-f, --formats"));
    assert!(!overview.contains("  formats   "));
    assert!(!overview.contains("  info      "));
    assert_eq!(s.run(&["info", "mesh.bdf"]).status.code(), Some(2));

    let convert = s.run(&["convert", "--help"]);
    assert!(convert.status.success());
    let convert = String::from_utf8(convert.stdout).unwrap();
    assert!(convert.contains("convert INPUT OUTPUT [OPTIONS]"));
    assert!(convert.contains("--accept-omissions"));
    assert!(convert.contains("--accept-all"));
    assert!(convert.contains("--overwrite"));
    assert!(convert.contains("-j, --json"));
    assert!(!convert.contains("Treat following arguments as paths"));
    assert!(convert.contains("--mesh-out FILE"));
    assert!(convert.contains("caexfer --formats"));
    assert_eq!(
        convert,
        String::from_utf8(s.run(&["help", "convert"]).stdout).unwrap()
    );
    let bare_convert = s.run(&["convert"]);
    assert!(bare_convert.status.success());
    assert_eq!(convert, String::from_utf8(bare_convert.stdout).unwrap());
    assert_eq!(bare_convert.stderr, b"");

    let validate = s.run(&["validate", "--help"]);
    assert!(validate.status.success());
    let validate = String::from_utf8(validate.stdout).unwrap();
    assert!(validate.contains("-s, --strict"));
    assert!(validate.contains("-j, --json"));
    assert!(!validate.contains("--accept-basic-frame"));
    assert!(!validate.contains("--mesh-out FILE"));
    let bare_validate = s.run(&["validate"]);
    assert!(bare_validate.status.success());
    assert_eq!(validate, String::from_utf8(bare_validate.stdout).unwrap());
    assert_eq!(bare_validate.stderr, b"");
    assert_eq!(s.run(&["convert", "mesh.bdf"]).status.code(), Some(2));
    assert_eq!(s.run(&["validate", "--strict"]).status.code(), Some(2));

    assert_eq!(
        String::from_utf8(s.run(&["--version"]).stdout)
            .unwrap()
            .trim(),
        format!("caexfer {}", env!("CARGO_PKG_VERSION"))
    );
}

/// Expose schema version and projected BDF counts in JSON validation.
#[test]
fn validation_json_has_version_and_counts() {
    let result = Scratch::new().run(&["validate", "mesh.bdf", "-j"]);
    assert!(result.status.success());
    let out = String::from_utf8(result.stdout).unwrap();
    assert!(out.contains("\"schema_version\":1"));
    assert!(out.contains("\"points\":2"));
    assert!(out.contains("\"cells\":1"));
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
    assert_eq!(std::fs::read_dir(&s.0).unwrap().count(), 1);
}

/// Write VTU geometry and a machine-readable omission receipt.
#[test]
fn geometry_conversion_creates_vtu_and_reports_losses() {
    let s = Scratch::new();
    let result = s.run(&["convert", "mesh.bdf", "mesh.vtu", "--accept-all", "-j"]);
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
    assert_eq!(std::fs::read_dir(&s.0).unwrap().count(), 2);
}

/// Protect an input file even when selected as the output path.
#[test]
fn source_file_is_never_overwritten() {
    let s = Scratch::new();
    let before = std::fs::read(s.0.join("mesh.bdf")).unwrap();
    assert!(
        !s.run(&["convert", "mesh.bdf", "mesh.bdf", "--accept-all"])
            .status
            .success()
    );
    assert_eq!(before, std::fs::read(s.0.join("mesh.bdf")).unwrap());
}

/// Existing output requires explicit replacement, which still requires loss approval.
#[test]
fn overwrite_replaces_only_after_acceptance() {
    let s = Scratch::new();
    let destination = s.0.join("mesh.vtu");
    std::fs::write(&destination, b"previous output").unwrap();

    assert_eq!(
        s.run(&["convert", "mesh.bdf", "mesh.vtu", "--accept-all"])
            .status
            .code(),
        Some(1)
    );
    assert_eq!(std::fs::read(&destination).unwrap(), b"previous output");

    let unapproved = s.run(&["convert", "mesh.bdf", "mesh.vtu", "--overwrite"]);
    assert_eq!(unapproved.status.code(), Some(2));
    assert_eq!(std::fs::read(&destination).unwrap(), b"previous output");

    let invalid = s.run(&[
        "convert",
        "missing.bdf",
        "mesh.vtu",
        "--overwrite",
        "--accept-all",
    ]);
    assert_eq!(invalid.status.code(), Some(1));
    assert_eq!(std::fs::read(&destination).unwrap(), b"previous output");

    let replaced = s.run(&[
        "convert",
        "mesh.bdf",
        "mesh.vtu",
        "--overwrite",
        "--accept-all",
    ]);
    assert!(
        replaced.status.success(),
        "{}",
        String::from_utf8_lossy(&replaced.stderr)
    );
    assert!(
        std::fs::read_to_string(&destination)
            .unwrap()
            .contains("nastran_node_id")
    );
    assert_eq!(std::fs::read_dir(&s.0).unwrap().count(), 2);

    let fresh = s.run(&[
        "convert",
        "mesh.bdf",
        "fresh.vtu",
        "--overwrite",
        "--accept-all",
    ]);
    assert!(fresh.status.success());
    assert!(s.0.join("fresh.vtu").is_file());
}

/// Replacement never consumes the source or a nonregular destination.
#[test]
fn overwrite_rejects_source_and_nonfile_destinations() {
    let s = Scratch::new();
    let before = std::fs::read(s.0.join("mesh.bdf")).unwrap();
    assert_eq!(
        s.run(&[
            "convert",
            "mesh.bdf",
            "mesh.bdf",
            "--overwrite",
            "--accept-all",
        ])
        .status
        .code(),
        Some(2)
    );
    assert_eq!(std::fs::read(s.0.join("mesh.bdf")).unwrap(), before);

    std::fs::create_dir(s.0.join("directory.vtu")).unwrap();
    assert_eq!(
        s.run(&[
            "convert",
            "mesh.bdf",
            "directory.vtu",
            "--overwrite",
            "--accept-all",
        ])
        .status
        .code(),
        Some(2)
    );
    assert!(s.0.join("directory.vtu").is_dir());
}

/// Resolve path aliases and hard links before replacing a destination.
#[cfg(unix)]
#[test]
fn overwrite_rejects_source_aliases_and_symlinks() {
    use std::os::unix::fs::symlink;

    let s = Scratch::new();
    let before = std::fs::read(s.0.join("mesh.bdf")).unwrap();
    std::fs::hard_link(s.0.join("mesh.bdf"), s.0.join("linked.bdf")).unwrap();
    assert_eq!(
        s.run(&[
            "convert",
            "mesh.bdf",
            "linked.bdf",
            "--overwrite",
            "--accept-all",
        ])
        .status
        .code(),
        Some(2)
    );
    assert_eq!(std::fs::read(s.0.join("linked.bdf")).unwrap(), before);

    std::fs::write(s.0.join("unrelated.vtu"), b"unrelated output").unwrap();
    symlink("unrelated.vtu", s.0.join("linked-symlink.vtu")).unwrap();
    assert_eq!(
        s.run(&[
            "convert",
            "mesh.bdf",
            "linked-symlink.vtu",
            "--overwrite",
            "--accept-all",
        ])
        .status
        .code(),
        Some(2)
    );
    assert_eq!(std::fs::read(s.0.join("mesh.bdf")).unwrap(), before);
    assert_eq!(
        std::fs::read(s.0.join("unrelated.vtu")).unwrap(),
        b"unrelated output"
    );
}

/// Replace both paired outputs only after explicit overwrite and acceptance.
#[test]
fn overwrite_replaces_both_paired_outputs_after_acceptance() {
    let s = Scratch::new();
    assert_eq!(
        s.run(&["validate", "mesh.bdf", "--overwrite"])
            .status
            .code(),
        Some(2)
    );
    let first = s.0.join("zeros.op2");
    let second = s.0.join("zeros.bdf");
    std::fs::write(&first, b"old results").unwrap();
    std::fs::write(&second, b"old mesh").unwrap();
    let no_overwrite = s.run(&[
        "convert",
        "mesh.bdf",
        "zeros.op2",
        "--mesh-out",
        "zeros.bdf",
        "--accept-all",
    ]);
    assert_eq!(no_overwrite.status.code(), Some(1));
    assert_eq!(std::fs::read(&first).unwrap(), b"old results");
    assert_eq!(std::fs::read(&second).unwrap(), b"old mesh");
    let unapproved = s.run(&[
        "convert",
        "mesh.bdf",
        "zeros.op2",
        "--mesh-out",
        "zeros.bdf",
        "--overwrite",
    ]);
    assert_eq!(unapproved.status.code(), Some(2));
    assert_eq!(std::fs::read(&first).unwrap(), b"old results");
    assert_eq!(std::fs::read(&second).unwrap(), b"old mesh");
    assert_eq!(
        s.run(&[
            "convert",
            "mesh.bdf",
            "zeros.op2",
            "--mesh-out",
            "zeros.bdf",
            "--overwrite",
            "--accept-all",
        ])
        .status
        .code(),
        Some(0)
    );
    assert_ne!(std::fs::read(&first).unwrap(), b"old results");
    assert!(
        std::fs::read_to_string(&second)
            .unwrap()
            .contains("BEGIN BULK")
    );
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
            "--accept-all",
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
            "--accept-all",
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
    let result = Scratch::new().run(&["validate", "absent.bdf", "--json"]);
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
        "--accept-all",
        "--json",
    ]);
    assert_eq!(result.status.code(), Some(2));
    assert!(
        String::from_utf8(result.stdout)
            .unwrap()
            .contains("OP2 requires --mesh")
    );
}

/// Show the PCH read-only capability in the global format listing.
#[test]
fn formats_advertises_read_only_pch() {
    let s = Scratch::new();
    let human = s.run(&["-f"]);
    assert!(human.status.success());
    let listing = String::from_utf8(human.stdout).unwrap();
    assert!(listing.contains("vti  ASCII XML ImageData occupancy, read/write"));
    assert!(listing.contains("vox  MagicaVoxel 150 single-model occupancy, read/write"));
    assert!(listing.contains("pch  ASCII real SORT1 displacement, read-only"));
    let unsupported_json = s.run(&["--formats", "--json"]);
    assert_eq!(unsupported_json.status.code(), Some(2));
    assert_eq!(unsupported_json.stdout, b"");
    assert!(String::from_utf8_lossy(&unsupported_json.stderr).contains("E_USAGE"));
}

/// Report projected counts and omissions in human validation output.
#[test]
fn human_validation_reports_counts_and_omissions() {
    // BDF and INP share the projected-data CLI path, but INP also supplies
    // a source omission that strict validation must reject.
    let s = Scratch::new();
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
    let relaxed = s.run(&["validate", "omitted.inp"]);
    assert!(relaxed.status.success());
    let relaxed = String::from_utf8(relaxed.stdout).unwrap();
    assert!(relaxed.contains("INP supported-subset checks passed: 2 points"));
    assert!(relaxed.contains("Omission:"));
    let strict = s.run(&["validate", "omitted.inp", "-s", "-j"]);
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
    let converted = s.run(&["convert", "mesh.bdf", "mesh.vtu", "--accept-all"]);
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
        "--accept-all",
        "--accept-synthetic-zero",
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

/// An unattended conversion lists missing acceptance and installs no output.
#[test]
fn unattended_conversion_needs_only_its_reported_acceptance() {
    let s = Scratch::new();
    let waiting = s.run(&["convert", "mesh.bdf", "mesh.vtu"]);
    assert_eq!(waiting.status.code(), Some(2));
    let warning = String::from_utf8(waiting.stderr).unwrap();
    assert!(warning.contains("1. "));
    assert!(warning.contains("[--accept-omissions]"));
    assert!(!s.0.join("mesh.vtu").exists());
    assert_eq!(std::fs::read_dir(&s.0).unwrap().count(), 1);

    let accepted = s.run(&["convert", "mesh.bdf", "mesh.vtu", "--accept-omissions"]);
    assert!(accepted.status.success());
    assert!(s.0.join("mesh.vtu").exists());
}

/// Synthetic results require their own consent after ordinary losses are accepted.
#[test]
fn synthetic_zero_needs_specific_acceptance_or_catchall() {
    let s = Scratch::new();
    let waiting = s.run(&[
        "convert",
        "mesh.bdf",
        "zeros.op2",
        "--accept-omissions",
        "--json",
    ]);
    assert_eq!(waiting.status.code(), Some(2));
    let message = String::from_utf8(waiting.stdout).unwrap();
    assert!(message.contains("--accept-synthetic-zero"));
    assert!(message.contains("SYNTHETIC ASSUMPTION"));
    assert!(!s.0.join("zeros.op2").exists());

    let accepted = s.run(&[
        "convert",
        "mesh.bdf",
        "zeros.op2",
        "--accept-omissions",
        "--accept-synthetic-zero",
        "--json",
    ]);
    assert!(
        accepted.status.success(),
        "{}",
        String::from_utf8_lossy(&accepted.stdout)
    );
    assert!(
        String::from_utf8(accepted.stdout)
            .unwrap()
            .contains("\"acceptance_flag\":\"--accept-synthetic-zero\"")
    );
    assert!(s.0.join("zeros.op2").exists());

    let catchall = s.run(&["convert", "mesh.bdf", "more-zeros.op2", "--accept-all"]);
    assert!(catchall.status.success());
}

/// A refused OP2 plus companion conversion installs neither staged output.
#[test]
fn paired_output_waits_for_acceptance_before_install() {
    let s = Scratch::new();
    let waiting = s.run(&[
        "convert",
        "mesh.bdf",
        "zeros.op2",
        "--mesh-out",
        "zeros.msh",
    ]);
    assert_eq!(waiting.status.code(), Some(2));
    let warning = String::from_utf8(waiting.stderr).unwrap();
    assert!(warning.contains("--accept-synthetic-zero"));
    assert!(warning.contains("--accept-omissions"));
    assert!(!s.0.join("zeros.op2").exists());
    assert!(!s.0.join("zeros.msh").exists());
    assert_eq!(std::fs::read_dir(&s.0).unwrap().count(), 1);
}

/// Report zero rotation infill separately from omitted source fields.
#[test]
fn three_component_results_name_zero_rotation_flag() {
    let s = Scratch::new();
    std::fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/linear-results.frd"),
        s.0.join("results.frd"),
    )
    .unwrap();
    let waiting = s.run(&[
        "convert",
        "results.frd",
        "results.op2",
        "--accept-omissions",
    ]);
    assert_eq!(waiting.status.code(), Some(2));
    assert!(
        String::from_utf8(waiting.stderr)
            .unwrap()
            .contains("[--accept-zero-rotations]")
    );
    assert!(!s.0.join("results.op2").exists());

    let accepted = s.run(&[
        "convert",
        "results.frd",
        "results.op2",
        "--accept-omissions",
        "--accept-zero-rotations",
    ]);
    assert!(
        accepted.status.success(),
        "{}",
        String::from_utf8_lossy(&accepted.stderr)
    );
}

/// A non-BDF companion requires its own basic-frame assertion.
#[test]
fn non_bdf_result_companion_requires_conversion_acceptance() {
    let s = Scratch::new();
    for args in [
        vec!["convert", "mesh.bdf", "zeros.op2", "--accept-all"],
        vec!["convert", "mesh.bdf", "mesh.vtu", "--accept-all"],
    ] {
        let result = s.run(&args);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let inspected = s.run(&["validate", "zeros.op2", "--mesh", "mesh.vtu", "-j"]);
    assert!(inspected.status.success());
    let receipt = String::from_utf8(inspected.stdout).unwrap();
    assert!(receipt.contains("\"passed\":true"));
    assert!(receipt.contains("basic-frame"));
    let strict = s.run(&["validate", "zeros.op2", "--mesh", "mesh.vtu", "-s"]);
    assert_eq!(strict.status.code(), Some(1));
    assert_eq!(
        s.run(&[
            "validate",
            "zeros.op2",
            "--mesh",
            "mesh.vtu",
            "--accept-basic-frame",
        ])
        .status
        .code(),
        Some(2)
    );
    let waiting = s.run(&[
        "convert",
        "zeros.op2",
        "results.vtu",
        "--mesh",
        "mesh.vtu",
        "--accept-omissions",
        "--accept-synthetic-zero",
    ]);
    assert_eq!(waiting.status.code(), Some(2));
    assert!(
        String::from_utf8(waiting.stderr)
            .unwrap()
            .contains("[--accept-basic-frame]")
    );
    assert!(!s.0.join("results.vtu").exists());

    let accepted = s.run(&[
        "convert",
        "zeros.op2",
        "results.vtu",
        "--mesh",
        "mesh.vtu",
        "--accept-omissions",
        "--accept-synthetic-zero",
        "--accept-basic-frame",
    ]);
    assert!(
        accepted.status.success(),
        "{}",
        String::from_utf8_lossy(&accepted.stderr)
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
        "--accept-all",
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
        "--accept-all",
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
        vec!["validate", "mesh.bdf", "--step", "bad"],
        vec!["validate", "mesh.bdf", "--max-bytes", "0"],
        vec!["validate", "mesh.bdf", "--max-bytes", "bad"],
        vec!["validate", "mesh.bdf", "--unknown"],
        vec!["validate", "mesh.bdf", "--from"],
        vec!["validate", "mesh.bdf", "--accept-basic-frame"],
        vec!["validate", "mesh.bdf", "--mesh", "mesh.bdf"],
        vec!["--formats", "--mesh", "mesh.bdf"],
        vec!["--formats", "--json"],
        vec![
            "convert",
            "mesh.bdf",
            "bad.vtu",
            "--accept-all",
            "--accept-zero-rotations",
        ],
        vec![
            "convert",
            "mesh.bdf",
            "bad.vtu",
            "--accept-all",
            "--accept-synthetic-zero",
        ],
    ] {
        let result = s.run(&command);
        assert_eq!(result.status.code(), Some(2), "{command:?}");
    }
    let unknown_format = s.run(&["validate", "mesh.bdf", "--from", "bogus", "--json"]);
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
        "--accept-all",
        "--accept-synthetic-zero",
        "--json",
    ]);
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8(result.stdout).unwrap().contains("E_OP2"));
    assert!(!s.0.join("bad.op2").exists());

    let converted = s.run(&["convert", "mesh.bdf", "mesh.vtu", "--accept-all"]);
    assert!(converted.status.success());
    let result = s.run(&[
        "convert",
        "mesh.vtu",
        "bad.op2",
        "--accept-all",
        "--accept-synthetic-zero",
        "--json",
    ]);
    assert_eq!(result.status.code(), Some(1));
    assert!(!s.0.join("bad.op2").exists());
}

/// Surface BDF geometry errors and entry help in human CLI output.
#[test]
fn entry_help_and_bdf_diagnostics_are_visible_to_human_users() {
    let s = Scratch::new();
    assert!(s.run(&[]).status.success());
    assert!(s.run(&["validate", "mesh.bdf", "--help"]).status.success());
    assert_eq!(
        s.run(&["validate", "mesh.bdf", "--accept-all"])
            .status
            .code(),
        Some(2)
    );
    std::fs::write(s.0.join("invalid.bdf"), "GRID,1,42,0,0,0\n").unwrap();
    let validated = s.run(&["validate", "invalid.bdf"]);
    assert_eq!(validated.status.code(), Some(1));
    assert!(
        String::from_utf8(validated.stderr)
            .unwrap()
            .contains("E_COORDINATE_SYSTEM")
    );
}

/// A nonbasic GRID cannot be accepted as an omission or installed as false geometry.
#[test]
fn nonbasic_frame_is_a_blocking_source_error_even_with_accept_all() {
    let s = Scratch::new();
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/nonbasic-frame.bdf");
    for flags in [vec![], vec!["--accept-all"]] {
        let mut args = vec!["convert", fixture, "nonbasic.stl"];
        args.extend(flags);
        let result = s.run(&args);
        assert_eq!(result.status.code(), Some(1));
        let diagnostic = String::from_utf8(result.stderr).unwrap();
        assert!(diagnostic.contains("E_COORDINATE_SYSTEM at line 2"));
        assert!(diagnostic.contains("approval flags cannot bypass this error"));
        assert!(!s.0.join("nonbasic.stl").exists());
    }
}

/// Stage a synthetic OP2 result with an explicitly selected MSH 2.2 mesh.
#[test]
fn synthetic_op2_can_stage_an_explicit_msh22_companion() {
    let s = Scratch::new();
    let result = s.run(&[
        "convert",
        "mesh.bdf",
        "zeros.op2",
        "--accept-all",
        "--accept-synthetic-zero",
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
