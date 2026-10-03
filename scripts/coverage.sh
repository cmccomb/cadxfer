#!/usr/bin/env bash
# Measure Rust coverage across unit tests and the independent CLI checks.
set -euo pipefail

if (( $# != 1 )); then
    echo "usage: bash scripts/coverage.sh python-with-pynastran" >&2
    exit 2
fi

# Keep the same instrumented binary for Rust tests and independent CLI checks.
coverage_env=$(mktemp)
trap 'rm -f "$coverage_env"' EXIT
cargo llvm-cov show-env --sh > "$coverage_env"
source "$coverage_env"
cargo llvm-cov clean --workspace
cargo test --all-targets --offline
cargo build --offline
python3 scripts/check_interop.py
"$1" scripts/check_matrix.py --op2-check

cargo llvm-cov report --lcov --output-path target/coverage.lcov
cargo llvm-cov report --json --output-path target/coverage.json
cargo llvm-cov report --fail-under-lines 80
