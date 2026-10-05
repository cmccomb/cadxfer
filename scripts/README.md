# Repository scripts

Run these commands from the repository root. Python is used for test tooling;
the `caexfer` library and CLI have no Python runtime dependency.

| Script | Purpose | Required input | Output or failure |
| --- | --- | --- | --- |
| `coverage.sh` | Measure Rust line coverage across tests and independent CLI checks | One Python interpreter with pyNastran, plus `cargo-llvm-cov` | Builds and writes raw profiles in a temporary directory; keeps raw JSON, LCOV, and a distinct source-line summary; fails below 95% |
| `source_line_coverage.py` | Count each Rust source file/line once from LCOV | LCOV report and summary JSON path | Rejects malformed or empty reports; enforces the 95% floor |
| `coverage_badge.sh` | Render the measured source-line percentage as a badge | Distinct source-line summary JSON and output SVG path; `jq`, `awk` | Rejects invalid totals; colors values below 95% red |
| `check_interop.py` | Compare BDF-to-VTU CLI output with hand-authored geometry expectations | Built CLI (`--binary` may override its path); optional VTK for `--vtk` | Prints passed checks; fails on differing bytes, IDs, topology, or exit status |
| `check_matrix.py` | Exercise mesh and results routes through the CLI | Built CLI (`--binary` may override its path); pyNastran for `--op2-check` | Prints the route count; fails on missing data, omissions, or frame checks |
| `check_vtk.py` | Verify Rust-written legacy VTK and VTI with VTK's own readers and writers | Built CLI and Python VTK bindings | Prints success; fails on external read or round trip |

The check scripts create temporary files and remove them when they finish.
`coverage.sh` keeps only its reports under `target/`. Cargo also directs raw
profiles from instrumented local runs to the system temporary directory.
