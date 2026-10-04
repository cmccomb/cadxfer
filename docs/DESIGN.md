# Design decisions

## Source layout

`src/core/` defines mesh, fields, validation, and errors. `src/conversion/`
separates format/options, receipt types, source reading, and destination
writing. `src/formats/` groups adapters by role: `solver_inputs/` holds BDF
and INP; `geometry_only/` holds STL, SU2, and UNV; `mesh_datasets/` holds
Exodus, FRD, MSH, VTK, and VTU; and `companion_results/` holds OP2, PCH, and
their shared displacement projection. OP2's record codec lives in its own
subdirectory. Format adapters and intermediate mesh types are private to the
crate. `src/api/` separates file conversion and validation while `src/lib.rs`
exposes only those operations plus supporting options and reports. `src/file_output/`
owns staged file installation. `src/cli/` parses flags, calls the two library operations,
renders receipts, and holds converted files privately until interactive approval.
`src/main.rs` starts the CLI without compiling another copy of the engine.
Each `mod.rs` is a module map. CLI arguments, conversion, validation, and
dispatch have separate files, as do OP2 binary records and displacement
handling.

## Test placement

Place a test at the boundary it checks:

| Test | Location | Use it for |
| --- | --- | --- |
| Doctest | Rustdoc on the public function or type | A short, compilable example of the supported library API. Do not use ignored examples. |
| Module test | `#[cfg(test)]` beside implementation in `src/` | Private parsing, acceptance, error, and file-installation behavior. Keep tests inline for one implementation file; use a child `tests.rs` when they share substantial setup or span several implementation files. |
| Integration test | `tests/*.rs` | Black-box behavior through exported `caexfer` items or the built CLI. These tests must not require private module access. |
| Independent tool check | `scripts/check_*.py` | Interoperability with another parser, writer, or solver through the CLI. |

For example, `src/api/convert.rs` tests its private proposal and acceptance
helpers inline. `src/cli/output/tests.rs` exercises private staging and race
paths beside `output.rs`. `tests/api.rs` checks the exported file operations,
and `tests/cli.rs` checks the installed command's output and exit status.
The Gmsh tests in `src/formats/mesh_datasets/msh/interop_tests.rs` use private
adapters, so they remain module tests despite invoking an external tool.
Shared input fixtures live under `tests/fixtures/`. When a behavior needs tests
at more than one boundary, each test should establish a distinct claim.

## Project deliberately

BDF parsing is internal to its geometry adapter. The file-level operations
report omissions, refuse unknown geometry-affecting inputs, and never invent
units or coordinate transformations. BDF output is a new geometry deck; it
does not copy or validate a complete solver model.

## Projected datasets

Readers project into a linear `Mesh` plus located numeric `Field`s. This is
not a universal solver schema: loads, constraints, units and constitutive laws
are never inferred from a mesh. Every CLI conversion needs an explicit
confirmation for reported changes, or explicit acceptance flags in unattended
use. Conversion reports source/destination omissions and assumptions.

FRD and OP2 have different boundaries. The FRD adapter reads and writes a
documented ASCII subset directly. OP2 reads and writes a bounded 32-bit real
SORT1 OUGV1 displacement subset in Rust, validating record framing and table
metadata before interpreting values. CI uses pyNastran as an independent reader.
Reading OP2 requires a matching mesh and checks node identity. A BDF also
supplies GRID CD, so the reader can reject nonbasic displacement frames.
Other supported mesh formats do not encode CD. Validation reports the
unverified basic-frame assumption; conversion requires explicit acceptance.
Writing OP2 emits no geometry; the CLI can optionally write a separate
companion mesh. A recognized three-component displacement has unknown
rotations. The library writer requires an explicit assertion to set
them to float zero. The CLI proposes that fill and asks for confirmation.
Format modules are part of one Rust
package; the CLI has no Python runtime dependency. PCH projects a bounded
real SORT1 ASCII displacement block through the same normalized nodal field
path as OP2. Its read route uses the same companion mesh and frame checks;
PCH writing is not yet supported.

A result-free BDF or INP can deliberately produce a synthetic all-zero OP2
through confirmation or `--accept-synthetic-zero`. This is a hypothetical
field, not a computed result. The OP2 title records that provenance and the reader
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
Both the CLI and public library stage writes and refuse an existing path by
default. The CLI can replace existing regular output files with `--overwrite`
after conversion and acceptance, including paired OP2 and mesh output. A
second install failure can leave the first file replaced. The library remains
no-clobber. Both paths retain completed recovery files after a partial pair;
the CLI also retains links to previous outputs when replacing files. The
internal stream writers may leave partial output in their
caller-owned streams after an I/O failure.

## What remains to earn a wider claim

Run the Rust suite on all supported platforms, add independently produced
solver decks with redistribution permission, cross-check projections against
pyNastran and at least one solver, fuzz the lexical layer, and benchmark measured
memory/runtime. Only then advertise broader compatibility or performance.
The included deterministic byte-input tests are not a completed fuzz campaign.
