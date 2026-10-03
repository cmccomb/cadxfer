[![CI](https://github.com/cmccomb/caexfer/actions/workflows/ci.yml/badge.svg)](https://github.com/cmccomb/caexfer/actions/workflows/ci.yml)

# caexfer

<p align="center"><img src="assets/logo.png" alt="caexfer logo" width="320"></p>

**Preserve engineering documents. Transfer the parts another format can represent.**

`caexfer` preserves Nastran BDF source bytes and projects supported linear
meshes and numeric results between BDF, VTU, MSH 4.1, INP, FRD, and OP2.
Conversions report information the destination cannot carry.

## Install and try the CLI

Requires Rust 1.85 or newer. Install directly from GitHub:

```sh
cargo install --git https://github.com/cmccomb/caexfer.git --locked
caexfer --help
```

To run the included examples, clone the repository first:

```sh
git clone https://github.com/cmccomb/caexfer.git
cd caexfer

caexfer formats
caexfer info examples/plate.bdf
caexfer roundtrip examples/plate.bdf plate-copy.bdf
caexfer convert examples/plate.bdf plate.vtu --geometry-only
```

For a local checkout without installing, replace `caexfer` with
`cargo run --locked --`. OP2 routes additionally need Python with pyNastran;
the other routes have no non-Rust runtime dependency.

`roundtrip` writes a byte-identical copy. `convert` requires `--geometry-only`
to acknowledge projection into the supported mesh and field subset. Output
files must be new; the CLI never overwrites an existing path.

## Conversion routes

[![Schematic of a byte-identical BDF copy and BDF, VTU, MSH, INP, FRD, and OP2 representations](assets/conversion-flow.svg)](assets/conversion-flow.svg)

The figure shows what each format can carry. The matrix gives the actual routes:

| From ↓ / To → | BDF | VTU | MSH 4.1 | INP | FRD | OP2 |
| --- | --- | --- | --- | --- | --- | --- |
| **BDF** | C / M | M | M | M | M† | Z |
| **VTU** | M | C / F | F | M | F† | D‡ |
| **MSH 4.1** | M | F | C / F | M | F† | D‡ |
| **INP** | M | M | M | C / M | M† | Z |
| **FRD** | M | F | F | M | C / F | D‡ |
| **OP2 + BDF mesh** | M* | F* | F* | M* | F*† | C / D*‡ |

| Key | Result |
| --- | --- |
| **C** | Native byte copy via `roundtrip`. |
| **M** | Linear mesh projection with original node and element IDs. |
| **F** | Mesh plus supported numeric fields. |
| **D** | One real displacement table written to OP2 via pyNastran. |
| **Z** | Synthetic OP2 with six float-zero displacement components per node; requires `--assume-zero-displacement`. |

`*` Reading OP2 requires `--mesh model.bdf` because OP2 carries no geometry.
`†` FRD does not support five-node pyramids; its output can carry nodal fields and
rounds ASCII values to six significant digits. `‡` OP2 writing requires
pyNastran and one selected three- or six-component `DISP` field. If rotations
are absent, `--zero-missing-rotations` explicitly asserts they are float zero;
otherwise the conversion fails. `Z` also requires pyNastran and labels its
values as assumed, not solver results. OP2 has no typed null for unknown
values. See [format limits](docs/SUPPORT.md) for other omissions and conditions.

## Scope

- BDF parsing preserves comments, unknown cards, line endings, and other source
  bytes. The original document can be copied without rewriting its contents.
- Projection supports linear lines, triangles, quads, tetrahedra, wedges,
  hexahedra, and pyramids. Bars and beams contribute centerlines only;
  higher-order geometry is rejected.
- BDF and INP exports are geometry-only decks, not runnable solver models. OP2
  exports results only and needs a matching BDF for later reading.
- `validate` checks the supported geometry or mesh/field subset, not full solver
  validity. OP2 is the only adapter that calls external Python software.

The full boundary, including rejected dialects and resource limits, is in
[SUPPORT.md](docs/SUPPORT.md).

## Rust library

Add the repository as a Git dependency (Cargo.lock pins the resolved commit):

```toml
[dependencies]
caexfer = { git = "https://github.com/cmccomb/caexfer.git" }
```

For a local checkout, use `caexfer = { path = "../caexfer" }` instead.
The package has not been published to crates.io.

Preserve a BDF while explicitly exporting its supported geometry:

```rust
use caexfer::{bdf::Document, vtu};

fn main() -> Result<(), Box<dyn std::error::Error>> {
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
returns omissions to inspect. Other readers return a `Dataset` or mesh
inspection. Library writers use caller-owned streams; the CLI stages output
files. The library guide below maps each format to its read/write calls.

## Documentation and development

- [CLI reference](docs/CLI.md): commands, flags, JSON output, and exit codes.
- [Library guide](docs/LIBRARY.md): dependencies, API map, examples, and I/O contracts.
- [Support contract](docs/SUPPORT.md): format subsets and conversion limits.
- [Design](docs/DESIGN.md): preservation, projection, and validation decisions.
- [Implementation references](docs/REFERENCES.md) and [contributing](CONTRIBUTING.md).

Run `cargo test --locked --offline` for Rust tests and `cargo doc --no-deps --open`
for API documentation. CI also checks formatting,
Clippy, rustdoc, the generated diagram, Cargo packaging, and independent
conversion routes on Linux, macOS, Windows, and Rust 1.85. Regenerate the figure
with `python3 scripts/generate_readme_diagram.py` after changing format support.

Licensed under MIT OR Apache-2.0. The imported pyNastran fixtures retain their
upstream BSD license; see [fixture provenance](tests/fixtures/PROVENANCE.md).
