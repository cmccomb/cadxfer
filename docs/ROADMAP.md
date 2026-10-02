# Follow-on scope, not implemented features

## First: earn confidence in the BDF foundation

Run compiler/tests/Clippy and CI; add licensed real-world decks and a differential
harness against pyNastran; test edits in a solver; add sustained fuzzing and
measured memory/runtime benchmarks. The publication gate comes before a wider
support badge. Resolve coordinate frames and GRDSET with explicit default
semantics, then add a controlled INCLUDE resolver with a caller-supplied policy.

Expand common BDF card families only with reference cases, precise support
levels, and no-loss native round trips. Higher-order elements need verified
node permutations and reader cross-checks, not corner-node truncation.

## Then: a result-file use case

FRD is a plausible next independently useful reader: document the ASCII dialect
first, then binary dialects, locations, component labels, time steps, and source
IDs. It should shape a real field/result API rather than inherit an invented one.

OP2 follows a separate compatibility plan. Start with a named set of solver
variants and complete displacement tables, with byte order, word size, subcases,
frames, lazy index semantics, and redistribution-safe fixtures. Do not describe
an envelope scanner as a result reader.

## Integration

Add adapters to established Gmsh/VTK Rust crates where their semantics fit. Add
PyO3 bindings after the native API and error model settle. Keep the Python layer
thin; it should not become the only usable interface. Add format detection only
when it reports confidence/ambiguity rather than guessing from a file suffix.

No dates or performance targets are promised by this roadmap.
