[![CI](https://github.com/cmccomb/caexfer/actions/workflows/ci.yml/badge.svg)](https://github.com/cmccomb/caexfer/actions/workflows/ci.yml)

# caexfer

<p align="center"><img src="assets/logo.png" alt="caexfer logo" width="320"></p>

**Preserve engineering documents. Transfer the parts another format can represent.**

`caexfer` preserves Nastran BDF source bytes, supports precise GRID edits, and
projects supported linear meshes and numeric results between BDF, VTU, MSH 4.1,
INP, FRD, and OP2. Conversions report information the destination cannot carry.

## Quick start

Requires Rust 1.85 or newer. This is a source repository, not a crates.io release:

```sh
cargo install --path . --locked --offline

caexfer formats
caexfer info examples/plate.bdf
caexfer roundtrip examples/plate.bdf plate-copy.bdf
caexfer set-grid examples/plate.bdf edited.bdf --id 20 --xyz 1.2 0 0
caexfer convert examples/plate.bdf plate.vtu --geometry-only
```

`roundtrip` writes a byte-identical copy. `set-grid` changes only the requested
native-frame coordinates. `convert` requires `--geometry-only` to acknowledge
projection into the supported mesh and field subset. Output files must be new;
the CLI never overwrites an existing path.

## Conversion routes

[![Schematic of native BDF copy and GRID edit above BDF, VTU, MSH, INP, FRD, and OP2 representations](assets/conversion-flow.svg)](assets/conversion-flow.svg)

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
| **C** | Native byte copy via `roundtrip`; BDF also supports `set-grid`. |
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
  bytes. Coordinate edits are transactional and reject fixed-width values that
  cannot fit without rounding.
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

Use this checkout as a path dependency:

```toml
[dependencies]
caexfer = { path = "../caexfer" }
```

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

`Document` keeps the native BDF; `geometry()` explicitly projects it. The
library also exposes `vtu`, `msh`, `inp`, `frd`, and `op2` modules, with shared
mesh and field types in `caexfer::core`. Library writers use caller-owned
streams; the CLI handles staged output files.

## Documentation and development

- [CLI reference](docs/CLI.md): commands, flags, JSON output, and exit codes.
- [Support contract](docs/SUPPORT.md): format subsets and conversion limits.
- [Design](docs/DESIGN.md): preservation, projection, and validation decisions.
- [Implementation references](docs/REFERENCES.md) and [contributing](CONTRIBUTING.md).

Run `cargo test --locked --offline` for Rust tests. CI also checks formatting,
Clippy, rustdoc, the generated diagram, Cargo packaging, and independent
conversion routes on Linux, macOS, Windows, and Rust 1.85. Regenerate the figure
with `python3 scripts/generate_readme_diagram.py` after changing format support.

Licensed under MIT OR Apache-2.0. The imported pyNastran fixtures retain their
upstream BSD license; see [fixture provenance](tests/fixtures/PROVENANCE.md).
