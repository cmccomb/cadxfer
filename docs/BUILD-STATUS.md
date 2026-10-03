# Build and verification status

**Version:** 0.1.0 source repository

**Record date:** October 2, 2026

**Registry publication:** Not published to crates.io

**Compiled binaries:** None included

## Local verification on macOS

The original source package was assembled without a Rust toolchain. After
importing it into this repository, the following checks ran successfully with
Cargo 1.98.1 on macOS:

- `cargo test --workspace --all-features --locked --offline` — 92 unit and
  integration tests and one doctest passed.
- All three `caxifer-formats` feature-isolation checks in the README passed.
- `cargo fmt --all -- --check` passed after formatting the supplied Rust source.
- `cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings`
  passed after resolving three lints in the original source.
- `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps --locked --offline`
  passed.
- `python3 scripts/check_source.py` passed all seven source-package check
  groups.
- `python3 scripts/check_interop.py --build` passed its independent JSON,
  byte-preserving BDF, VTU XML, coordinate-edit, and no-overwrite checks.

## cadxfer rename verification on macOS

After renaming the workspace packages and narrowing the BDF public API,
`cargo test --workspace --all-features --locked --offline` passed 92 Rust tests
and one doctest. The three `cadxfer-formats` feature-isolation checks,
`cargo fmt --all -- --check`, Clippy with warnings denied, rustdoc with warnings
denied, the source-package check, the independent CLI interoperability check,
and the generated-diagram check also passed. The [renamed repository's CI run](https://github.com/cmccomb/cadxfer/actions/runs/37080719854)
passed all four jobs: stable Rust on Linux, macOS, and Windows, plus Rust 1.85.0
on Linux.

## Cross-platform CI

The [initial GitHub Actions run](https://github.com/cmccomb/caxifer/actions/runs/37034376490)
passed all four jobs: stable Rust on Linux, macOS, and Windows, plus Rust 1.85.0
on Linux. It ran tests, feature checks, Clippy, rustdoc, source checks, and CLI
interoperability checks. Later repository changes should be checked against
their own CI runs.

## Verification still needed

The optional interoperability check using Python VTK has not run, and the
bundled synthetic fixtures do not establish broad compatibility with vendor
decks or solver results. This is a source candidate, not a production-readiness
claim. See [RELEASING.md](RELEASING.md) before any registry publication or
release tag.

## Original package evidence

The package includes the pre-import [source checks](source-checks.json),
[static review](static-review.json), and [rename checks](rename-checks.json).
Those records document the original authoring environment, which lacked a Rust
toolchain. The local and CI execution results above supersede its unverified
Rust status. The Rust source has since been formatted and three lints fixed.
