#!/usr/bin/env bash
# Measure Rust line coverage across tests and independent CLI checks.
# Usage: bash scripts/coverage.sh PYTHON_WITH_PYNASTRAN
# Run from the repository root after installing cargo-llvm-cov. The interpreter
# is used only to verify Rust-written OP2 files; the crate has no Python runtime.
# Reports are written to target/coverage.{lcov,json}; exit nonzero below 90%.
set -euo pipefail

if (($# != 1)); then
    echo "usage: bash scripts/coverage.sh python-with-pynastran" >&2
    exit 2
fi

# Source cargo-llvm-cov's instrumentation settings so every command contributes
# to the same coverage report. Remove the temporary settings file on all exits.
coverage_env=$(mktemp)
trap 'rm -f "$coverage_env"' EXIT
cargo llvm-cov show-env --sh >"$coverage_env"
# The generated file contains the instrumentation exports for this shell.
# shellcheck disable=SC1090
source "$coverage_env"
cargo llvm-cov clean --workspace
# Rust tests, the executable, and independent format checks all use that build.
cargo test --all-targets --offline
cargo build --offline
python3 scripts/check_interop.py
"$1" scripts/check_matrix.py --op2-check

# Produce both reviewable reports before enforcing the line-coverage gate.
cargo llvm-cov report --lcov --output-path target/coverage.lcov
cargo llvm-cov report --json --output-path target/coverage.json
cargo llvm-cov report --fail-under-lines 90
