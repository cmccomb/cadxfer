//! Public file API checks for voxel inputs, outputs, and rejected metadata.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use caexfer::{Options, convert, validate};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "caexfer-voxel-routes-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn write(&self, name: &str, bytes: impl AsRef<[u8]>) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
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

fn grid_input(scratch: &Scratch) -> PathBuf {
    scratch.write(
        "source.vti",
        r#"<?xml version="1.0"?>
<VTKFile type="ImageData" version="0.1" byte_order="LittleEndian">
<ImageData WholeExtent="0 2 0 1 0 1" Origin="2 3 4" Spacing="0.5 0.5 0.5">
<Piece Extent="0 2 0 1 0 1"><PointData/><CellData Scalars="occupancy">
<DataArray type="UInt8" Name="occupancy" format="ascii">1 1</DataArray>
</CellData></Piece></ImageData></VTKFile>"#,
    )
}

#[test]
fn voxel_grid_routes_preserve_geometry_or_report_loss() {
    let scratch = Scratch::new();
    let source = grid_input(&scratch);
    let source_report = validate(&source, &Options::default()).unwrap();
    assert_eq!((source_report.points, source_report.cells), (12, 2));
    let options = Options {
        accept_all: true,
        ..Options::default()
    };
    for extension in [
        "vtu", "vtk", "msh", "inp", "bdf", "frd", "stl", "su2", "unv", "exo", "vox",
    ] {
        let output = scratch.path(&format!("result.{extension}"));
        let report = convert(&source, &output, &options)
            .unwrap_or_else(|error| panic!("VTI to {extension}: {error}"));
        assert!(output.is_file(), "{extension}");
        assert_eq!(
            report.cells,
            if extension == "stl" { 20 } else { 2 },
            "{extension}"
        );
        if extension == "vox" {
            assert!(
                report
                    .omissions
                    .iter()
                    .any(|item| item.detail.contains("origin and spacing"))
            );
        }
        if extension != "vox" {
            let reread = validate(&output, &Options::default()).unwrap();
            assert!(reread.cells > 0, "{extension}");
        }
    }
}

#[test]
fn vti_rejects_unsupported_metadata_through_public_api() {
    let scratch = Scratch::new();
    let canonical = std::fs::read_to_string(grid_input(&scratch)).unwrap();
    let variants = [
        canonical.replace("Spacing=\"0.5 0.5 0.5\"", "Spacing=\"0.5 1 0.5\""),
        canonical.replace("WholeExtent=\"0 2", "WholeExtent=\"1 2"),
        canonical.replace("format=\"ascii\"", "format=\"binary\""),
        canonical.replace("Name=\"occupancy\"", "Name=\"density\""),
        canonical.replace("1 1</DataArray>", "1 1 1</DataArray>"),
    ];
    for (index, text) in variants.iter().enumerate() {
        let path = scratch.write(&format!("bad-{index}.vti"), text);
        assert!(
            validate(&path, &Options::default()).is_err(),
            "variant {index}"
        );
    }
}

#[test]
fn vox_rejects_corrupt_geometry_through_public_api() {
    let scratch = Scratch::new();
    let source = grid_input(&scratch);
    let vox = scratch.path("good.vox");
    convert(
        &source,
        &vox,
        &Options {
            accept_all: true,
            ..Options::default()
        },
    )
    .unwrap();
    let original = std::fs::read(&vox).unwrap();
    let mut variants = Vec::new();
    let mut bad = original.clone();
    bad[0] = b'X';
    variants.push(bad);
    let mut bad = original.clone();
    bad[32..36].copy_from_slice(&0_u32.to_le_bytes());
    variants.push(bad);
    let mut bad = original.clone();
    bad[60] = 2;
    variants.push(bad);
    let mut bad = original.clone();
    bad[64..68].copy_from_slice(&original[60..64]);
    variants.push(bad);
    for (index, bytes) in variants.iter().enumerate() {
        let path = scratch.write(&format!("bad-{index}.vox"), bytes);
        assert!(
            validate(&path, &Options::default()).is_err(),
            "variant {index}"
        );
    }
}

