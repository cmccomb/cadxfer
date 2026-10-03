# Changelog

## 0.1.0 — initial release candidate

- Preserve Nastran BDF source bytes with a separate, explicit geometry projection.
- Exchange seven linear cell topologies and complete numeric fields through VTU,
  legacy VTK ASCII, Gmsh MSH 4.1/2.2, INP, and CalculiX FRD within each
  adapter's documented subset.
- Read and write bounded real Nastran OP2 displacement tables; read real SORT1
  PCH displacements with a matching companion mesh.
- Report source omissions, destination omissions, and explicit assumptions in
  CLI and library conversion receipts.
- Stage CLI output without overwriting existing files. The executable uses
  native Rust adapters and does not require Python at runtime.

See [the support contract](docs/SUPPORT.md) for accepted dialects and limits.
