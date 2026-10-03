# Test coverage assessment

The coverage workflow measures Rust source lines executed by the unit and
integration tests, independent CLI interoperability checks, and the full
conversion matrix with pyNastran. Its README badge reports whether that workflow
passes, including an **80% Rust line-coverage floor**. The run publishes
`coverage.json` and `coverage.lcov` as downloadable artifacts with the current
numbers.

## Baseline

Measured on 2026-10-03 with Rust 1.98.1, cargo-llvm-cov 0.8.7, and pyNastran
1.4.1:

| Measure | Covered / total | Coverage |
| --- | ---: | ---: |
| Rust source lines | 3,517 / 4,264 | 82.5% |
| Rust source regions | 5,564 / 6,686 | 83.2% |
| Rust functions | 287 / 401 | 71.6% |

The run executed 94 Rust tests, the independent CLI interoperability checks,
and 42 conversion routes across BDF, VTU, MSH, INP, FRD, and OP2. Doctests run
in the regular CI workflow but are not included in this coverage measurement.
The Python check scripts and pyNastran adapters are exercised but their Python
lines are not counted. Branch coverage is not reported by this setup; the line
percentage should not be interpreted as branch coverage or solver validation.

## Largest gaps

| Rust module | Line coverage | Next useful checks |
| --- | ---: | --- |
| `src/main.rs` | 69.5% | More CLI option combinations and error paths. |
| `src/op2.rs` | 76.7% | Malformed or unsupported OP2 results and Python process failures. |
| `src/bdf/mesh.rs` | 78.6% | Geometry writer edge cases and write failures. |
| `src/inp.rs` | 79.2% | Unsupported keywords and malformed mesh inputs. |

The remaining Rust modules range from 82.7% to 100% line coverage. These gaps
are priorities for future tests; the current suite already exercises the
advertised conversion routes through the CLI. Its fixtures are small, so they
do not establish compatibility with every real solver file.

## Reproduce

Install `cargo-llvm-cov` and the Rust toolchain's `llvm-tools-preview`
component, then use a Python interpreter with `pyNastran==1.4.1`:

```sh
bash scripts/coverage.sh python3
```

If Rust was installed through Homebrew rather than rustup, set `LLVM_COV` and
`LLVM_PROFDATA` to tools whose LLVM major version matches `rustc -vV` before
running the script.

The script writes `target/coverage.json` and `target/coverage.lcov` and fails
when line coverage falls below 80%. The assessment above is a dated baseline;
the latest workflow run contains the current measurement.
