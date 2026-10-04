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

`gmsh-2.2-mixed.msh` was serialized by Gmsh 4.15.2 from the authored mixed
linear mesh. Gmsh renumbered its elements; the fixture tests independent MSH
2.2 syntax and all seven linear topologies, not ID preservation across Gmsh's
own resave operation.

`su2-triangle.geo` is an authored Gmsh geometry with a named wall and fluid
region. Gmsh 4.15.2 generated `su2-triangle.msh` from it using ASCII MSH 4.1.
The opt-in SU2 interop test converts that file to `.su2` and asks SU2_CFD 8.4.0
to load the resulting mesh and `wall` marker.

`msc-reference-displacement.pch` is a minimal displacement block transcribed
from the published MSC Nastran 2021.4 Reference Guide's punch-format example
(grid 101, T2 = 9.994075E-04). Its header sequence and continuation layout are
independent of caexfer. `pch-multiple.pch` is an authored selection fixture.

`mixed-linear.expected.json` records independent, explicit topology expectations
for `mixed-linear.bdf`; it is not output claimed to come from a Rust execution.
The cells overlap on purpose. They are an I/O fixture, not a simulation model.
`mixed-newlines.bdf` deliberately contains a non-UTF-8 comment byte.
