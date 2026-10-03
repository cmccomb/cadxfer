# Design decisions

## Decode first. Interpret second. Project deliberately.

A document is not a mesh. Keeping a BDF source buffer and an index of its native
fields prevents unfamiliar records from disappearing simply because an adapter
does not understand them. It also supports precise, reviewable edits without
regenerating an entire deck.

The geometry projection is explicitly narrower. It returns geometry plus an
omission report, refuses unknown geometry-affecting inputs, and never invents
units or coordinate transformations. A BDF-to-VTU operation is not labeled
lossless. Native byte preservation and semantic preservation are separate claims.

## Source documents and projected datasets

BDF remains a byte-preserving document with narrow typed views. Other readers
project directly into a linear `Mesh` plus located numeric `Field`s. This is
not a universal solver schema: loads, constraints, units and constitutive laws
are never inferred from a mesh. Every CLI conversion needs an explicit
`--geometry-only` acknowledgement and reports source/destination omissions.

FRD and OP2 have different boundaries. The FRD adapter reads and writes a
documented ASCII subset directly. OP2 reads and writes one real displacement
table through pyNastran because record framing alone is not displacement data.
Reading OP2 requires a matching BDF geometry projection and checks node
identity and basic result coordinates. Writing OP2 emits no geometry; a matching
BDF must be kept separately. A recognized three-component displacement has
unknown rotations and fails by default; an explicit assertion can set them to
float zero, and the fill is reported. Format
features remain optional; all Rust
workspace dependencies are local.

A result-free BDF or INP can deliberately produce a synthetic all-zero OP2
through `--assume-zero-displacement`. This is an explicit hypothetical field,
not a computed result. The OP2 title records that provenance and the reader
reports it. OP2 has no typed null slot for unknown displacement values.

## Precision and identity

Source node/element IDs are not array offsets. Mapping to contiguous indices
occurs only at projection time; IDs remain separate UInt64 attributes. Never
round a large ID through f64. Coordinate edits try exact f64-round-trippable
text forms; they do not consume a tolerance budget the user did not provide.

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
traverse the filesystem. The BDF parser does not spawn tools. The optional OP2 adapter explicitly launches
a user-selected Python interpreter with pyNastran installed.
Outputs use a new filename; the CLI stages writes and refuses an existing path.
The library writes to caller-provided streams and documents partial-I/O behavior.

## What remains to earn a wider claim

Run the Rust suite on all supported platforms, add independently produced
solver decks with redistribution permission, cross-check native edits against
pyNastran and at least one solver, fuzz the lexical layer, and benchmark measured
memory/runtime. Only then advertise broader compatibility or performance.
The included deterministic byte-input tests are not a completed fuzz campaign.
