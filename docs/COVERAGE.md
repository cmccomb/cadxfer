# Test coverage assessment

The coverage workflow measures Rust source lines executed by the unit and
integration tests, independent CLI interoperability checks, and the full
conversion matrix with pyNastran as an independent OP2 reader check. The README badge displays the measured Rust
line percentage from the latest `main` run that produced a report. The workflow
also enforces an **80% Rust line-coverage floor**. Each report, including one
below that floor, updates the badge and is available as downloadable
`coverage.json` and `coverage.lcov` artifacts. If a run fails before producing a
report, the badge retains the last measurement; follow its link to inspect the
workflow status.

## Baseline

Measured on 2026-10-03 with Rust 1.98.1, cargo-llvm-cov 0.8.7, and pyNastran
1.4.1:

| Measure | Covered / total | Coverage |
| --- | ---: | ---: |
| Rust source lines | 5,170 / 6,185 | 83.6% |
| Rust source regions | 8,360 / 10,012 | 83.5% |
| Rust functions | 408 / 581 | 70.2% |

The run executed 118 Rust tests, the independent CLI interoperability checks,
and 76 conversion routes across BDF, VTU, legacy VTK, MSH 4.1/2.2, INP, FRD,
OP2, and PCH. Doctests run
in the regular CI workflow but are not included in this coverage measurement.
The Python check scripts and test-only pyNastran invocation are exercised but
their Python lines are not counted. Branch coverage is not reported; the line
percentage should not be interpreted as branch coverage or solver validation.

## Largest gaps

| Rust module | Line coverage | Next useful checks |
| --- | ---: | --- |
| `src/main.rs` | 70.6% | More CLI option combinations and error paths. |
| `src/nastran_results/op2.rs` | 78.9% | More malformed field and mesh association cases. |
| `src/nastran_results/op2_binary.rs` | 88.3% | More malformed or unsupported OP2 records. |
| `src/bdf/mesh.rs` | 78.6% | Geometry writer edge cases and write failures. |
| `src/mesh_formats/vtk.rs` | 84.1% | More malformed legacy attribute layouts. |
| `src/mesh_formats/msh.rs` | 86.5% | More unusual 2.2 tag and field layouts. |
| `src/nastran_results/pch.rs` | 81.7% | More independent solver-produced punch files and malformed header cases. |

These gaps are priorities for future tests; the current suite already exercises the
advertised conversion routes through the CLI. Its fixtures are small, so they
do not establish compatibility with every real solver file.

## Reproduce

Install `cargo-llvm-cov`, `jq`, and the Rust toolchain's `llvm-tools-preview`
component, then use a test-only Python interpreter with `pyNastran==1.4.1`
for independent OP2 output checks:

```sh
bash scripts/coverage.sh python3
```

If Rust was installed through Homebrew rather than rustup, set `LLVM_COV` and
`LLVM_PROFDATA` to tools whose LLVM major version matches `rustc -vV` before
running the script.

The script writes `target/coverage.json` and `target/coverage.lcov` and fails
when line coverage falls below 80%. The assessment above is a dated baseline;
the latest workflow run contains the current measurement.
