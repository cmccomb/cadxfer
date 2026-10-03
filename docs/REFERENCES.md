# Implementation references

These are specification and comparison references, not endorsement or evidence
that cadxfer passes a vendor qualification suite. No source code from these
projects is vendored into cadxfer.

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
