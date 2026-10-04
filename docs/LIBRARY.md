# Rust library guide

`caexfer` is one Rust package with a library and a CLI. It uses the Rust 2024
edition, requires Rust 1.86 or newer, and uses `quick-xml` for VTU parsing and
pure Rust `netcdf3` for classic Exodus II. It
is available from crates.io:

```sh
cargo add caexfer
```

For development commits, use GitHub:

```toml
[dependencies]
caexfer = { git = "https://github.com/cmccomb/caexfer.git" }
```

Your application's `Cargo.lock` records the resolved Git revision. For a local
checkout, use `caexfer = { path = "../caexfer" }`. Run
`cargo doc --no-deps --open` in the checkout to browse every public type and
method.

Fallible library calls return `core::Result<T>`. Its error has a stable
diagnostic code, and file I/O errors convert to it, so examples can use the
same result type for both caexfer and filesystem operations.

## Choose the right representation

Import adapters from `caexfer::formats`, such as `caexfer::formats::vtu`.
The role directories under `src/formats/` are internal source organization.

| Need | Start with | Outcome |
| --- | --- | --- |
| Extract supported BDF geometry | `formats::bdf::mesh::read(bytes)` or `formats::bdf::mesh::read_from(reader, max_bytes)` | `GeometryProjection { mesh, omissions, has_nonbasic_output_frame }` |
| Read a mesh and numeric results | `formats::vtu::read`, `formats::msh::read`, or `formats::frd::read` | `core::Dataset` |
| Read a flat INP mesh | `formats::inp::read` | `Inspection { mesh, omitted_keywords }` |
| Read an STL triangle surface | `formats::stl::read_projection` | Generated facet-local IDs and explicit source losses |
| Read SU2 markers or Gmsh physical groups | `formats::su2::read_projection` or `formats::msh::read_projection` | `Mesh` with named boundary cell sets |
| Read UNV geometry | `formats::unv::read_projection` | Original node and element labels; other datasets reported |
| Read classic Exodus II | `formats::exodus::read_projection` | Linear blocks, ID maps, complete scalar nodal and element time series |
| Read one OP2 displacement result | `formats::op2::read_displacements` | `(Dataset, assumed_zero)`; matching `Mesh` required |
| Read one PCH displacement result | `formats::pch::read` | `Projection { dataset, skipped_blocks }`; matching `Mesh` required |
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

## Project BDF geometry

```rust
use caexfer::core::Result;
use caexfer::formats::bdf;

fn main() -> Result<()> {
    let input = std::fs::File::open("model.bdf")?;
    let projection = bdf::mesh::read_from(input, 256 * 1024 * 1024)?;
    for omission in &projection.omissions {
        eprintln!("{}: {}", omission.category, omission.detail);
    }
    Ok(())
}
```

Only basic-frame GRID coordinates enter the mesh. Inspect `omissions` before
writing it. The output is a geometry deck, not a copy of the input. Run the
executable
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

Each `Omission` has a `stage`. Its `assumption` identifies basic-frame,
zero-rotation, or synthetic-zero values when `stage` is `Assumption`. Library
callers decide which notices they can accept; CLI acceptance flags do not
change the library's default `Options`.

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
use caexfer::core::Result;
use caexfer::formats::{msh, vtu};

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
| Geometry-only BDF | `formats::bdf::mesh::write(&mesh, writer)` | `&Mesh` |
| VTU | `formats::vtu::write(&mesh, writer)` or `formats::vtu::write_data(&dataset, writer)` | `&Mesh` or `&Dataset` |
| Legacy VTK | `formats::vtk::write(&mesh, writer)` or `formats::vtk::write_data(&dataset, writer)` | `&Mesh` or `&Dataset` |
| MSH 4.1 / 2.2 | `formats::msh::write(&dataset, writer)` or `formats::msh::write_version(&dataset, version, writer)` | `&Dataset` |
| Geometry-only INP | `formats::inp::write(&mesh, writer)` | `&Mesh` |
| FRD | `formats::frd::write(&dataset, writer)` | `&Dataset` with supported nodal fields |
| OP2 | `formats::op2::write_displacements(...)` | One real displacement field; native Rust writer |
| STL | `formats::stl::write_data(&dataset, writer)` or `formats::stl::write_ascii(&dataset, writer)` | Triangle surface without numeric fields, properties, or named sets |
| SU2 | `formats::su2::write_data(&dataset, writer)` | 2D/3D mesh with named boundary cell sets; no numeric fields or property IDs |
| UNV geometry | `formats::unv::write_data(&dataset, writer)` | Six supported linear cell families; no pyramids |
| Classic Exodus II | `formats::exodus::write_data(&dataset, writer)` | One cell dimension, topology blocks, scalar time series |

The BDF and INP exporters emit mesh exchange decks, not runnable solver
models. FRD rounds ASCII values. Each writer can reject a dataset that is
structurally valid but outside that format's supported subset.

## Nastran results and I/O boundaries

The OP2 adapter reads a bounded 32-bit real OUGV1 subset directly. To read,
first project the matching BDF into a basic-frame mesh, then pass the OP2 bytes
and mesh to `formats::op2::read_displacements`:

```rust
use caexfer::core::Result;
use caexfer::formats::{bdf, op2};

fn main() -> Result<()> {
    let input = std::fs::File::open("model.bdf")?;
    let projection = bdf::mesh::read_from(input, 256 * 1024 * 1024)?;
    if projection.has_nonbasic_output_frame {
        return Err(caexfer::core::Error::new("E_FRAME", "GRID CD is not basic"));
    }
    let mesh = projection.mesh;
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
nonbasic output frames are not transformed. Check `has_nonbasic_output_frame`
before calling the adapter; the file-level conversion API performs this check for BDF and
requires an explicit assertion for the other formats. The returned boolean marks
an explicitly assumed all-zero table. The OP2 file contains results but no
mesh. The write function returns bytes; the caller chooses how to persist them
and must keep a matching mesh separately. See [`SUPPORT.md`](SUPPORT.md) for
subcase, step, component, precision, and synthetic-result limits.

For text results, call `formats::pch::read(&source, &mesh, subcase, step)` with a
matching mesh. Its `Projection` contains one normalized displacement field
and the number of other result block headers skipped. The file-level API
enforces the same BDF frame check as OP2. PCH has no writer.

All `read` functions report `core::Error` with a stable `code` and optional
one-based source `line`. Human-readable `message` wording is not a stable
interface. `formats::bdf::mesh::read_from` takes an explicit byte limit. Other
format readers accept source text or bytes supplied by the caller; bound file
reads yourself. Library writers use caller-owned streams and can
leave partial output after an I/O error. Use a temporary file and rename or
another persistence policy appropriate to your application, or use the CLI's
staged output behavior.
