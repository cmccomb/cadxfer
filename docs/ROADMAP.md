# Follow-on scope

The 0.1.0 conversion matrix covers a **linear mesh and numeric-field subset**.
Wider compatibility requires independent examples, precise loss reporting, and
checks against the tools that own each format.

## BDF and INP

Resolve coordinate frames, GRDSET and controlled INCLUDE expansion before
claiming broader deck support. Add element types only with verified node order
and solver references. Geometry-only BDF/INP output should gain optional
property/material mapping in a distinct, explicit solver-model workflow.

## VTU and MSH

Add binary/compressed and multi-piece layouts through proven readers and
writers. Preserve Gmsh physical/entity metadata and component labels when a
mapping is available. Extend result support to sparse fields with an explicit
missing-value model, rather than filling unreported values silently.

## FRD and OP2

Extend FRD beyond ASCII and one-material-per-node results with fixtures from
CalculiX. OP2 currently delegates one real displacement table to pyNastran;
a native Rust reader would need named solver dialects, byte-order and word-size
coverage, frame transforms, subcases, and real-world fixtures. Other result
tables, complex modes, time-series export and lazy reads need separate support
contracts. Neither FRD nor OP2 output is implied by their read support.

## Verification and publication

Run the full CI matrix, cross-check with Gmsh, VTK, CalculiX and pyNastran,
add fuzzing and measured resource benchmarks, then revisit crates.io release.
No dates or performance targets are promised.
