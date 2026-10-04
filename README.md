[![CI](https://github.com/cmccomb/caexfer/actions/workflows/ci.yml/badge.svg)](https://github.com/cmccomb/caexfer/actions/workflows/ci.yml)
[![Measured Rust line coverage](https://raw.githubusercontent.com/cmccomb/caexfer/coverage-badge/coverage.svg)](https://github.com/cmccomb/caexfer/actions/workflows/coverage.yml)

# caexfer

<p align="center"><img src="https://raw.githubusercontent.com/cmccomb/caexfer/main/assets/logo.svg" alt="caexfer logo" width="320"></p>

**Exchange finite-element meshes and results across formats.**

`caexfer` reads and writes supported subsets of STL, VTU, legacy VTK, Gmsh MSH
4.1/2.2, Abaqus/CalculiX INP, CalculiX FRD, Nastran BDF, and Nastran OP2.
It also reads displacement results from PCH when given a
matching mesh. Each conversion reports omitted data and explicit assumptions.
BDF users can also keep and inspect the original document bytes.

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

[![Schematic of BDF, VTU, VTK, MSH, INP, FRD, OP2, and PCH representations](https://raw.githubusercontent.com/cmccomb/caexfer/main/assets/conversion-flow.svg)](https://github.com/cmccomb/caexfer/blob/main/assets/conversion-flow.svg)

The figure and matrix summarize the mesh and result routes. STL is also
available for triangle surfaces: it has no source IDs or fields and accepts no
volume cells. See the [format limits](https://github.com/cmccomb/caexfer/blob/main/docs/SUPPORT.md)
for its conversion contract.

| From ↓ / To →           | BDF | VTU | VTK | MSH 4.1/2.2 | INP | FRD | OP2 (+ optional mesh) |
|-------------------------|-----|-----|-----|-------------|-----|-----|-----------------------|
| **BDF**                 | M   | M   | M   | M           | M   | M†  | Z                     |
| **VTU**                 | M   | F   | F   | F           | M   | F†  | D‡                    |
| **VTK legacy**          | M   | F   | F   | F           | M   | F†  | D‡                    |
| **MSH 4.1/2.2**         | M   | F   | F   | F           | M   | F†  | D‡                    |
| **INP**                 | M   | M   | M   | M           | M   | M†  | Z                     |
| **FRD**                 | M   | F   | F   | F           | M   | F†  | D‡                    |
| **OP2 + matching mesh** | M*  | F*  | F*  | F*          | M*  | F*† | D*‡                   |
| **PCH + matching mesh** | M*  | F*  | F*  | F*          | M*  | F*† | D*‡                   |

| Key   | Result                                                                                                                                                                                                                                                                                                                                |
|-------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| **M** | Linear mesh projection with original node and element IDs.                                                                                                                                                                                                                                                                            |
| **F** | Mesh plus supported numeric fields.                                                                                                                                                                                                                                                                                                   |
| **D** | One real displacement table written to OP2 in Rust.                                                                                                                                                                                                                                                                                   |
| **Z** | Synthetic OP2 with six float-zero displacement components per node; requires `--assume-zero-displacement` and labels its values as assumed, not solver results.                                                                                                                                                                       |
| `*`   | Reading OP2 or PCH requires `--mesh` with a matching BDF, VTU, VTK, MSH, INP, or FRD mesh. Node IDs must match; results cannot verify companion coordinates or cells. BDF verifies `GRID CD=0`. Other formats require `--assume-basic-frame`.                                                                                         |
| `†`   | FRD does not support five-node pyramids; its output can carry nodal fields and rounds ASCII values to six significant digits. `‡` OP2 writing requires one selected three- or six-component `DISP` field. If rotations are absent, `--zero-missing-rotations` explicitly asserts they are float zero; otherwise the conversion fails. |

The [format limits](https://github.com/cmccomb/caexfer/blob/main/docs/SUPPORT.md)
describe supported cells, result subsets, omissions, and resource limits.

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
refuses to overwrite them. For source-preserving BDF work, use
`bdf::Document`; see the [library guide](https://github.com/cmccomb/caexfer/blob/main/docs/LIBRARY.md)
for that API and other format adapters.

Licensed under MIT OR Apache-2.0. The imported pyNastran fixtures retain their
upstream BSD license; see [fixture provenance](https://github.com/cmccomb/caexfer/blob/main/tests/fixtures/PROVENANCE.md).
