#!/usr/bin/env python3
"""Execute every advertised conversion route; optionally include real OP2 decoding."""
from __future__ import annotations
import argparse
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / 'target' / 'debug' / ('caexfer.exe' if sys.platform == 'win32' else 'caexfer')


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument('--op2-python', type=Path, help='Python interpreter with pyNastran')
    options = parser.parse_args()
    if not BINARY.is_file():
        raise SystemExit('Build caexfer before running this check')

    def run(*args: object, code: int = 0) -> dict:
        result = subprocess.run([str(BINARY), *map(str, args), '--json'],
                                cwd=ROOT, capture_output=True, text=True)
        assert result.returncode == code, (args, result.returncode, result.stdout, result.stderr)
        return json.loads(result.stdout)

    with tempfile.TemporaryDirectory(prefix='caexfer-matrix-') as folder:
        folder = Path(folder)
        sources = {'bdf': ROOT / 'examples/plate.bdf',
                   'frd': ROOT / 'tests/fixtures/linear-results.frd'}
        for extension in ('vtu', 'msh', 'inp'):
            path = folder / f'source.{extension}'
            run('convert', sources['bdf'], path, '--geometry-only')
            sources[extension] = path
        if options.op2_python:
            sources['op2'] = ROOT / 'tests/fixtures/solid_bending.op2'
        routes = 0
        for source_format, source in sources.items():
            native = folder / f'{source_format}-copy.{source_format}'
            extra = []
            if source_format == 'op2':
                extra = ['--mesh', ROOT / 'tests/fixtures/solid_bending.bdf',
                         '--python', options.op2_python]
            run('roundtrip', source, native)
            assert source.read_bytes() == native.read_bytes()
            for target_format in ('bdf', 'vtu', 'msh', 'inp'):
                target = folder / f'{source_format}-to-{target_format}.{target_format}'
                report = run('convert', source, target, '--geometry-only', *extra)
                assert target.is_file() and report['points'] > 0 and report['cells'] > 0
                expected = (72, 186) if source_format == 'op2' else ((3, 1) if source_format == 'frd' else (4, 1))
                assert (report['points'], report['cells']) == expected, (source_format, target_format, report)
                if target_format == 'bdf':
                    assert run('validate', target)['passed']
                else:
                    info = run('info', target)
                    assert (info['points'], info['cells']) == expected
                    expected_fields = 2 if source_format == 'frd' else (1 if source_format == 'op2' else 0)
                    if target_format == 'inp':
                        expected_fields = 0
                    assert info['fields'] == expected_fields, (source_format, target_format, info)
                    if target_format == 'vtu' and source_format == 'frd':
                        piece = ET.parse(target).find('./UnstructuredGrid/Piece')
                        assert piece is not None
                        disp = piece.find("./PointData/DataArray[@Name='DISP']")
                        stress = piece.find("./PointData/DataArray[@Name='STRESS']")
                        assert disp is not None and stress is not None
                        assert [float(x) for x in disp.text.split()] == [0., 0., 0., 0.1, 0., 0., 0., 0.2, 0.]
                        assert [float(x) for x in stress.text.split()] == [1., 2., 3., 4., 5., 6., 2., 3., 4., 5., 6., 7., 3., 4., 5., 6., 7., 8.]
                    if target_format == 'vtu' and source_format == 'op2':
                        piece = ET.parse(target).find('./UnstructuredGrid/Piece')
                        assert piece is not None
                        field = piece.find("./PointData/DataArray[@Name='DISPLACEMENT_SUBCASE_1']")
                        assert field is not None
                        first = [float(x) for x in field.text.split()[:6]]
                        assert abs(first[0] - 0.007644693832844496) < 1e-9
                        assert first[3:] == [0., 0., 0.]
                routes += 1
            target = folder / f'{source_format}-to-frd.frd'
            report = run('convert', source, target, '--geometry-only', *extra)
            assert target.is_file() and report['points'] == expected[0]
            info = run('info', target)
            assert (info['points'], info['cells']) == expected
            assert info['fields'] == (2 if source_format == 'frd' else
                                      1 if source_format == 'op2' else 0)
            routes += 1

            if source_format == 'frd':
                result = folder / 'frd-to-op2.op2'
                refused = run('convert', source, result, '--geometry-only', code=1)
                assert refused['error']['code'] == 'E_OP2' and not result.exists()
                assert 'unknown rotations' in refused['error']['message']

            if options.op2_python and source_format in ('frd', 'op2'):
                result = folder / f'{source_format}-to-op2.op2'
                if source_format == 'frd':
                    report = run('convert', source, result, '--geometry-only',
                                 '--zero-missing-rotations', '--python', options.op2_python)
                else:
                    report = run('convert', source, result, '--geometry-only', *extra)
                assert result.is_file() and report['fields'] >= 1
                mesh = (ROOT / 'tests/fixtures/solid_bending.bdf' if source_format == 'op2'
                        else folder / 'frd-for-op2.bdf')
                if source_format == 'frd':
                    run('convert', source, mesh, '--geometry-only')
                    assert any('typed float 0.0' in item['detail'] for item in report['omissions'])
                reread = folder / f'{source_format}-op2-reread.vtu'
                run('convert', result, reread, '--mesh', mesh,
                    '--python', options.op2_python, '--geometry-only')
                piece = ET.parse(reread).find('./UnstructuredGrid/Piece')
                field = piece.find("./PointData/DataArray[@Name='DISPLACEMENT_SUBCASE_1']")
                assert field is not None
                first = [float(x) for x in field.text.split()[:6]]
                assert first[3:] == [0., 0., 0.]
                routes += 1
            elif source_format in ('bdf', 'inp', 'vtu', 'msh'):
                target = folder / f'{source_format}-to-op2.op2'
                result = run('convert', source, target, '--geometry-only', code=1)
                assert result['error']['code'] == 'E_OP2' and not target.exists()
        if options.op2_python:
            mesh = folder / 'result-carrier-mesh.bdf'
            run('convert', sources['frd'], mesh, '--geometry-only')
            for carrier in ('vtu', 'msh'):
                enriched = folder / f'frd-fields.{carrier}'
                run('convert', sources['frd'], enriched, '--geometry-only')
                target = folder / f'{carrier}-fields-to-op2.op2'
                report = run('convert', enriched, target, '--python', options.op2_python,
                             '--zero-missing-rotations', '--geometry-only')
                assert target.is_file()
                assert any('typed float 0.0' in item['detail'] for item in report['omissions'])
                assert run('info', target, '--mesh', mesh,
                           '--python', options.op2_python)['fields'] == 1
                routes += 1
        print(json.dumps({'routes_checked': routes,
                          'sources': sorted(sources),
                          'native_copies_checked': len(sources)}))


if __name__ == '__main__':
    main()
