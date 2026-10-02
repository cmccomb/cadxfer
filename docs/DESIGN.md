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

## Why BDF before OP2

A shallow OP2 table-name scanner would look impressive beside a format support
badge but would not deliver the promised lazy displacement/stress reader.
Correct OP2 support needs vendor/version/endian/word-size coverage, accurate
record/table decoding, original IDs, result locations, and frame conventions.
There is no placeholder OP2 crate or empty feature gate in this release.

BDF establishes a source model, diagnostics, resource limits, transactional
field edits, and an honest projection boundary with an immediately useful CLI.
Those choices are reusable without committing the entire project to one large
universal engineering object graph.

## Small shared core

The core shares only errors/diagnostics and the minimal geometry needed by the
first adapter. It does not force native BDF records into a generic solver schema.
There are no speculative FieldView/ResultView traits without an implementation.
When a second format has genuinely different needs, add a view based on both
formats rather than predicting every future result data model now.

The format facade has optional dependencies and no default formats. Individual
crates remain independently usable. The CLI includes every format actually
implemented in this snapshot. All current Rust dependencies are workspace-local.

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
traverse the filesystem. The parser does not spawn tools or execute code.
Outputs use a new filename; the CLI stages writes and refuses an existing path.
The library writes to caller-provided streams and documents partial-I/O behavior.

## What remains to earn a wider claim

Run the Rust suite on all supported platforms, add independently produced
solver decks with redistribution permission, cross-check native edits against
pyNastran and at least one solver, fuzz the lexical layer, and benchmark measured
memory/runtime. Only then advertise broader compatibility or performance.
The included deterministic byte-input tests are not a completed fuzz campaign.
