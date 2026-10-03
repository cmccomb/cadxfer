#!/usr/bin/env python3
"""Independently read Rust-written legacy VTK and reread a VTK-written file."""
from pathlib import Path
import subprocess
import tempfile

import vtk

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / 'target/debug/caexfer'


def convert(source: Path, target: Path) -> None:
    result = subprocess.run([str(BINARY), 'convert', str(source), str(target),
                             '--accept-projection'], capture_output=True, text=True)
    assert result.returncode == 0, (result.stdout, result.stderr)


with tempfile.TemporaryDirectory(prefix='caexfer-vtk-') as directory:
    folder = Path(directory)
    mixed = folder / 'mixed.vtk'
    convert(ROOT / 'tests/fixtures/mixed-linear.bdf', mixed)
    reader = vtk.vtkUnstructuredGridReader()
    reader.SetFileName(str(mixed))
    reader.ReadAllScalarsOn()
    reader.ReadAllFieldsOn()
    reader.Update()
    grid = reader.GetOutput()
    assert (grid.GetNumberOfPoints(), grid.GetNumberOfCells()) == (9, 7)
    assert [grid.GetCellType(i) for i in range(7)] == [3, 5, 9, 10, 12, 13, 14]
    assert [int(grid.GetPointData().GetArray('nastran_node_id').GetTuple1(i))
            for i in range(9)] == list(range(1, 10))
    assert [int(grid.GetCellData().GetArray('nastran_element_id').GetTuple1(i))
            for i in range(7)] == list(range(101, 108))

    # VTK's own writer produces a differently serialized external input.
    external = folder / 'external.vtk'
    writer = vtk.vtkUnstructuredGridWriter()
    writer.SetFileName(str(external))
    writer.SetFileTypeToASCII()
    writer.SetInputData(grid)
    assert writer.Write() == 1
    roundtrip = folder / 'external.vtu'
    convert(external, roundtrip)
    import xml.etree.ElementTree as ET
    piece = ET.parse(roundtrip).find('./UnstructuredGrid/Piece')
    assert piece is not None
    assert (int(piece.attrib['NumberOfPoints']),
            int(piece.attrib['NumberOfCells'])) == (9, 7)
    assert [int(value) for value in
            piece.find('./PointData/DataArray[@Name="nastran_node_id"]').text.split()] == list(range(1, 10))

    results = folder / 'results.vtk'
    convert(ROOT / 'tests/fixtures/linear-results.frd', results)
    reader.SetFileName(str(results))
    reader.Update()
    grid = reader.GetOutput()
    disp = grid.GetPointData().GetArray('DISP')
    stress = grid.GetPointData().GetArray('STRESS')
    assert disp is not None and stress is not None
    assert disp.GetNumberOfComponents() == 3
    assert tuple(round(value, 8) for value in disp.GetTuple(1)) == (0.1, 0.0, 0.0)
    assert stress.GetNumberOfComponents() == 6
    assert stress.GetTuple(2) == (3.0, 4.0, 5.0, 6.0, 7.0, 8.0)

print('legacy VTK independent read/write checks passed')
