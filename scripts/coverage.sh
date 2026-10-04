#!/usr/bin/env bash
# Measure Rust line coverage across tests and independent CLI checks.
# Usage: bash scripts/coverage.sh PYTHON_WITH_PYNASTRAN
# Run from the repository root after installing cargo-llvm-cov. The interpreter
# is used only to verify Rust-written OP2 files; the crate has no Python runtime.
# Build artifacts and raw profiles use a temporary Cargo target directory.
# Reports are written to target/coverage.{lcov,json}; exit nonzero below 90%.
set -euo pipefail

if (($# != 1)); then
    echo "usage: bash scripts/coverage.sh python-with-pynastran" >&2
    exit 2
fi

# Isolate instrumented binaries and .profraw files from normal Cargo builds.
# Clean only this script-owned directory when the script exits.
coverage_tmp=$(mktemp -d "${TMPDIR:-/tmp}/caexfer-coverage.XXXXXX")
coverage_target="$coverage_tmp/target"
coverage_env="$coverage_tmp/env.sh"
cleanup() {
    cargo clean --target-dir "$coverage_target" >/dev/null 2>&1 || true
    rm -f "$coverage_env"
    rmdir "$coverage_tmp" 2>/dev/null || true
}
trap cleanup EXIT
export CARGO_TARGET_DIR="$coverage_target"

# Source cargo-llvm-cov's instrumentation settings so tests and external
# readers contribute to one coverage report in the temporary target.
cargo llvm-cov show-env --sh >"$coverage_env"
# The generated file contains the instrumentation exports for this shell.
# shellcheck disable=SC1090
source "$coverage_env"
# Rust tests, the executable, and independent format checks all use that build.
cargo test --all-targets --offline
cargo build --offline
coverage_binary="$coverage_target/debug/caexfer"
python3 scripts/check_interop.py --binary "$coverage_binary"
"$1" scripts/check_matrix.py --binary "$coverage_binary" --op2-check

# Produce both reviewable reports before enforcing the line-coverage gate.
mkdir -p target
cargo llvm-cov report --lcov --output-path target/coverage.lcov
cargo llvm-cov report --json --output-path target/coverage.json
cargo llvm-cov report --fail-under-lines 90
