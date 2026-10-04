# Rust library guide

`caexfer` is one Rust package with a library and a CLI. It uses the Rust 2024
edition, requires Rust 1.85 or newer, and uses `quick-xml` for VTU parsing and
pure Rust `netcdf3` for classic Exodus II. It
is available as a versioned dependency after publication:

```toml
[dependencies]
caexfer = "0.1.0"
```

For unreleased commits, use GitHub:

```toml
[dependencies]
caexfer = { git = "https://github.com/cmccomb/caexfer.git" }
```

Your application's Cargo.lock records the resolved Git revision. For a local
checkout, use `caexfer = { path = "../caexfer" }`. Run
`cargo doc --no-deps --open` in the checkout to browse every public type and
method.

Fallible library calls return `core::Result<T>`. Its error has a stable
diagnostic code, and file I/O errors convert to it, so examples can use the
same result type for both caexfer and filesystem operations.

## Choose the right representation

| Need | Start with | Outcome |
| --- | --- | --- |
| Inspect or copy a BDF without rewriting its source | `bdf::Document` | Original bytes, indexed cards, typed GRID access |
| Extract supported BDF geometry | `bdf::mesh::read(bytes)` or `Document::geometry()` | `GeometryProjection { mesh, omissions }` |
| Read a mesh and numeric results | `vtu::read`, `msh::read`, or `frd::read` | `core::Dataset` |
| Read a flat INP mesh | `inp::read` | `Inspection { mesh, omitted_keywords }` |
| Read an STL triangle surface | `stl::read_projection` | Generated facet-local IDs and explicit source losses |
| Read SU2 markers or Gmsh physical groups | `su2::read_projection` or `msh::read_projection` | `Mesh` with named boundary cell sets |
| Read UNV geometry | `unv::read_projection` | Original node and element labels; other datasets reported |
| Read classic Exodus II | `exodus::read_projection` | Linear blocks, ID maps, complete scalar nodal and element time series |
| Read one OP2 displacement result | `op2::read_displacements` | `(Dataset, assumed_zero)`; matching `Mesh` required |
| Read one PCH displacement result | `pch::read` | `Projection { dataset, skipped_blocks }`; matching `Mesh` required |
| Convert a file and inspect losses | `conversion::convert_path` | Caller-owned output stream and `ConversionReport` |

`core::Mesh` contains positive point and cell IDs plus named node and cell sets.
STL and SU2 lack source IDs, so their readers assign deterministic IDs and
report that choice. Cell connectivity
contains **zero-based indices into `mesh.points`**, not point IDs. A
`core::Dataset` adds `Field`s; their values are ordered by entity, then by
component. A field's `location` identifies whether those entities are points
or cells. `Mesh::validate()` and `Dataset::validate()` check structural
invariants, not solver correctness or whether a specific output format can
represent every field.

## Inspect, copy, and project BDF

```rust
use caexfer::{bdf::Document, core::Result};

fn main() -> Result<()> {
    let doc = Document::open("model.bdf")?;
    for grid in doc.grids() {
        println!("GRID {}: {:?}", grid?.id, grid?.coordinates);
    }
    let output = std::fs::OpenOptions::new()
        .write(true).create_new(true).open("model-copy.bdf")?;
    doc.write_to(output)?;
    Ok(())
}
```

GRID coordinates are in the card's native CP frame. `write_to` copies the
original bytes. For conversion, call `geometry()` and inspect `omissions`
before writing its mesh. Run the executable
[`project_geometry.rs`](../examples/project_geometry.rs) example with
`cargo run --example project_geometry`.

## Convert a dataset with numeric fields

The high-level conversion API applies the same projections as the CLI and
returns typed source, destination, and assumption notices:

```rust
use caexfer::conversion::{self, Format, Options};
use caexfer::core::Result;
use std::path::Path;

fn main() -> Result<()> {
    let output = std::fs::OpenOptions::new()
        .write(true).create_new(true).open("results.msh")?;
    let report = conversion::convert_path(
        Path::new("results.vtu"), Format::Msh, &Options::default(), output,
    )?;
    for omission in report.omissions {
        eprintln!("{}: {}", omission.stage.name(), omission.detail);
    }
    Ok(())
}
```

`Options` sets source format, result selection, and byte limits. For OP2 or PCH input,
set `mesh` to a matching mesh file with original node IDs. A mesh whose node IDs
were assigned during import cannot verify Nastran GRID identity.
Set `assume_basic_frame` for a non-BDF companion only when
its coordinates and the result displacements are known to use the basic frame.
`read_path` returns a `ReadResult` if an application needs
to inspect or modify the dataset before calling `conversion::convert`. The
caller owns the output stream and handles incomplete output on failure; use
the CLI for staged no-clobber file creation.

For direct control over format-specific features, call an adapter:

Read a supported ASCII VTU piece and write its mesh and fields as Gmsh MSH 4.1:

