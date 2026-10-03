# Fixture provenance

Except for the two `solid_bending` files and the corresponding license, files
in this directory and `examples/` were authored specifically for this package.
They are released under the package license. `linear-results.frd` is an authored
ASCII fixture following the CalculiX GraphiX result-format documentation.

`solid_bending.bdf` and `solid_bending.op2` come from pyNastran's
`models/solid_bending/` directory at commit
`af9f2edc2ac86f2afa412826b12ed18bffd04dc6`. They are included to test
actual OP2 displacement decoding and mesh association. They retain the
upstream three-clause BSD terms in `pyNastran-LICENSE.md`.

`two-subcases.op2` and `two-steps.op2` are small authored fixtures generated
with pyNastran 1.4.1. They contain only two GRID displacement rows per result
step, with explicit values and times, to check independent OP2 decoding.

`vtk-5.1-mixed.vtk` was serialized by VTK 9.6.1's
`vtkUnstructuredGridWriter` from the authored mixed-linear mesh. Its VTK 5.1
offsets/connectivity layout checks a reader independent of caexfer's writer.

`mixed-linear.expected.json` records independent, explicit topology expectations
for `mixed-linear.bdf`; it is not output claimed to come from a Rust execution.
The cells overlap on purpose. They are an I/O fixture, not a simulation model.
`mixed-newlines.bdf` deliberately contains a non-UTF-8 comment byte.
