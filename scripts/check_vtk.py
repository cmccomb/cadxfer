#!/usr/bin/env python3
"""Verify legacy VTK and VTI interoperability with native VTK bindings.

Build ``target/debug/caexfer`` first and install VTK in this test interpreter.
The script checks Rust-written VTK with VTK itself, then converts a VTK-written
file back through caexfer. All outputs are temporary; failures raise assertions.
"""

from pathlib import Path
import subprocess
import tempfile
import xml.etree.ElementTree as ET

import vtk

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / 'target/debug/caexfer'


def convert(source: Path, target: Path, *options: str) -> None:
    """Require a successful CLI conversion between two fixture paths."""
    result = subprocess.run([str(BINARY), 'convert', str(source), str(target),
                             *options, '--accept-all'], capture_output=True, text=True)
    assert result.returncode == 0, (result.stdout, result.stderr)


def main() -> None:
    """Check geometry, IDs, arrays, and an external-writer round trip."""
    with tempfile.TemporaryDirectory(prefix='caexfer-vtk-') as directory:
        folder = Path(directory)
        mixed = folder / 'mixed.vtk'
        convert(ROOT / 'tests/fixtures/mixed-linear.bdf', mixed)
        # VTK validates geometry and original identity arrays independently.
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
        piece = ET.parse(roundtrip).find('./UnstructuredGrid/Piece')
        assert piece is not None
        assert (int(piece.attrib['NumberOfPoints']),
                int(piece.attrib['NumberOfCells'])) == (9, 7)
        assert [int(value) for value in
                piece.find('./PointData/DataArray[@Name="nastran_node_id"]').text.split()] == list(range(1, 10))

        # Read Rust-written nodal result arrays with the independent VTK reader.
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

        # Independently read the voxel grid, then check a VTK-written VTI
        # against caexfer's reader through its volume-mesh projection.
        occupancy = folder / 'occupancy.vti'
        convert(ROOT / 'tests/fixtures/mixed-linear.bdf', occupancy,
                '--voxel-size', '0.5')
        image_reader = vtk.vtkXMLImageDataReader()
        image_reader.SetFileName(str(occupancy))
        image_reader.Update()
        image = image_reader.GetOutput()
        assert image.GetDimensions() == (3, 3, 3)
        assert image.GetOrigin() == (0.0, 0.0, 0.0)
        assert image.GetSpacing() == (0.5, 0.5, 0.5)
        values = image.GetCellData().GetArray('occupancy')
        assert values is not None
        assert [int(values.GetTuple1(i)) for i in range(8)].count(1) == 3

        external_vti = folder / 'external.vti'
        image_writer = vtk.vtkXMLImageDataWriter()
        image_writer.SetFileName(str(external_vti))
        image_writer.SetDataModeToAscii()
        image_writer.SetInputData(image)
        assert image_writer.Write() == 1
        volume = folder / 'occupancy.vtu'
        convert(external_vti, volume)
        piece = ET.parse(volume).find('./UnstructuredGrid/Piece')
        assert piece is not None
        assert int(piece.attrib['NumberOfCells']) == 3

    print('legacy VTK and VTI independent read/write checks passed')


if __name__ == "__main__":
    main()