```rust
use caexfer::{core::Result, msh, vtu};

fn main() -> Result<()> {
    let source = std::fs::read_to_string("results.vtu")?;
    let mut dataset = vtu::read(&source)?;
    dataset.validate()?;
    let omitted_properties = dataset.mesh.cells.iter()
        .filter(|cell| cell.property_id.is_some()).count();
    if omitted_properties > 0 {
        eprintln!("Omitting {omitted_properties} property IDs: MSH has no mapping");
        for cell in &mut dataset.mesh.cells {
            cell.property_id = None;
        }
    }
    let output = std::fs::OpenOptions::new()
        .write(true).create_new(true).open("results.msh")?;
    msh::write(&dataset, output)?;
    Ok(())
}
```

This calls the format writers directly. The MSH writer rejects property IDs
until the caller explicitly removes them. It also cannot retain component
labels. The CLI performs supported projections, reports these omissions, and
stages no-clobber output. If those attributes matter, check the
[`SUPPORT.md`](SUPPORT.md) contract and preserve the source file.

| Output | Library call | Input |
| --- | --- | --- |
| Source-preserving BDF | `Document::write_to(writer)` | `&Document` |
| Geometry-only BDF | `bdf::mesh::write(&mesh, writer)` | `&Mesh` |
| VTU | `vtu::write(&mesh, writer)` or `vtu::write_data(&dataset, writer)` | `&Mesh` or `&Dataset` |
| Legacy VTK | `vtk::write(&mesh, writer)` or `vtk::write_data(&dataset, writer)` | `&Mesh` or `&Dataset` |
| MSH 4.1 / 2.2 | `msh::write(&dataset, writer)` or `msh::write_version(&dataset, version, writer)` | `&Dataset` |
| Geometry-only INP | `inp::write(&mesh, writer)` | `&Mesh` |
| FRD | `frd::write(&dataset, writer)` | `&Dataset` with supported nodal fields |
| OP2 | `op2::write_displacements(...)` | One real displacement field; native Rust writer |
| STL | `stl::write_data(&dataset, writer)` or `stl::write_ascii(&dataset, writer)` | Triangle surface without numeric fields, properties, or named sets |
| SU2 | `su2::write_data(&dataset, writer)` | 2D/3D mesh with named boundary cell sets; no numeric fields or property IDs |
| UNV geometry | `unv::write_data(&dataset, writer)` | Six supported linear cell families; no pyramids |
| Classic Exodus II | `exodus::write_data(&dataset, writer)` | One cell dimension, topology blocks, scalar time series |

The BDF and INP exporters emit mesh exchange decks, not runnable solver
models. FRD rounds ASCII values. Each writer can reject a dataset that is
structurally valid but outside that format's supported subset.

## Nastran results and I/O boundaries

The OP2 adapter reads a bounded 32-bit real OUGV1 subset directly. To read,
first project the matching BDF into a basic-frame mesh, then pass the OP2 bytes
and mesh to `op2::read_displacements`:

```rust
use caexfer::{bdf::Document, core::Result, op2};
fn main() -> Result<()> {
    let doc = Document::open("model.bdf")?;
    let mesh = doc.geometry()?.mesh;
    let bytes = std::fs::read("results.op2")?;
    let (dataset, assumed_zero) = op2::read_displacements(
        &bytes, &mesh, None, None,
    )?;
    println!("{} fields; assumed zero: {assumed_zero}", dataset.fields.len());
    Ok(())
}
```

The low-level OP2 reader accepts any validated `Mesh` with matching node IDs.
Its caller must ensure basic-frame coordinates and displacement components.
All GRID CD values in a BDF must be zero because OP2 displacements in
nonbasic output frames are not transformed. Check `doc.grids()` before calling
the adapter; the file-level conversion API performs this check for BDF and
requires an explicit assertion for the other formats. The returned boolean marks
an explicitly assumed all-zero table. The OP2 file contains results but no
mesh. The write function returns bytes; the caller chooses how to persist them
and must keep a matching mesh separately. See [`SUPPORT.md`](SUPPORT.md) for
subcase, step, component, precision, and synthetic-result limits.

For text results, call `pch::read(&source, &mesh, subcase, step)` with a
matching mesh. Its `Projection` contains one normalized displacement field
and the number of other result block headers skipped. The file-level API
enforces the same BDF frame check as OP2. PCH has no writer.

All `read` functions report `core::Error` with a stable `code` and optional
one-based source `line`. Human-readable `message` wording is not a stable
interface. BDF reads have configurable [`ParseOptions`](../src/formats/solver_inputs/bdf/syntax.rs)
limits. Other format readers accept source text or bytes supplied by the caller;
bound file reads yourself. Library writers use caller-owned streams and can
leave partial output after an I/O error. Use a temporary file and rename or
another persistence policy appropriate to your application, or use the CLI's
staged output behavior.