#[test]
fn voxel_size_requires_a_mesh_source_and_explicit_size() {
    let scratch = Scratch::new();
    let source = grid_input(&scratch);
    let vti_output = scratch.path("copy.vti");
    let error = convert(
        &source,
        &vti_output,
        &Options {
            voxel_size: Some(1.0),
            accept_all: true,
            ..Options::default()
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "E_USAGE");
    assert!(!vti_output.exists());
    let plate = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/plate.bdf");
    let no_size = convert(
        &plate,
        scratch.path("plate.vti"),
        &Options {
            accept_all: true,
            ..Options::default()
        },
    )
    .unwrap_err();
    assert_eq!(no_size.code, "E_USAGE");
}

#[test]
fn stl_to_volume_and_bdf_to_occupancy_round_trip() {
    let scratch = Scratch::new();
    let source = grid_input(&scratch);
    let stl = scratch.path("surface.stl");
    convert(
        &source,
        &stl,
        &Options {
            accept_all: true,
            ..Options::default()
        },
    )
    .unwrap();
    let filled = scratch.path("filled.vtu");
    let volume = convert(
        &stl,
        &filled,
        &Options {
            voxel_size: Some(0.5),
            accept_all: true,
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!(volume.cells, 2);
    assert!(
        volume
            .omissions
            .iter()
            .any(|item| item.detail.contains("Hex8"))
    );

    let bdf = scratch.path("volume.bdf");
    convert(
        &filled,
        &bdf,
        &Options {
            accept_all: true,
            ..Options::default()
        },
    )
    .unwrap();
    let raster = scratch.path("back.vti");
    let report = convert(
        &bdf,
        &raster,
        &Options {
            voxel_size: Some(0.5),
            accept_all: true,
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!(report.voxel_grid, Some(([2, 1, 1], 2)));
    assert!(
        report
            .omissions
            .iter()
            .any(|item| item.detail.contains("property IDs"))
    );
}

#[test]
fn bounded_format_mutations_never_panic() {
    let scratch = Scratch::new();
    let vti = grid_input(&scratch);
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    // Binary Exodus/OP2 framing is covered by focused adapter tests. Mutating
    // arbitrary encoded lengths here can request enormous allocations upstream.
    let mut inputs = vec![
        vti.clone(),
        root.join("examples/plate.bdf"),
        root.join("tests/fixtures/gmsh-2.2-mixed.msh"),
        root.join("tests/fixtures/gmsh-six-kind.unv"),
        root.join("tests/fixtures/linear-results.frd"),
        root.join("tests/fixtures/vtk-5.1-mixed.vtk"),
        root.join("tests/fixtures/pch-multiple.pch"),
    ];
    for extension in ["vtu", "stl", "vox", "inp", "su2"] {
        let output = scratch.path(&format!("generated.{extension}"));
        convert(
            &vti,
            &output,
            &Options {
                accept_all: true,
                ..Options::default()
            },
        )
        .unwrap();
        inputs.push(output);
    }
    for (format_index, input) in inputs.iter().enumerate() {
        let bytes = std::fs::read(input).unwrap();
        let extension = input.extension().unwrap().to_str().unwrap();
        let candidate = scratch.path(&format!("mutation-{format_index}.{extension}"));
        let mut options = Options {
            max_bytes: bytes.len() + 16,
            ..Options::default()
        };
        if extension == "pch" {
            options.mesh = Some(root.join("tests/fixtures/pch-companion.bdf"));
            options.subcase = Some(2);
            options.step = Some(1);
        }
        assert!(validate(input, &options).is_ok(), "{extension} baseline");
        let mut mutations = Vec::new();
        for length in [0, 1, bytes.len() / 4, bytes.len() / 2, bytes.len() - 1] {
            mutations.push(bytes[..length].to_vec());
        }
        for position in 0..bytes.len() {
            for replacement in [0, b' ', b'\n', b'0', b'9', 0xff] {
                let mut changed = bytes.clone();
                changed[position] = replacement;
                mutations.push(changed);
            }
        }
        for (mutation_index, changed) in mutations.into_iter().enumerate() {
            std::fs::write(&candidate, changed).unwrap();
            let outcome = std::panic::catch_unwind(|| validate(&candidate, &options));
            let parsed = outcome.unwrap_or_else(|_| {
                panic!("parser panicked on {extension} mutation {mutation_index}")
            });
            if let Err(error) = parsed {
                assert!(error.code.starts_with("E_"), "{extension}: {error}");
            }
        }
    }
}
