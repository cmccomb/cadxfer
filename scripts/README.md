# Repository scripts

Run these commands from the repository root. Python is used for test tooling;
the `caexfer` library and CLI have no Python runtime dependency.

| Script | Purpose | Required input | Output or failure |
| --- | --- | --- | --- |
| `coverage.sh` | Measure Rust line coverage across tests and independent CLI checks | One Python interpreter with pyNastran, plus `cargo-llvm-cov` | Builds and writes raw profiles in a temporary directory; keeps `target/coverage.json` and `.lcov`; fails below 90% |
| `coverage_badge.sh` | Render the measured line percentage as a badge | Coverage JSON and output SVG path; `jq`, `awk` | Rejects invalid totals; colors values below 90% red |
| `check_interop.py` | Compare BDF-to-VTU CLI output with hand-authored geometry expectations | Built CLI (`--binary` may override its path); optional VTK for `--vtk` | Prints passed checks; fails on differing bytes, IDs, topology, or exit status |
| `check_matrix.py` | Exercise mesh and results routes through the CLI | Built CLI (`--binary` may override its path); pyNastran for `--op2-check` | Prints the route count; fails on missing data, omissions, or frame checks |
| `check_vtk.py` | Verify Rust-written legacy VTK with VTK's own reader and writer | Built CLI and Python VTK bindings | Prints success; fails on external read or round trip |

The check scripts create temporary files and remove them when they finish.
`coverage.sh` keeps only its reports under `target/`. Cargo also directs raw
profiles from instrumented local runs to the system temporary directory.
