#!/usr/bin/env python3
"""Check CLI projection behavior against independent fixture expectations.

Build ``target/debug/caexfer`` first, or pass ``--binary``. ``--vtk`` adds an
optional read using VTK's own XML reader. The script writes only temporary files
and fails on any unexpected exit code, topology, ID, or output side effect.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]


def main() -> None:
    """Run BDF-to-VTU interoperability and failure-path checks."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, help="path to an existing caexfer binary")
    parser.add_argument("--vtk", action="store_true", help="also require an independent Python VTK read")
    args = parser.parse_args()
    binary = (args.binary or ROOT / "target/debug" / ("caexfer.exe" if sys.platform == "win32" else "caexfer")).resolve()
    if not binary.is_file():
        raise SystemExit(f"No executable at {binary}; run cargo build first or pass --binary.")
    checks: list[str] = []

    def run(*cmd: str | Path, expected_code: int = 0) -> subprocess.CompletedProcess:
        """Run the CLI and retain stdout/stderr for exact output assertions."""
        result = subprocess.run([str(binary), *map(str, cmd)], cwd=ROOT, capture_output=True)
        if result.returncode != expected_code:
            raise AssertionError((cmd, result.returncode, result.stdout.decode(errors="replace"), result.stderr.decode(errors="replace")))
        return result
    with tempfile.TemporaryDirectory(prefix="caexfer-interop-") as temp:
        temp = Path(temp)
        # Inspect the current projected-data JSON contract before conversion.
        info = json.loads(run("info", "examples/plate.bdf", "--json").stdout)
        assert info["schema_version"] == 1 and (info["points"], info["cells"], info["fields"]) == (4, 1, 0)
        assert info["format"] == "bdf" and info["omissions"]
        checks.append("CLI info emits projected counts and source omissions")
        source = ROOT / "tests/fixtures/mixed-linear.bdf"
        destination = temp / "mixed.vtu"
        result = run("convert", source, destination, "--accept-projection", "--json")
        report = json.loads(result.stdout)
        assert report["points"] == 9 and report["cells"] == 7 and report["omissions"]
        expected = json.loads((ROOT / "tests/fixtures/mixed-linear.expected.json").read_text())
        piece = ET.parse(destination).find("./UnstructuredGrid/Piece")
        assert piece is not None

        def values(xpath: str, convert=int) -> list:
            """Read numeric values from one required VTU DataArray."""
            node = piece.find(xpath)
            assert node is not None
            return [convert(item) for item in (node.text or "").split()]
        node_ids = values("./PointData/DataArray[@Name='nastran_node_id']")
        element_ids = values("./CellData/DataArray[@Name='nastran_element_id']")
        property_ids = values("./CellData/DataArray[@Name='nastran_property_id']")
        coordinates = values("./Points/DataArray", float)
        connectivity = values("./Cells/DataArray[@Name='connectivity']")
        offsets = values("./Cells/DataArray[@Name='offsets']")
        types = values("./Cells/DataArray[@Name='types']")
        # Compare original IDs and every cell's ordered connectivity against
        # a hand-authored expectation, not another caexfer reader.
        assert node_ids == [p["id"] for p in expected["nodes"]]
        assert coordinates == [x for p in expected["nodes"] for x in p["xyz"]]
        start = 0
        for i, cell in enumerate(expected["cells"]):
            assert element_ids[i] == cell["id"] and property_ids[i] == cell["property_id"]
            assert types[i] == cell["vtk_type"]
            assert [node_ids[j] for j in connectivity[start:offsets[i]]] == cell["node_ids"]
            start = offsets[i]
        assert start == len(connectivity)
        checks.append("actual Rust VTU output matches independent IDs, points, topology, offsets and types")
        if args.vtk:
            # An independent VTK reader checks that the file is usable outside
            # the crate, beyond its XML shape and our fixture comparison.
            try:
                from vtkmodules.vtkIOXML import vtkXMLUnstructuredGridReader
            except ImportError as error:
                raise SystemExit("--vtk requested, but Python VTK is not installed") from error
            reader = vtkXMLUnstructuredGridReader()
            reader.SetFileName(str(destination))
            reader.Update()
            mesh = reader.GetOutput()
            assert mesh.GetNumberOfPoints() == 9 and mesh.GetNumberOfCells() == 7
            assert [mesh.GetCellType(i) for i in range(7)] == types
            id_array = mesh.GetPointData().GetArray("nastran_node_id")
            assert [id_array.GetValue(i) for i in range(9)] == node_ids
            checks.append("independent VTK reader accepts the actual Rust output and original IDs")
        # Refused conversions must leave no destination; an existing one must
        # retain its original bytes.
        run("convert", source, temp / "no-ack.vtu", expected_code=2)
        assert not (temp / "no-ack.vtu").exists()
        run("convert", "examples/unsupported.bdf", temp / "unknown.vtu", "--accept-projection", expected_code=1)
        assert not (temp / "unknown.vtu").exists()
        run("convert", "examples/nonbasic-frame.bdf", temp / "frame.vtu", "--accept-projection", expected_code=1)
        assert not (temp / "frame.vtu").exists()
        checks.append("loss acknowledgement, unknown geometry and frame failures leave no output")
        before = destination.read_bytes()
        run("convert", source, destination, "--accept-projection", expected_code=1)
        assert destination.read_bytes() == before
        checks.append("existing output is not overwritten")
    print(json.dumps({"kind": "executed-cli-interoperability", "checks_passed": checks}, indent=2))


if __name__ == "__main__":
    main()
