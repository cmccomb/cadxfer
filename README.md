[![CI](https://github.com/cmccomb/caexfer/actions/workflows/ci.yml/badge.svg)](https://github.com/cmccomb/caexfer/actions/workflows/ci.yml)
[![Measured Rust line coverage](https://raw.githubusercontent.com/cmccomb/caexfer/coverage-badge/coverage.svg)](https://github.com/cmccomb/caexfer/actions/workflows/coverage.yml)

# caexfer

<p align="center"><img src="assets/logo.png" alt="caexfer logo" width="320"></p>

**Preserve engineering documents. Transfer the parts another format can represent.**

`caexfer` preserves Nastran BDF source bytes and projects supported linear
meshes and numeric results between BDF, VTU, MSH 4.1, INP, FRD, and OP2.
Conversions report information the destination cannot carry.

## Install and try the CLI

Requires Rust 1.85 or newer. Install directly from GitHub:

```sh
cargo install --git https://github.com/cmccomb/caexfer.git
caexfer --help
```

To run the included examples, clone the repository first:

```sh
git clone https://github.com/cmccomb/caexfer.git
cd caexfer

caexfer formats
caexfer info examples/plate.bdf
caexfer convert examples/plate.bdf plate.vtu --accept-projection
# With pyNastran and a source containing displacement results:
caexfer convert tests/fixtures/linear-results.frd results.op2 --zero-missing-rotations \
  --mesh-out results-mesh.bdf --accept-projection
```

For a local checkout without installing, replace `caexfer` with
`cargo run --`. OP2 routes additionally need Python with pyNastran;
the other routes have no non-Rust runtime dependency.

`convert` requires `--accept-projection` to acknowledge projection into the
supported mesh and field subset. Output
files must be new; the CLI never overwrites an existing path.

## Conversion routes

[![Schematic of BDF, VTU, MSH, INP, FRD, and OP2 representations](assets/conversion-flow.svg)](assets/conversion-flow.svg)

The figure shows what each format can carry; the matrix lists `convert` routes:

| From ↓ / To → | BDF | VTU | MSH 4.1 | INP | FRD | OP2 (+ optional mesh) |
| --- | --- | --- | --- | --- | --- | --- |
| **BDF** | M | M | M | M | M† | Z |
| **VTU** | M | F | F | M | F† | D‡ |
| **MSH 4.1** | M | F | F | M | F† | D‡ |
| **INP** | M | M | M | M | M† | Z |
| **FRD** | M | F | F | M | F† | D‡ |
| **OP2 + matching mesh** | M* | F* | F* | M* | F*† | D*‡ |

| Key | Result                                                                                                                                                                                                                                                                                                                                              |
| --- |-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| **M** | Linear mesh projection with original node and element IDs.                                                                                                                                                                                                                                                                                          |
| **F** | Mesh plus supported numeric fields.                                                                                                                                                                                                                                                                                                                 |
| **D** | One real displacement table written to OP2 via pyNastran.                                                                                                                                                                                                                                                                                           |
| **Z** | Synthetic OP2 with six float-zero displacement components per node; requires `--assume-zero-displacement`. Requires pyNastran and labels its values as assumed, not solver results.                                                                                                                                                                 |
|`*` | Reading OP2 requires `--mesh` with a matching BDF, VTU, MSH, INP, or FRD mesh. Node IDs must match; OP2 cannot verify companion coordinates or cells. BDF verifies `GRID CD=0`. Other formats cannot verify the OP2 displacement frame and require `--assume-basic-frame`.                                                                          |
|`†` | FRD does not support five-node pyramids; its output can carry nodal fields and rounds ASCII values to six significant digits. `‡` OP2 writing requires pyNastran and one selected three- or six-component `DISP` field. If rotations are absent, `--zero-missing-rotations` explicitly asserts they are float zero; otherwise the conversion fails. |

See [format limits](docs/SUPPORT.md) for other omissions and conditions.

## Scope

- BDF parsing preserves comments, unknown cards, line endings, and other source
  bytes. The original document can be copied without rewriting its contents.
- Projection supports linear lines, triangles, quads, tetrahedra, wedges,
  hexahedra, and pyramids. Bars and beams contribute centerlines only;
  higher-order geometry is rejected.
- BDF and INP exports are geometry-only decks, not runnable solver models. OP2
  exports results only and needs a matching mesh for later reading.
- `validate` checks the supported geometry or mesh/field subset, not full solver
  validity. OP2 is the only adapter that calls external Python software.

The full boundary, including rejected dialects and resource limits, is in
[SUPPORT.md](docs/SUPPORT.md).

## Rust library

Add the repository as a Git dependency (your application's Cargo.lock pins the resolved commit):

```toml
[dependencies]
caexfer = { git = "https://github.com/cmccomb/caexfer.git" }
```

For a local checkout, use `caexfer = { path = "../caexfer" }` instead.
The package has not been published to crates.io.

Preserve a BDF while explicitly exporting its supported geometry:

```rust
use caexfer::{bdf::Document, core::Result, vtu};

fn main() -> Result<()> {
    let document = Document::open("model.bdf")?;
    let projection = document.geometry()?;
    for omission in &projection.omissions {
        eprintln!("{}: {}", omission.category, omission.detail);
    }
    let output = std::fs::OpenOptions::new()
        .write(true).create_new(true).open("model.vtu")?;
    vtu::write(&projection.mesh, output)?;
    Ok(())
}
```

`Document` keeps the native BDF; `geometry()` explicitly projects it and
returns omissions to inspect. [`conversion::convert_path`](docs/LIBRARY.md)
provides format-aware conversion and a typed omission report. Library writers
use caller-owned streams; the CLI stages output files.

## Documentation and development

- [CLI reference](docs/CLI.md): commands, flags, JSON output, and exit codes.
- [Library guide](docs/LIBRARY.md): dependencies, API map, examples, and I/O contracts.
- [Support contract](docs/SUPPORT.md): format subsets and conversion limits.
- [Design](docs/DESIGN.md): preservation, projection, and validation decisions.
- [Test coverage](docs/COVERAGE.md): measured Rust coverage, scope, and gaps.
- [Implementation references](docs/REFERENCES.md) and [contributing](CONTRIBUTING.md).

Run `cargo test --offline` for Rust tests and `cargo doc --no-deps --open`
for API documentation. CI also checks formatting,
Clippy, rustdoc, the generated diagram, Cargo packaging, and independent
conversion routes on Linux, macOS, Windows, and Rust 1.85. Regenerate the figure
with `python3 scripts/generate_readme_diagram.py` after changing format support.

Licensed under MIT OR Apache-2.0. The imported pyNastran fixtures retain their
upstream BSD license; see [fixture provenance](tests/fixtures/PROVENANCE.md).
