[![CI](https://github.com/cmccomb/caxifer/actions/workflows/ci.yml/badge.svg)](https://github.com/cmccomb/caxifer/actions/workflows/ci.yml)

# caxifer

<p align="center"><img src="assets/logo.png" alt="caxifer logo" width="320"></p>

**Engineering files, without silent data loss.**

caxifer separates the original engineering document from the information a
particular downstream tool can represent. Read and preserve the document first;
project it into geometry only when that loss of information is explicit.

[![Diagram of model.bdf flowing through caxifer to an exact BDF copy, an edited BDF, a geometry-only VTU mesh, and JSON inspection output](assets/conversion-flow.svg)](assets/conversion-flow.svg)

This is the **0.1.0 source repository**, not a crates.io publication. The Rust
compiler was unavailable in the original authoring environment. See the
[build status](docs/BUILD-STATUS.md) for subsequent local verification and run
the verification commands below.
Do not mistake the version number for a production-readiness claim.

## Start here

Requires Rust/Cargo 1.85 or newer. No third-party Rust crates or native libraries
are required by this workspace. From this directory:

```sh
cargo test --workspace --all-features --locked --offline
cargo install --path crates/caxifer --locked --offline

caxifer formats
caxifer info examples/plate.bdf
caxifer validate examples/plate.bdf
caxifer roundtrip examples/plate.bdf plate-copy.bdf
caxifer set-grid examples/plate.bdf edited.bdf --id 20 --xyz 1.2 0 0
caxifer convert examples/plate.bdf plate.vtu --geometry-only
```

Output paths must be new. `plate-copy.bdf` preserves the original file bytes.
`edited.bdf` changes only the three requested GRID coordinate fields.
`plate.vtu` contains geometry and original IDs, **not a runnable Nastran model**.
The converter reports what it excludes. No command overwrites the source.

`cargo install caxifer` is **not** the installation instruction for this repository:
the package has not been published to a registry.

## What 0.1 does

The BDF reader indexes small-field, large-field, and comma-separated data, with
adjacent continuations. It retains original source bytes, including comments,
unknown cards, line endings, and control-section text. Semantic access covers
GRID records and a documented subset of linear element connectivity.

The native writer is a **document-preserving writer**, not a general BDF model
generator. Coordinate edits patch existing fields transactionally. A value that
cannot fit a fixed-width field without rounding is rejected rather than
silently shortened.

The VTU writer produces ASCII VTK XML UnstructuredGrid geometry with UInt64
`nastran_node_id`, `nastran_element_id`, and `nastran_property_id` arrays.
Absent property IDs, for CONROD, use the reserved value zero. Coordinates are
Float64. Original IDs are not confused with zero-based connectivity indices.

Supported geometry: CROD, CONROD, CBAR, CBEAM, CTRIA3, CQUAD4, linear CTETRA,
CHEXA, CPENTA, and CPYRAM. Bars/beams become their node-to-node centerline only;
section data, orientations, and offsets are not projected. Higher-order solids
are rejected, not reduced to their corner nodes.

**Not implemented:** OP2/FRD readers, result fields, INCLUDE expansion, coordinate
system resolution, GRDSET defaults, general BDF generation, mesh repair, unit
conversion, Python bindings, automatic format detection, binary VTU, or lazy
multi-gigabyte result access. Unsupported data stays in a native document;
unsupported geometry does not get silently skipped during conversion.

The exact boundary is in [SUPPORT.md](docs/SUPPORT.md).

## Command-line use

```sh
caxifer info examples/plate.bdf --json
caxifer validate examples/plate.bdf --strict --json
caxifer info known-bdf-without-extension --from bdf
caxifer info larger.bdf --max-bytes 536870912
```

JSON output carries `schema_version: 1`. `validate` means **geometry-subset
validation**, not complete solver validity. Materials, properties, loads,
constraints, and control sections are not solver-validated. Recognized opaque
nongeometry card types produce warnings; `--strict` turns those warnings into a
failed command. See [CLI.md](docs/CLI.md) for exit codes and error behavior.

## Rust library use

For a local consuming project, use a path dependency to this repository:

```toml
[dependencies]
caxifer-formats = { path = "../caxifer/crates/caxifer-formats", features = ["bdf", "vtu"] }
```

```rust
use caxifer_formats::{bdf::Document, vtu};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let document = Document::open("model.bdf")?;

    // Native representation: nothing is discarded.
    for card in document.cards() {
        println!("{} at physical line {}", card.name(), card.line);
    }

    // Explicitly lossy projection, with a report of omissions.
    let projection = document.geometry()?;
    for omission in &projection.omissions {
        eprintln!("{}: {}", omission.category, omission.detail);
    }
    let file = std::fs::OpenOptions::new()
        .write(true).create_new(true).open("model.vtu")?;
    let mut buffered = std::io::BufWriter::new(file);
    vtu::write(&projection.mesh, &mut buffered)?;
    std::io::Write::flush(&mut buffered)?;
    Ok(())
}
```

A caller writing to its own stream owns output durability/error handling. The
CLI additionally stages its file output before installing a new destination.

For only BDF, depend directly on `crates/caxifer-bdf`. The umbrella crate defaults
to **no format features**. It conditionally re-exports independent format crates;
it does not contain a second implementation.

## Workspace

| Package | Responsibility |
| --- | --- |
| `caxifer` | CLI, JSON reporting, staged no-clobber output |
| `caxifer-bdf` | Native BDF document, field indexing, edits, geometry projection |
| `caxifer-vtu` | ASCII VTU geometry writer |
| `caxifer-formats` | Feature-selected facade (`bdf`, `vtu`, `all-formats`) |
| `caxifer-core` | Diagnostics and the minimal geometry types shared by adapters |

No universal solver IR, runtime plugin registry, AI interface, or empty format
crate is included. Those are not prerequisites for reading a file correctly.

## Verification

```sh
cargo test --workspace --all-features --locked --offline
cargo check -p caxifer-formats --no-default-features --locked --offline
cargo check -p caxifer-formats --no-default-features --features bdf --locked --offline
cargo check -p caxifer-formats --no-default-features --features vtu --locked --offline
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps --locked --offline
python3 scripts/check_source.py
python3 scripts/check_interop.py --build
python3 scripts/generate_readme_diagram.py --check
```

The last script executes the built CLI, checks its JSON and XML with independent
Python parsers, verifies byte-for-byte round trips, and checks all linear cell
families against an explicit fixture. With Python VTK installed, add `--vtk` for
an independent VTK reader check. The Python scripts require Python 3.11+.

CI covers Linux, macOS, Windows, and the intended minimum Rust version. The
[verification record](docs/BUILD-STATUS.md) lists checks that have run and the
remaining limits of this source candidate.

## Development and release

Read [DESIGN.md](docs/DESIGN.md) before changing preservation or projection
semantics. [ROADMAP.md](docs/ROADMAP.md) describes the next format milestones.
[RELEASING.md](docs/RELEASING.md) lists the gates before a registry publication.
Regenerate the diagram with `python3 scripts/generate_readme_diagram.py` after
changing the supported outputs.

Licensed under MIT OR Apache-2.0. Synthetic fixtures use the same license.
