# Changelog

## 0.1.0 — source package, 2026-10-02

The CLI, library crates, examples, and documentation use the `caxifer` namespace.

Initial BDF-first implementation: native source preservation, small/large/free
fields, adjacent continuations, finite Nastran numbers, scoped GRID access,
transactional GRID coordinate edits, geometry-subset validation, and an explicit
linear-geometry projection with omission reporting.

Adds an ASCII VTU writer, CLI, feature-selected facade, standalone crates,
synthetic fixtures, Rust tests, independent Python interoperability checks,
CI definitions, and release documentation. The workspace has no external Rust
dependencies and forbids unsafe Rust.

Not published to crates.io. Rust compilation/tests were not run in the original authoring environment;
see docs/BUILD-STATUS.md. OP2/FRD, Python bindings, coordinate resolution,
INCLUDE expansion, and performance claims are deliberately not part of 0.1.0.
