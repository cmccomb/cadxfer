# Before publishing 0.1.0

This archive is a source package. Nothing has been uploaded to crates.io or a
GitHub repository, no registry names have been reserved, and no CI run has been
claimed. Do not run `cargo publish` merely because package versions say 0.1.0.

## Required gates

Run Rust compilation, the complete test suite and examples, all feature
combinations, Clippy, rustdoc, and the independent CLI interoperability script.
Run on Linux, macOS, and Windows and on the intended Rust 1.85 minimum. Run
`cargo fmt --all`, review the diff, then enforce `cargo fmt --all --check` before
publishing. The authoring environment could not execute Rust formatting either.

Read BUILD-STATUS.md and replace the unverified status only with recorded real
command outputs. Add real-world, redistributable decks and compare against
pyNastran/solver behavior before widening the compatibility statement.
Benchmark before claiming performance or memory advantages. A 0.1 experimental
release can retain its narrow scope, but must pass its own tests.

Confirm license/authorship details, package-name availability, and repository
ownership. Set real `repository` and `homepage` metadata only after creating the
repository; this source package intentionally contains no invented URLs.
Check that every packaged crate includes both licenses and its README.

## Packaging

```sh
python3 scripts/check_source.py
python3 scripts/package_source.py
```

This creates ZIP/tar.gz source archives and SHA256SUMS under `dist/`. It performs
no external writes. Cargo's per-crate package operation is a separate gate.
Inspect those archives using `cargo package --list -p PACKAGE` and dry-run the
publication in dependency order. Path dependencies also specify version 0.1.0;
registry publication replaces the local paths with registry dependencies.

## Publication order, after the gates

`caxifer-core` → `caxifer-bdf` and `caxifer-vtu` → `caxifer-formats` → `caxifer`.
Wait for each published dependency to be visible in the registry before
packaging its dependents. Before the dependencies exist there, a downstream
`cargo publish --dry-run` may fail for registry resolution, not source-code
correctness. Do not bypass verification to conceal that distinction.

A release tag, registry publication, and release binaries require explicit
maintainer action. There is intentionally no automatic publishing workflow or
registry token configuration in this package.
