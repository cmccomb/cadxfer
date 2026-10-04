# Design decisions

## Source layout

`src/core/` defines mesh, fields, validation, and errors. `src/conversion/`
owns format dispatch and projection reports. `src/bdf/` holds the native BDF
document, parser, and geometry projection. `src/mesh_formats/` groups adapters
that carry mesh geometry.
`src/nastran_results/` groups OP2 and PCH with their shared displacement
projection and OP2 record codec. `src/cli/` contains JSON rendering and staged
output; `src/main.rs` handles commands. Public module paths such as
`caexfer::vtu` and `caexfer::op2` remain unchanged through explicit paths in
`src/lib.rs`.

## Decode first. Interpret second. Project deliberately.

A document is not a mesh. Keeping a BDF source buffer and an index of its native
fields prevents unfamiliar records from disappearing simply because an adapter
does not understand them. The original document can be copied byte-for-byte.

The geometry projection is explicitly narrower. It returns geometry plus an
omission report, refuses unknown geometry-affecting inputs, and never invents
units or coordinate transformations. A BDF-to-VTU operation is not labeled
lossless. Native byte preservation and semantic preservation are separate claims.
`bdf::read_geometry` and `bdf::write_geometry` provide paired mesh-exchange
entry points; `Document` remains the source-preserving entry point.

## Source documents and projected datasets

BDF remains a byte-preserving document with narrow typed views. Other readers
project directly into a linear `Mesh` plus located numeric `Field`s. This is
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

There is no unqualified `is_valid()` for a full BDF model. The public operation
is `validate_geometry()`, and CLI JSON identifies its scope and explicitly says
`full_solver_validation: false`. Nongeometry cards are not silently blessed as
valid simply because they survived lexical parsing.

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
