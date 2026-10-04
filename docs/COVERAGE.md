# Test coverage assessment

The coverage workflow measures Rust source lines executed by the unit and
integration tests, independent CLI interoperability checks, and the full
conversion matrix with pyNastran as an independent OP2 reader check. The README badge displays the measured Rust
line percentage from the latest `main` run that produced a report. The workflow
also enforces a **90% Rust line-coverage floor**. Each report, including one
below that floor, updates the badge and is available as downloadable
`coverage.json` and `coverage.lcov` artifacts. If a run fails before producing a
report, the badge retains the last measurement; follow its link to inspect the
workflow status.

## Baseline

Measured on 2026-10-04 with Rust 1.99.0, cargo-llvm-cov 0.8.7, and pyNastran
1.4.1:

| Measure | Covered / total | Coverage |
| --- | ---: | ---: |
| Rust source lines | 8,119 / 8,999 | 90.22% |
| Rust source regions | 12,822 / 14,564 | 88.04% |
| Rust functions | 627 / 865 | 72.49% |

The run executed 142 Rust tests, the independent CLI interoperability checks,
and 76 conversion routes across BDF, VTU, legacy VTK, MSH 4.1/2.2, INP, FRD,
OP2, and PCH. Doctests run
in the regular CI workflow but are not included in this coverage measurement.
The Python check scripts and test-only pyNastran invocation are exercised but
their Python lines are not counted. Branch coverage is not reported; the line
percentage should not be interpreted as branch coverage or solver validation.

## Largest gaps

| Rust module | Line coverage | Next useful checks |
| --- | ---: | --- |
| `src/conversion/write.rs` | 65.9% | Failure paths for destination-specific projections and output streams. |
| `src/core/field.rs` | 58.6% | Structural failures for numeric fields. |
| `src/cli/output.rs` | 84.6% | Staged output failures and cleanup paths. |
| `src/formats/solver_inputs/inp.rs` | 85.0% | More malformed section and generated mesh records. |
| `src/formats/companion_results/op2/binary.rs` | 88.3% | More malformed or unsupported OP2 records. |

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
when line coverage falls below 90%. The assessment above is a dated baseline;
the latest workflow run contains the current measurement.
