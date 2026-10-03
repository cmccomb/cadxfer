# caexfer-formats

Feature-selected engineering format adapters. Features: `bdf`, `vtu`, `msh`,
`inp`, `frd`, `op2`, and `all-formats`. The OP2 adapter uses an installed
pyNastran interpreter when called; no Python package is needed to compile.
FRD supports documented ASCII mesh/nodal-field input and output. OP2 handles
one real displacement table in either direction; output from a three-component
displacement needs an explicit zero-rotation assertion and has no mesh.
The CLI can also create a labeled synthetic all-zero OP2 displacement table
from a result-free BDF or INP after `--assume-zero-displacement` is supplied.

Part of caexfer 0.1.0. See the repository README and docs/SUPPORT.md for the exact support boundary.
