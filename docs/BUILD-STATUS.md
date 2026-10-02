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
- `cargo clippy --workspace --all-targets --all-features --locked --offline`
  completed with three nonfatal warnings: two `map_entry` suggestions and one
  `unnecessary_map_or` suggestion.
- `cargo doc --workspace --all-features --no-deps --locked --offline` passed.
- `python3 scripts/check_source.py` passed all seven source-package check
  groups.
- `python3 scripts/check_interop.py --build` passed its independent JSON,
  byte-preserving BDF, VTU XML, coordinate-edit, and no-overwrite checks.

`cargo fmt --all -- --check` did not pass: the packaged Rust source has not
been formatted with rustfmt. Formatting was not applied during the repository
import so the source remains comparable to the supplied package.

## Verification still needed

The GitHub Actions matrix covers Linux, macOS, Windows, and Rust 1.85. Its
results should be checked after the initial push. The minimum Rust version has
not been tested locally. The optional interoperability check using Python VTK
has not run, and the bundled synthetic fixtures do not establish broad
compatibility with vendor decks or solver results. This is a source candidate,
not a production-readiness claim. See [RELEASING.md](RELEASING.md) before any
registry publication or release tag.

## Original package evidence

The package includes the pre-import [source checks](source-checks.json),
[static review](static-review.json), and [rename checks](rename-checks.json).
Those records document the original authoring environment, which lacked a Rust
toolchain. The local execution results above supersede its unverified Rust
status; they do not establish cross-platform compatibility.
