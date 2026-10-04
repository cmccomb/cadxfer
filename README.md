[![CI](https://github.com/cmccomb/caexfer/actions/workflows/ci.yml/badge.svg)](https://github.com/cmccomb/caexfer/actions/workflows/ci.yml)
[![Measured Rust line coverage](https://raw.githubusercontent.com/cmccomb/caexfer/coverage-badge/coverage.svg)](https://github.com/cmccomb/caexfer/actions/workflows/coverage.yml)

# caexfer

<p align="center"><img src="https://raw.githubusercontent.com/cmccomb/caexfer/main/assets/logo.svg" alt="caexfer logo" width="320"></p>

**Exchange finite-element meshes and results across formats.**


`caexfer` reads and writes supported subsets of STL, SU2, UNV, Exodus II,
VTU, legacy VTK, Gmsh MSH 4.1/2.2, Abaqus/CalculiX INP, CalculiX FRD,
Nastran BDF, and Nastran OP2. It also reads PCH displacement results when
given a matching mesh. Each conversion reports omissions and assumptions.

## Get started

Requires Rust 1.86 or newer. Until version 0.1.0 is published, install the
CLI from GitHub or add the library as a Git dependency:

```sh
cargo install --git https://github.com/cmccomb/caexfer.git
caexfer --help
```

```toml
[dependencies]
caexfer = { git = "https://github.com/cmccomb/caexfer.git" }
```

Your application's `Cargo.lock` pins the Git commit. After publication, use
`cargo install caexfer` for the CLI or `caexfer = "0.1.0"` for the library.
For a local checkout, use `caexfer = { path = "../caexfer" }`.

These examples use fixtures from a checkout:

```sh
git clone https://github.com/cmccomb/caexfer.git
cd caexfer
caexfer --formats
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
use caexfer::conversion::{convert_path, Format, Options};
use caexfer::core::Result;
use caexfer::formats::msh;
use std::fs::OpenOptions;
use std::path::Path;

fn convert_file(input: &str, output: &str, format: Format, options: &Options) -> Result<()> {
    let file = OpenOptions::new().write(true).create_new(true).open(output)?;
    let report = convert_path(Path::new(input), format, options, file)?;
    for notice in report.omissions {
        eprintln!("{}: {}", notice.stage.name(), notice.detail);
    }
    Ok(())
}

fn main() -> Result<()> {
    convert_file("tests/fixtures/linear-results.frd", "results.vtu",
                 Format::Vtu, &Options::default())?;
    convert_file("results.vtu", "results.msh", Format::Msh,
                 &Options {
                     msh_version: Some(msh::Version::V2_2),
                     ..Options::default()
                 })?;
    convert_file("tests/fixtures/pch-multiple.pch", "displacements.vtu",
                 Format::Vtu,
                 &Options {
                     mesh: Some("tests/fixtures/pch-companion.bdf".into()),
                     subcase: Some(1),
                     ..Options::default()
                 })?;
    Ok(())
}
```

`convert` lists reported losses and assumptions before asking for confirmation.
For scripts, pass the specific `--accept-...` flags named in that list, or use
`--accept-all-approximations-and-infill` to accept every reported change.
The library returns the same receipt and writes to a caller-owned stream.
Output files must be new in these examples. The CLI stages output before
installation; library callers own their output policy. OP2 and PCH routes run
in Rust without a Python installation. In a checkout, `cargo run --` can replace
the installed `caexfer` command.

## Conversion routes

[![Twelve format representations, including STL, SU2, UNV, and Exodus II](https://raw.githubusercontent.com/cmccomb/caexfer/main/assets/conversion-flow.svg)](https://github.com/cmccomb/caexfer/blob/main/assets/conversion-flow.svg)

`caexfer --formats` lists the current readers and writers. Mesh sources can be
converted when the destination supports their cell types and data. VTU, legacy
VTK, MSH, and classic Exodus carry numeric fields within their documented
subsets; FRD carries nodal fields. BDF, INP, STL, SU2, and UNV provide narrower
geometry routes. MSH physical groups become named selections, and SU2 carries
named boundary markers. OP2 and PCH displacement results require a companion
mesh with original node IDs; OP2 can write a selected displacement table, while
PCH is read-only.

Every conversion returns a receipt of source omissions, destination omissions,
and explicit assumptions. The [format limits](https://github.com/cmccomb/caexfer/blob/main/docs/SUPPORT.md)
explain topology, field, group, and result restrictions for each adapter.

Format adapters live at paths such as `caexfer::formats::vtu` and
`caexfer::formats::msh`. See the
[library guide](https://github.com/cmccomb/caexfer/blob/main/docs/LIBRARY.md)
for the conversion API and format adapters.

Licensed under MIT OR Apache-2.0. The imported pyNastran fixtures retain their
upstream BSD license;
see [fixture provenance](https://github.com/cmccomb/caexfer/blob/main/tests/fixtures/PROVENANCE.md).
