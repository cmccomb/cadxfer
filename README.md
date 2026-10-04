[![CI](https://github.com/cmccomb/caexfer/actions/workflows/ci.yml/badge.svg)](https://github.com/cmccomb/caexfer/actions/workflows/ci.yml)
[![Measured Rust line coverage](https://raw.githubusercontent.com/cmccomb/caexfer/coverage-badge/coverage.svg)](https://github.com/cmccomb/caexfer/actions/workflows/coverage.yml)

# caexfer

<p align="center"><img src="https://raw.githubusercontent.com/cmccomb/caexfer/main/assets/logo.svg" alt="caexfer logo" width="320"></p>

**Exchange finite-element meshes and results across formats.**

`caexfer` reads and writes supported subsets of STL, SU2, UNV, Exodus II,
VTU, legacy VTK, Gmsh MSH 4.1/2.2, Abaqus/CalculiX INP, CalculiX FRD,
Nastran BDF, and Nastran OP2. It also reads PCH displacement results when
given a matching mesh. Each conversion reports omissions and assumptions.

## Install and try the CLI

Requires Rust 1.85 or newer. Until version 0.1.0 is published, install the CLI
from GitHub:

```sh
cargo install --git https://github.com/cmccomb/caexfer.git
caexfer --help
```

To run the included examples, clone the repository first:

```sh
git clone https://github.com/cmccomb/caexfer.git
cd caexfer

# Inspect supported formats and a source with nodal results.
caexfer formats
caexfer info tests/fixtures/linear-results.frd

# Carry the supported mesh and fields through two output formats.
caexfer convert tests/fixtures/linear-results.frd results.vtu --accept-projection
caexfer convert results.vtu results.msh --msh-version 2.2 --accept-projection
```

After publication, install the released CLI with `cargo install caexfer`. For a
local checkout without installing, replace `caexfer` with `cargo run --`. OP2
and PCH routes run in Rust without a Python installation.

`convert` requires `--accept-projection` to acknowledge projection into the
supported mesh and field subset. Output
files must be new; the CLI never overwrites an existing path.

## Conversion routes

[![Twelve format representations, including STL, SU2, UNV, and Exodus II](https://raw.githubusercontent.com/cmccomb/caexfer/main/assets/conversion-flow.svg)](https://github.com/cmccomb/caexfer/blob/main/assets/conversion-flow.svg)

`caexfer formats` lists the current readers and writers. Mesh sources can be
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

## Rust library

Add the published library as a versioned dependency:

```toml
[dependencies]
caexfer = "0.1.0"
```

For unreleased commits, use the repository as a Git dependency (your application's
Cargo.lock pins the resolved commit):

```toml
[dependencies]
caexfer = { git = "https://github.com/cmccomb/caexfer.git" }
```

For a local checkout, use `caexfer = { path = "../caexfer" }` instead.

Convert a VTU file to MSH and inspect the conversion report:

```rust
use caexfer::conversion::{convert_path, Format, Options};
use caexfer::core::Result;
use std::path::Path;

fn main() -> Result<()> {
    let output = std::fs::OpenOptions::new()
        .write(true).create_new(true).open("results.msh")?;
    let report = convert_path(
        Path::new("results.vtu"), Format::Msh, &Options::default(), output,
    )?;
    for omission in &report.omissions {
        eprintln!("{}: {}", omission.stage.name(), omission.detail);
    }
    Ok(())
}
```

The library accepts caller-owned output streams. The CLI stages files and
refuses to overwrite them. See the
[library guide](https://github.com/cmccomb/caexfer/blob/main/docs/LIBRARY.md)
for the conversion API and format adapters.

Licensed under MIT OR Apache-2.0. The imported pyNastran fixtures retain their
upstream BSD license;
see [fixture provenance](https://github.com/cmccomb/caexfer/blob/main/tests/fixtures/PROVENANCE.md).
