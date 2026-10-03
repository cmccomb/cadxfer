# Changelog

## Unreleased — caexfer namespace

Consolidated five workspace crates into one `caexfer` package with a library
and CLI. All format adapters are modules, and the geometry-only BDF writer now
lives under `bdf::mesh`. Removed feature gates, facade crates, duplicated
manifests and licenses, and feature-isolation checks.

Renamed the GitHub repository, five Rust packages, CLI, examples, scripts, and
README branding from `caxifer`, briefly `cadxfer`, to `caexfer`. Added a from/to
conversion matrix.
Added documented ASCII FRD mesh/nodal-field output and pyNastran-backed OP2
real displacement output. Three-component displacements require an explicit
assertion before receiving float `0.0` rotational components, with omission reports and OP2 reread
verification. Mesh-only inputs are never presented as computed results.
Added an explicit BDF/INP-to-OP2 synthetic baseline: `--assume-zero-displacement`
writes six zeros per node and labels the OP2 title as assumed, non-solver data.
Narrowed the BDF public API by hiding source field spans and removing the
physical field format classification and redundant INCLUDE-line accessor.
`Document::cards()` and `Document::card_text()` remain available for inspection.

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
