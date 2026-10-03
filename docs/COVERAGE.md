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
| Rust source lines | 4,066 / 4,879 | 83.3% |
| Rust source regions | 6,504 / 7,782 | 83.6% |
| Rust functions | 336 / 466 | 72.1% |

The run executed 104 Rust tests, the independent CLI interoperability checks,
and 42 conversion routes across BDF, VTU, MSH, INP, FRD, and OP2. Doctests run
in the regular CI workflow but are not included in this coverage measurement.
The Python check scripts and test-only pyNastran invocation are exercised but
their Python lines are not counted. Branch coverage is not reported; the line
percentage should not be interpreted as branch coverage or solver validation.

## Largest gaps

| Rust module | Line coverage | Next useful checks |
| --- | ---: | --- |
| `src/main.rs` | 69.4% | More CLI option combinations and error paths. |
| `src/op2.rs` | 78.9% | More malformed field and mesh association cases. |
| `src/op2_binary.rs` | 88.4% | More malformed or unsupported OP2 records. |
| `src/bdf/mesh.rs` | 78.6% | Geometry writer edge cases and write failures. |

These gaps
are priorities for future tests; the current suite already exercises the
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
