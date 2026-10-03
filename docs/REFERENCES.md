# Implementation references

These are specification and comparison references, not endorsement or evidence
that caexfer passes a vendor qualification suite. No source code from these
projects is vendored into caexfer.

- Altair, Bulk Data Guidelines: physical field layouts, free/large formats,
  continuations, comments, and implicit real exponents.
  https://help.altair.com/hwsolvers/os/topics/solvers/os/guidelines_bulk_data_r.htm
- Altair, GRID reference: ID, CP, coordinate, CD, PS, and SEID meanings.
  https://help.altair.com/hwsolvers/os/topics/solvers/os/grid_bulk_r.htm
- pyNastran developer guide: BDF structure and small/large-field interpretation.
  https://pynastran-git.readthedocs.io/en/latest/manual/bdf_developer.html
- VTK XML File Formats: UnstructuredGrid, connectivity, offsets, cell types,
  and typed PointData/CellData arrays.
  https://docs.vtk.org/en/latest/vtk_file_formats/vtkxml_file_format.html
- meshio's Nastran implementation, `_convert_to_vtk_ordering`, inspected as a
  comparison for linear vs. higher-order connectivity conventions. File blob
  SHA at inspection: 0e1313c9d71c05eb002b618cf02c412bd5ef64ae.
  https://github.com/nschloe/meshio/blob/main/src/meshio/nastran/_nastran.py
- Gmsh MSH 4.1 specification: node/element blocks and NodeData/ElementData.
  https://gmsh.info/doc/texinfo/gmsh.html#MSH-file-format
- CalculiX GraphiX manual, chapter 11: ASCII FRD node, element and result records.
  https://www.dhondt.de/cgx_2.19.pdf
- Abaqus/Explicit element index: linear INP element type names.
  https://docs.software.vt.edu/abaqusv2025/English/SIMACAEELMRefMap/simaelm-c-expelementindex.htm
- pyNastran OP2 NumPy demo: displacement table, node IDs and six result components.
  https://pynastran-git.readthedocs.io/en/latest/quick_start/op2_demo_numpy1.html
- pyNastran result-object documentation: static/transient real table creation and
  OP2 writing interfaces used by the optional displacement exporter.
  https://pynastran-git.readthedocs.io/en/latest/reference/op2/result_objects/pyNastran.op2.result_objects.html
- Cargo documentation: workspaces, feature selection, local installation.
  https://doc.rust-lang.org/cargo/reference/workspaces.html
  https://doc.rust-lang.org/cargo/reference/features.html
  https://doc.rust-lang.org/cargo/commands/cargo-install.html

Vendor dialects differ. These references establish the intended subset; they do
not justify claiming complete MSC/NX/OptiStruct interoperability. Revisit the
relevant primary specification before extending any card or result-table schema.

## Real-field writing

pyNastran developer documentation explains Nastran's lexical distinction between integer and real fields. Edited coordinate values always retain a decimal point, including integer-valued and scientific representations.

https://pynastran-git.readthedocs.io/en/latest/manual/bdf_developer.html
