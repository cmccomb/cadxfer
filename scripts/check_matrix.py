#!/usr/bin/env python3
"""Execute every advertised conversion route; optionally include real OP2 decoding."""
from __future__ import annotations
import argparse
import contextlib
import io
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
    parser.add_argument('--op2-check', action='store_true', help='exercise native OP2 routes')
    options = parser.parse_args()
    if not BINARY.is_file():
        raise SystemExit('Build caexfer before running this check')

    def independent_op2(path: Path, *, assumed_zero: bool = False,
                        expected_rows: dict[int, list[float]] | None = None) -> None:
        """Check Rust-written bytes with pyNastran, which is CI-only tooling."""
        from pyNastran.op2.op2 import read_op2
        with contextlib.redirect_stdout(io.StringIO()):
            external = read_op2(str(path), build_dataframe=False, debug=False,
                                include_results=['displacements'])
        assert list(external.displacements) == [1]
        result = external.displacements[1]
        assert result.data.shape[0] == 1 and result.data.shape[2] == 6
        assert all(int(kind) == 1 for kind in result.node_gridtype[:, 1])
        if expected_rows is not None:
            actual_rows = {int(node): row.tolist() for node, row in
                           zip(result.node_gridtype[:, 0], result.data[0])}
            assert actual_rows.keys() == expected_rows.keys()
            for node, expected in expected_rows.items():
                assert all(abs(actual - wanted) < 1e-6 for actual, wanted in
                           zip(actual_rows[node], expected))
        if assumed_zero:
            assert 'CAEXFER ASSUMED ZERO DISPLACEMENT' in result.title
            assert (result.data == 0).all()

    def run(*args: object, code: int = 0) -> dict:
        result = subprocess.run([str(BINARY), *map(str, args), '--json'],
                                cwd=ROOT, capture_output=True, text=True)
        assert result.returncode == code, (args, result.returncode, result.stdout, result.stderr)
        return json.loads(result.stdout)

    with tempfile.TemporaryDirectory(prefix='caexfer-matrix-') as folder:
        folder = Path(folder)
        sources = {'bdf': ROOT / 'examples/plate.bdf',
                   'frd': ROOT / 'tests/fixtures/linear-results.frd'}
        for extension in ('vtu', 'vtk', 'msh', 'inp'):
            path = folder / f'source.{extension}'
            run('convert', sources['bdf'], path, '--accept-projection')
            sources[extension] = path
        msh22 = folder / 'source22.msh'
        run('convert', sources['bdf'], msh22, '--msh-version', '2.2', '--accept-projection')
        sources['msh22'] = msh22
        sources['pch'] = ROOT / 'tests/fixtures/pch-multiple.pch'
        if options.op2_check:
            sources['op2'] = ROOT / 'tests/fixtures/solid_bending.op2'
        routes = 0
        for source_format, source in sources.items():
            extra = []
            if source_format == 'op2':
                extra = ['--mesh', ROOT / 'tests/fixtures/solid_bending.bdf']
            elif source_format == 'pch':
                extra = ['--mesh', ROOT / 'tests/fixtures/pch-companion.bdf', '--subcase', '1']
            for target_format in ('bdf', 'vtu', 'vtk', 'msh', 'inp'):
                target = folder / f'{source_format}-to-{target_format}.{target_format}'
                report = run('convert', source, target, '--accept-projection', *extra)
                assert target.is_file() and report['points'] > 0 and report['cells'] > 0
                expected = (72, 186) if source_format == 'op2' else ((3, 1) if source_format == 'frd' else (2, 1) if source_format == 'pch' else (4, 1))
                assert (report['points'], report['cells']) == expected, (source_format, target_format, report)
                if target_format == 'bdf':
                    assert run('validate', target)['passed']
                else:
                    info = run('info', target)
                    assert (info['points'], info['cells']) == expected
                    expected_fields = 2 if source_format == 'frd' else (1 if source_format in ('op2', 'pch') else 0)
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
            dialect = folder / f'{source_format}-to-msh22.msh'
            report = run('convert', source, dialect, '--msh-version', '2.2',
                         '--accept-projection', *extra)
            assert dialect.read_text().startswith('$MeshFormat\n2.2 0 8\n')
            assert (report['points'], report['cells']) == expected
            assert run('info', dialect)['fields'] == (2 if source_format == 'frd' else
                                                     1 if source_format in ('op2', 'pch') else 0)
            routes += 1
            target = folder / f'{source_format}-to-frd.frd'
            report = run('convert', source, target, '--accept-projection', *extra)
            assert target.is_file() and report['points'] == expected[0]
            info = run('info', target)
            assert (info['points'], info['cells']) == expected
            assert info['fields'] == (2 if source_format == 'frd' else
                                      1 if source_format in ('op2', 'pch') else 0)
            routes += 1

            if source_format == 'frd':
                result = folder / 'frd-to-op2.op2'
                refused = run('convert', source, result, '--accept-projection', code=1)
                assert refused['error']['code'] == 'E_OP2' and not result.exists()
                assert 'unknown rotations' in refused['error']['message']

            if options.op2_check and source_format in ('frd', 'op2'):
                result = folder / f'{source_format}-to-op2.op2'
                if source_format == 'frd':
                    mesh = folder / 'frd-for-op2.bdf'
                    report = run('convert', source, result, '--accept-projection',
                                 '--zero-missing-rotations',
                                 '--mesh-out', mesh)
                    assert report['mesh_output']['path'] == str(mesh)
                    assert report['mesh_output']['format'] == 'bdf'
                    assert any('excluded from companion mesh' in item['detail']
                               for item in report['mesh_output']['omissions'])
                else:
                    report = run('convert', source, result, '--accept-projection', *extra)
                assert result.is_file() and report['fields'] >= 1
                independent_op2(result, expected_rows={
                    1: [0., 0., 0., 0., 0., 0.],
                    2: [0.1, 0., 0., 0., 0., 0.],
                    3: [0., 0.2, 0., 0., 0., 0.],
                } if source_format == 'frd' else None)
                mesh = (ROOT / 'tests/fixtures/solid_bending.bdf' if source_format == 'op2'
                        else folder / 'frd-for-op2.bdf')
                if source_format == 'frd':
                    assert mesh.is_file() and run('validate', mesh)['passed']
                    assert any('typed float 0.0' in item['detail'] for item in report['omissions'])
                reread = folder / f'{source_format}-op2-reread.vtu'
                run('convert', result, reread, '--mesh', mesh, '--accept-projection')
                piece = ET.parse(reread).find('./UnstructuredGrid/Piece')
                field = piece.find("./PointData/DataArray[@Name='DISPLACEMENT_SUBCASE_1']")
                assert field is not None
                first = [float(x) for x in field.text.split()[:6]]
                assert first[3:] == [0., 0., 0.]
                routes += 1
            elif source_format in ('bdf', 'inp', 'vtu', 'msh'):
                target = folder / f'{source_format}-to-op2.op2'
                result = run('convert', source, target, '--accept-projection', code=1)
                assert result['error']['code'] == 'E_OP2' and not target.exists()
                if options.op2_check and source_format in ('bdf', 'inp'):
                    report = run('convert', source, target, '--accept-projection',
                                 '--assume-zero-displacement')
                    assert target.is_file()
                    independent_op2(target, assumed_zero=True)
                    assert any('SYNTHETIC ASSUMPTION' in item['detail']
                               for item in report['omissions'])
                    mesh = (source if source_format == 'bdf'
                            else folder / 'inp-to-bdf.bdf')
                    reread = folder / f'{source_format}-zero-reread.vtu'
                    loaded = run('convert', target, reread, '--mesh', mesh, '--accept-projection')
                    assert any('synthetic all-zero' in item['detail']
                               for item in loaded['omissions'])
                    piece = ET.parse(reread).find('./UnstructuredGrid/Piece')
                    field = piece.find("./PointData/DataArray[@Name='DISPLACEMENT_SUBCASE_1']")
                    assert field is not None and all(float(x) == 0.0 for x in field.text.split())
                    if source_format == 'bdf':
                        rewritten = folder / 'synthetic-op2-rewritten.op2'
                        run('convert', target, rewritten, '--mesh', mesh, '--accept-projection')
                        reread_report = run('info', rewritten, '--mesh', mesh)
                        assert any('synthetic all-zero' in item for item in reread_report['omissions'])
                    routes += 1
        if options.op2_check:
            pch_mesh = ROOT / 'tests/fixtures/pch-companion.bdf'
            pch_op2 = folder / 'pch-to-op2.op2'
            run('convert', sources['pch'], pch_op2, '--mesh', pch_mesh,
                '--subcase', '1', '--accept-projection')
            independent_op2(pch_op2, expected_rows={
                10: [1., 2., 3., 4., 5., 6.],
                20: [7., 8., 9., 10., 11., 12.],
            })
            assert run('info', pch_op2, '--mesh', pch_mesh)['fields'] == 1
            routes += 1
            op2_source = sources['op2']
            op2_mesh = ROOT / 'tests/fixtures/solid_bending.bdf'
            for extension in ('vtu', 'msh', 'inp', 'frd'):
                companion = folder / f'op2-companion.{extension}'
                run('convert', op2_mesh, companion, '--accept-projection')
                destination = folder / f'op2-with-{extension}-mesh.vtu'
                refused = run('convert', op2_source, destination, '--mesh', companion,
                              '--accept-projection', code=2)
                assert refused['error']['code'] == 'E_USAGE' and not destination.exists()
                report = run('convert', op2_source, destination, '--mesh', companion,
                             '--assume-basic-frame',
                             '--accept-projection')
                assert (report['points'], report['cells'], report['fields']) == (72, 186, 1)
                assert any(item['stage'] == 'assumption' and 'basic-frame' in item['detail']
                           for item in report['omissions'])
                routes += 1
            enriched_mesh = folder / 'op2-enriched-companion.vtu'
            run('convert', op2_source, enriched_mesh, '--mesh', op2_mesh,
                '--accept-projection')
            enriched_result = folder / 'op2-with-enriched-mesh.vtu'
            enriched_report = run('convert', op2_source, enriched_result,
                                  '--mesh', enriched_mesh, '--assume-basic-frame',
                                  '--accept-projection')
            assert any('numeric field(s) in companion mesh ignored' in item['detail']
                       for item in enriched_report['omissions'])
            routes += 1
            mismatched = folder / 'op2-mismatched-mesh.vtu'
            refused = run('convert', op2_source, mismatched,
                          '--mesh', sources['vtu'], '--assume-basic-frame',
                          '--accept-projection', code=1)
            assert refused['error']['code'] == 'E_OP2' and not mismatched.exists()
            existing = folder / 'existing-companion.bdf'
            existing.write_text('already here')
            failed_pair = folder / 'no-partial-pair.op2'
            refused = run('convert', sources['frd'], failed_pair, '--mesh-out', existing,
                          '--zero-missing-rotations',
                          '--accept-projection', code=1)
            assert refused['error']['code'] == 'E_EXISTS' and not failed_pair.exists()
            assert existing.read_text() == 'already here'
            paired_op2 = folder / 'paired-with-vtu.op2'
            paired_vtu = folder / 'paired-mesh.vtu'
            paired_report = run('convert', sources['frd'], paired_op2,
                                '--mesh-out', paired_vtu, '--zero-missing-rotations',
                                '--accept-projection')
            assert paired_report['mesh_output']['format'] == 'vtu'
            assert run('info', paired_vtu)['fields'] == 0
            paired_readback = folder / 'paired-vtu-readback.msh'
            readback_report = run('convert', paired_op2, paired_readback,
                                  '--mesh', paired_vtu, '--assume-basic-frame',
                                  '--accept-projection')
            assert readback_report['fields'] == 1
            routes += 1
            mesh = folder / 'result-carrier-mesh.bdf'
            run('convert', sources['frd'], mesh, '--accept-projection')
            for carrier in ('vtu', 'msh'):
                enriched = folder / f'frd-fields.{carrier}'
                run('convert', sources['frd'], enriched, '--accept-projection')
                target = folder / f'{carrier}-fields-to-op2.op2'
                report = run('convert', enriched, target,
                             '--zero-missing-rotations', '--accept-projection')
                assert target.is_file()
                assert any('typed float 0.0' in item['detail'] for item in report['omissions'])
                assert run('info', target, '--mesh', mesh)['fields'] == 1
                routes += 1
        print(json.dumps({'routes_checked': routes,
                          'sources': sorted(sources)}))


if __name__ == '__main__':
    main()
