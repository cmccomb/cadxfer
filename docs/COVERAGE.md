# Test coverage assessment

The coverage workflow measures Rust source lines executed by unit and
integration tests, independent CLI checks, and the conversion matrix with
pyNastran as an independent OP2 reader. It enforces a
**95% distinct source-line coverage floor**. Each physical Rust file/line pair
in the LCOV `DA` records counts once, even when a generic function has multiple
compiled instantiations. The raw LLVM JSON summary remains available and may
show a different percentage because it counts those instantiations. The README
badge shows the distinct source-line result from the latest run on `main`.
The workflow retains `coverage-source-lines.json`, `coverage.json`, and `coverage.lcov`
workflow artifacts. If a run fails before producing a report, the badge
retains the last measurement. Use its link to inspect workflow status.

Doctests run in regular CI but are outside this measurement. Python checks
are exercised, but their lines are not counted. Line coverage does not
establish branch coverage or solver validation.

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

The script writes `target/coverage-source-lines.json`, `target/coverage.json`,
and `target/coverage.lcov` and fails when distinct source-line coverage falls
below 95%. The latest workflow run contains the current measurement.
