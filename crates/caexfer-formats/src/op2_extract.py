"""pyNastran-backed extraction of one real OP2 displacement table.

The Rust caller supplies the mesh, subcase, and zero-based result step.
"""
import contextlib
import math
import sys

from pyNastran.op2.op2 import read_op2

path, subcase_text, step_text = sys.argv[1:4]
with contextlib.redirect_stdout(sys.stderr):
    model = read_op2(path, build_dataframe=False, debug=False,
                     include_results=['displacements'])
keys = list(model.displacements)
if not keys:
    raise ValueError('OP2 contains no displacement table')
if subcase_text == '-':
    if len(keys) != 1:
        raise ValueError('multiple displacement subcases; select --subcase')
    key = keys[0]
else:
    key = int(subcase_text)
    if key not in model.displacements:
        raise ValueError(f'subcase {key} has no displacement table')
disp = model.displacements[key]
if disp.is_complex:
    raise ValueError('complex displacement tables are unsupported')
if disp.data.ndim != 3 or disp.data.shape[2] != 6:
    raise ValueError('expected six displacement components')
if step_text == '-':
    if disp.data.shape[0] != 1:
        raise ValueError('multiple result steps; select --step (zero-based)')
    step = 0
else:
    step = int(step_text)
if step < 0 or step >= disp.data.shape[0]:
    raise ValueError('result step is out of range')
time = float(disp._times[step])
if not math.isfinite(time):
    time = 0.0
assumed_zero = 'CAEXFER ASSUMED ZERO DISPLACEMENT' in str(disp.title)
if assumed_zero and not (disp.data[step] == 0).all():
    raise ValueError('OP2 assumed-zero title conflicts with nonzero displacement data')
print(f'OK\t{len(disp.node_gridtype)}\t{key}\t{step}\t{time:.17g}\t{int(assumed_zero)}')
for (node, grid_type), values in zip(disp.node_gridtype, disp.data[step]):
    if int(grid_type) != 1:
        raise ValueError(f'non-GRID result entity {node} has type {grid_type}')
    if not all(math.isfinite(float(v)) for v in values):
        raise ValueError(f'nonfinite result for GRID {node}')
    print(str(int(node)) + '\t' + '\t'.join(f'{float(v):.17g}' for v in values))
