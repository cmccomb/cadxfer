[![Crates.io version](https://img.shields.io/crates/v/caexfer.svg)](https://crates.io/crates/caexfer)
[![Documentation](https://img.shields.io/docsrs/caexfer.svg)](https://docs.rs/caexfer)
[![CI](https://github.com/cmccomb/caexfer/actions/workflows/ci.yml/badge.svg)](https://github.com/cmccomb/caexfer/actions/workflows/ci.yml)
[![Measured Rust line coverage](https://raw.githubusercontent.com/cmccomb/caexfer/coverage-badge/coverage.svg)](https://github.com/cmccomb/caexfer/actions/workflows/coverage.yml)

# caexfer

<p align="center"><img src="https://raw.githubusercontent.com/cmccomb/caexfer/main/assets/logo.svg" alt="caexfer logo" width="320"></p>

**Transfer finite-element meshes, voxel occupancy, and results across formats.**

`caexfer` reads and writes supported subsets of STL, VTK ImageData VTI,
MagicaVoxel VOX, SU2, UNV, Exodus II, VTU, legacy VTK,
Gmsh MSH 4.1/2.2, Abaqus/CalculiX INP, CalculiX FRD,
Nastran BDF, and Nastran OP2. It also reads PCH displacement results when
given a matching mesh. Each conversion reports omissions and assumptions.

## Get started

Requires Rust 1.86 or newer. Install the published CLI from crates.io:

```sh
cargo install caexfer
caexfer --formats
```

Add the published library to your Rust project:

```sh
cargo add caexfer
```

The CLI and library can perform the same conversions. This sequence carries
nodal results from FRD through VTU to Gmsh MSH 2.2, then reads PCH results
with a matching BDF mesh.

**CLI**

```sh
caexfer convert tests/fixtures/linear-results.frd results.vtu
caexfer convert results.vtu results.msh --msh-version 2.2
caexfer convert tests/fixtures/pch-multiple.pch displacements.vtu \
  --mesh tests/fixtures/pch-companion.bdf --subcase 1
```

**Rust library**

```rust
use caexfer::{MshVersion, Options, Result, convert, validate};

fn main() -> Result<()> {
    let source = validate("tests/fixtures/linear-results.frd", &Options::default())?;
    assert!(source.passed);
    let report = convert("tests/fixtures/linear-results.frd", "results.vtu",
                         &Options { accept_omissions: true, ..Options::default() })?;
    for notice in report.omissions {
        eprintln!("{}: {}", notice.stage.name(), notice.detail);
    }
    convert("results.vtu", "results.msh", &Options {
        msh_version: Some(MshVersion::V2_2),
        accept_omissions: true,
        ..Options::default()
    })?;
    convert("tests/fixtures/pch-multiple.pch", "displacements.vtu", &Options {
        mesh: Some("tests/fixtures/pch-companion.bdf".into()),
        subcase: Some(1),
        accept_omissions: true,
        ..Options::default()
    })?;
    Ok(())
}
```

`convert` lists reported losses and assumptions before asking for confirmation.
For scripts, pass the specific `--accept-...` flags named in that list, or use
`--accept-all` to accept every reported change.

## Conversion routes

[![Flowchart of solver inputs, geometry formats, mesh and voxel datasets, and companion results connecting through caexfer](assets/conversion-flow.svg)](assets/conversion-flow.svg)

`caexfer --formats` lists the current readers and writers. Mesh sources can be
converted when the destination supports their cell types and data. VTU, legacy
VTK, MSH, and classic Exodus carry numeric fields within their documented
subsets; FRD carries nodal fields. BDF, INP, STL, SU2, and UNV provide narrower
geometry routes. MSH physical groups become named selections, and SU2 carries
named boundary markers. OP2 and PCH displacement results require a companion
mesh with original node IDs; OP2 can write a selected displacement table, while
PCH is read-only.

VTI and VOX carry binary voxel occupancy. Convert a closed triangle or quad
surface (including STL), or the external boundary of a linear volume mesh,
with `--voxel-size`. Voxel input becomes shared-corner Hex8 volume cells in
mesh formats or exposed, triangulated faces in STL. STL can also become a
Hex8 volume mesh with `--voxel-size`:

```sh
caexfer convert closed.stl solid.vti --voxel-size 0.5 --accept-omissions
caexfer convert solid.vti solid.vox --accept-omissions
caexfer convert solid.vox shell.stl --accept-omissions
caexfer convert solid.vti smooth-shell.stl --smooth-iterations 10 --accept-omissions
caexfer convert closed.stl volume.vtu --voxel-size 0.5 --accept-omissions
```

These are voxel-center samples and blocky meshes, not conforming tetrahedral
meshing. VOX has no physical origin or spacing, and its palette is not carried
through the occupancy model. The conversion report lists those losses.
The optional `--smooth-iterations N` (1–50) moves shared surface vertices with
paired smoothing passes when exporting VTI or VOX to STL. It rounds the voxel
steps but can soften small features and does not recover detail lost during
voxelization. Without this option, STL output follows voxel boundaries exactly.
