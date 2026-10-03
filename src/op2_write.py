"""Write and independently reread one real displacement OP2 table.

Input is a tab-separated stream from the Rust adapter. Binary OP2 goes to
stdout only after pyNastran verifies the table and node IDs.
"""
import contextlib
import math
import pathlib
import sys
import tempfile

import numpy as np
from pyNastran.op2.op2 import OP2, read_op2
from pyNastran.op2.tables.oug.oug_displacements import RealDisplacementArray

header = sys.stdin.readline().rstrip('\n').split('\t')
if len(header) != 5:
    raise ValueError('invalid OP2 writer header')
count, subcase, time_text, components, assumed_zero_text = header
count, subcase, components = int(count), int(subcase), int(components)
if assumed_zero_text not in ('0', '1'):
    raise ValueError('invalid assumed-zero flag')
assumed_zero = assumed_zero_text == '1'
if count <= 0 or components not in (3, 6):
    raise ValueError('OP2 writer requires complete 3- or 6-component nodal displacements')
ids = np.empty((count, 2), dtype=np.int32)
values = np.zeros((1, count, 6), dtype=np.float32)
for index in range(count):
    row = sys.stdin.readline().rstrip('\n').split('\t')
    if len(row) != components + 1:
        raise ValueError(f'incomplete OP2 displacement row {index + 1}')
    ids[index] = (int(row[0]), 1)
    for component, text in enumerate(row[1:]):
        value = float(text)
        if not math.isfinite(value):
            raise ValueError('nonfinite OP2 displacement')
        values[0, index, component] = value
if sys.stdin.read().strip():
    raise ValueError('extra OP2 displacement rows')
if len(set(ids[:, 0])) != count:
    raise ValueError('duplicate OP2 node ID')

title = ('CAEXFER ASSUMED ZERO DISPLACEMENT - NOT SOLVER RESULTS'
         if assumed_zero else '')
if time_text == '-':
    table = RealDisplacementArray.add_static_case('OUGV1', ids, values, subcase,
                                                   title=title)
else:
    time = float(time_text)
    if not math.isfinite(time) or not math.isfinite(np.float32(time)):
        raise ValueError('OP2 time exceeds float32 range')
    table = RealDisplacementArray.add_transient_case(
        'OUGV1', ids, values, subcase, np.array([time], dtype=np.float32),
        title=title)

with tempfile.TemporaryDirectory(prefix='caexfer-op2-') as directory:
    path = pathlib.Path(directory) / 'result.op2'
    with contextlib.redirect_stdout(sys.stderr):
        model = OP2(mode='msc', debug=False)
        model.displacements[subcase] = table
        model.write_op2(str(path), nastran_format='msc')
        check = read_op2(str(path), build_dataframe=False, debug=False,
                         include_results=['displacements'])
    actual = check.displacements[subcase]
    if not np.array_equal(actual.node_gridtype, ids) or not np.array_equal(actual.data, values):
        raise ValueError('OP2 reread disagrees with encoded displacements')
    if assumed_zero and 'ASSUMED ZERO DISPLACEMENT' not in actual.title:
        raise ValueError('OP2 reread lost assumed-zero provenance title')
    sys.stdout.buffer.write(path.read_bytes())
