# Design decisions

## Source layout

`src/core/` defines mesh, fields, validation, and errors. `src/conversion/`
separates format/options, receipt types, source reading, and destination
writing. `src/formats/` groups adapters by role: `solver_inputs/` holds BDF
and INP; `geometry_only/` holds STL, SU2, and UNV; `mesh_datasets/` holds
Exodus, FRD, MSH, VTK, and VTU; and `companion_results/` holds OP2, PCH, and
their shared displacement projection. OP2's record codec lives in its own
subdirectory. `src/formats/mod.rs` exposes every adapter directly under
`caexfer::formats`, so callers do not depend on role folders. `src/cli/`
contains parsing, dispatch, JSON rendering, and staged output. `src/main.rs`
only starts the private CLI.
Each `mod.rs` is a module map; implementation and tests live in the files it
declares. CLI arguments, conversion, validation, and dispatch have separate
files, as do OP2 binary records and displacement handling.

## Project deliberately

BDF parsing is internal to its geometry adapter. The public reader returns a
mesh and omission report, refuses unknown geometry-affecting inputs, and never
invents units or coordinate transformations. Its writer emits a new geometry
deck. It does not copy or validate a complete solver model.

## Projected datasets

Readers project into a linear `Mesh` plus located numeric `Field`s. This is
not a universal solver schema: loads, constraints, units and constitutive laws
are never inferred from a mesh. Every CLI conversion needs an explicit
`--accept-projection` acknowledgement and reports source/destination omissions.

FRD and OP2 have different boundaries. The FRD adapter reads and writes a
documented ASCII subset directly. OP2 reads and writes a bounded 32-bit real
SORT1 OUGV1 displacement subset in Rust, validating record framing and table
metadata before interpreting values. CI uses pyNastran as an independent reader.
Reading OP2 requires a matching mesh and checks node identity. A BDF also
supplies GRID CD, so the reader can reject nonbasic displacement frames.
Other supported mesh formats require an explicit basic-frame assertion because
they do not encode CD. Writing OP2 emits no geometry; the CLI can optionally
write a separate companion mesh. A recognized three-component displacement has
unknown rotations and fails by default; an explicit assertion can set them to
float zero, and the fill is reported. Format modules are part of one Rust
package; the CLI has no Python runtime dependency. PCH projects a bounded
real SORT1 ASCII displacement block through the same normalized nodal field
path as OP2. Its read route uses the same companion mesh and frame checks;
PCH writing is not yet supported.

A result-free BDF or INP can deliberately produce a synthetic all-zero OP2
through `--assume-zero-displacement`. This is an explicit hypothetical field,
not a computed result. The OP2 title records that provenance and the reader
reports it. OP2 has no typed null slot for unknown displacement values.

## Precision and identity

Source node/element IDs are not array offsets. Mapping to contiguous indices
occurs only at projection time; IDs remain separate UInt64 attributes. Never
round a large ID through f64.

No physical units are inferred from coordinate magnitudes or from the suffix
`.bdf`. Native-frame GRID coordinates are not global coordinates unless their
coordinate system has been resolved or is the basic frame.

## Scope-aware validation

`validate` uses the supported-subset reader for every format and reports
projected counts and source omissions.
Successful BDF validation establishes a usable geometry projection; it says
nothing about solver-model correctness. Opaque solver cards appear as omissions.

## Safe file operations

Input reads are bounded. INCLUDE is an inert native record, not permission to
traverse the filesystem. Format adapters do not spawn tools.
Outputs use a new filename; the CLI stages writes and refuses an existing path.
The library writes to caller-provided streams and documents partial-I/O behavior.

## What remains to earn a wider claim

Run the Rust suite on all supported platforms, add independently produced
solver decks with redistribution permission, cross-check projections against
pyNastran and at least one solver, fuzz the lexical layer, and benchmark measured
memory/runtime. Only then advertise broader compatibility or performance.
The included deterministic byte-input tests are not a completed fuzz campaign.
