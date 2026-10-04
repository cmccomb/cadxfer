//! Exercise the two public file operations from an external crate.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use caexfer::{MshVersion, Options, convert, validate};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "caexfer-api-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(name)
}

#[test]
fn validation_matches_cli_strictness_and_counts() {
    let source = fixture("examples/plate.bdf");
    let ordinary = validate(&source, &Options::default()).unwrap();
    assert!(ordinary.passed);
    assert_eq!(
        (ordinary.points, ordinary.cells, ordinary.fields),
        (4, 1, 0)
    );
    assert_ne!(ordinary.omissions.len(), 0);
    let strict = validate(
        &source,
        &Options {
            strict: true,
            ..Options::default()
        },
    )
    .unwrap();
    assert!(!strict.passed);
    assert_eq!(strict.omissions, ordinary.omissions);
}

#[test]
fn conversion_requires_acceptance_and_never_overwrites() {
    let scratch = Scratch::new();
    let output = scratch.path("model.vtu");
    let source = fixture("examples/plate.bdf");
    let error = convert(&source, &output, &Options::default()).unwrap_err();
    assert_eq!(error.code, "E_USAGE");
    assert!(error.message.contains("accept_omissions"));
    assert!(!output.exists());
    let report = convert(
        &source,
        &output,
        &Options {
            accept_omissions: true,
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!((report.points, report.cells), (4, 1));
    assert_ne!(report.omissions.len(), 0);
    assert!(
        std::fs::read_to_string(&output)
            .unwrap()
            .contains("<VTKFile")
    );
    assert_eq!(
        convert(
            &source,
            &output,
            &Options {
                accept_omissions: true,
                ..Options::default()
            }
        )
        .unwrap_err()
        .code,
        "E_EXISTS"
    );
}

#[test]
fn op2_companion_is_staged_and_reported() {
    let scratch = Scratch::new();
    let output = scratch.path("results.op2");
    let mesh = scratch.path("mesh.bdf");
    let options = Options {
        mesh_output: Some(mesh.clone()),
        accept_all: true,
        ..Options::default()
    };
    let report = convert(fixture("examples/plate.bdf"), &output, &options).unwrap();
    let companion = report.mesh_output.unwrap();
    assert_eq!(companion.path, mesh);
    assert!(output.exists());
    assert!(companion.path.exists());
    let reread = validate(
        &output,
        &Options {
            mesh: Some(companion.path),
            ..Options::default()
        },
    )
    .unwrap();
    assert!(reread.passed);
    assert_eq!(reread.points, 4);
}

#[test]
fn companion_output_requires_op2_destination() {
    let scratch = Scratch::new();
    let output = scratch.path("result.vtu");
    let companion = scratch.path("mesh.bdf");
    let error = convert(
        fixture("examples/plate.bdf"),
        &output,
        &Options {
            mesh_output: Some(companion.clone()),
            accept_all: true,
            ..Options::default()
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "E_USAGE");
    assert!(!output.exists() && !companion.exists());
}

#[test]
fn op2_companion_requires_a_mesh_bearing_format() {
    let scratch = Scratch::new();
    let output = scratch.path("results.op2");
    let companion = scratch.path("companion.op2");
    let error = convert(
        fixture("examples/plate.bdf"),
        &output,
        &Options {
            mesh_output: Some(companion.clone()),
            accept_all: true,
            ..Options::default()
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "E_USAGE");
    assert!(!output.exists() && !companion.exists());
}

#[test]
fn documented_frd_msh_and_pch_routes_use_only_the_root_api() {
    let scratch = Scratch::new();
    let vtu = scratch.path("results.vtu");
    let msh = scratch.path("results.msh");
    let pch_vtu = scratch.path("displacements.vtu");
    let accepted = Options {
        accept_omissions: true,
        ..Options::default()
    };
    convert(
        fixture("tests/fixtures/linear-results.frd"),
        &vtu,
        &accepted,
    )
    .unwrap();
    convert(
        &vtu,
        &msh,
        &Options {
            msh_version: Some(MshVersion::V2_2),
            ..accepted.clone()
        },
    )
    .unwrap();
    let msh_text = std::fs::read_to_string(&msh).unwrap();
    assert!(msh_text.contains("2.2 0 8"));
    let report = convert(
        fixture("tests/fixtures/pch-multiple.pch"),
        &pch_vtu,
        &Options {
            mesh: Some(fixture("tests/fixtures/pch-companion.bdf")),
            subcase: Some(1),
            ..accepted
        },
    )
    .unwrap();
    assert_eq!(report.fields, 1);
}
