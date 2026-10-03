# Changelog

## Unreleased

- Removed BDF GRID editing from the public library API and CLI to keep the
  package focused on inspection, preservation, and conversion.
- Renamed the project to `caexfer` and consolidated five crates into one
  dependency-free Rust package.
- Added MSH, INP, FRD, and pyNastran-backed OP2 conversion paths, with an
  explicit conversion matrix and reported omissions.
- Added FRD nodal-field output and OP2 displacement output. Missing rotations
  require an explicit zero assertion; BDF/INP-to-OP2 requires an explicit
  synthetic all-zero assumption.
- Narrowed the BDF public API to document, card, GRID, diagnostic, and geometry
  operations.

## 0.1.0 source snapshot — 2026-10-02

- Initial source-preserving BDF reader and editor, linear geometry projection,
  ASCII VTU writer, CLI, fixtures, and tests. Not published to crates.io.
