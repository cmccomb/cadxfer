# Build and verification status

**Version:** 0.1.0 source repository

**Record date:** October 2, 2026

**Registry publication:** Not published to crates.io

**Compiled binaries:** None included

## Single-package refactor on macOS

The five-crate workspace was consolidated into one dependency-free `caexfer`
package. The 101 Rust unit and integration tests, two example targets, and one
doctest passed. Formatting, Clippy with warnings denied, rustdoc with warnings
denied, the source-package check, generated README diagram, independent CLI
interoperability check, and 25 routes without optional Python passed. With
pyNastran 1.4.1, the full matrix passed 36 routes and six byte-identical native
copies. `cargo package --allow-dirty --locked --offline` verified a 66-file
source package.

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

## caexfer rename verification on macOS

The subsequent `caexfer` namespace change passed the same local Rust test,
feature-isolation, formatting, Clippy, rustdoc, source-package, diagram, and CLI
interoperability checks. The [caexfer CI run](https://github.com/cmccomb/caexfer/actions/runs/37081354637)
passed all four Linux, macOS, and Windows jobs, including Rust 1.85.0 on Linux.

## Expanded conversion matrix verification on macOS

The six-format work passed 95 workspace Rust tests and one doctest, source-package check,
formatting and Clippy. `scripts/check_matrix.py` exercised 20 mesh routes and
five native copies. With pyNastran 1.4.1 in Python 3.12, it exercised all 24
mesh routes and six native copies using a real upstream OP2/BDF pair. The
emitted MSH, VTU and INP files were also read by independent meshio 5.3.5.
The [GitHub CI run for the expanded matrix](https://github.com/cmccomb/caexfer/actions/runs/37084246758)
passed all four jobs: stable Rust on Linux, macOS and Windows, plus Rust 1.85.0
on Linux. CI runs the 20 routes that need no optional Python package; the four
OP2 routes were checked locally with pyNastran.

## FRD/OP2 output extension on macOS

The new FRD long-format ASCII writer and OP2 real displacement writer passed
the workspace tests and matrix script. Without optional Python, the script
checked 25 routes and five byte-identical native copies. With pyNastran 1.4.1
in Python 3.12, it checked 34 routes and six native copies, including FRD,
VTU, and MSH three-component `DISP` fields converted to OP2 with explicitly
asserted float `0.0` rotations. A later extension exercised 36 routes, adding
explicit BDF/INP-to-OP2 all-zero assumptions, OP2 title provenance, and a
synthetic OP2 rewrite that retained the title. Generated OP2 files were reread
through pyNastran and matched to
separate BDF geometry. Generated FRD files were reread by caexfer; an
independent CalculiX GraphiX reader check remains to be done.

## Cross-platform CI

The [initial GitHub Actions run](https://github.com/cmccomb/caxifer/actions/runs/37034376490)
passed all four jobs: stable Rust on Linux, macOS, and Windows, plus Rust 1.85.0
on Linux. It ran tests, feature checks, Clippy, rustdoc, source checks, and CLI
interoperability checks. Later repository changes should be checked against
their own CI runs.

## Verification still needed

The optional interoperability check using Python VTK has not run. One real
OP2/BDF fixture plus synthetic FRD and mesh fixtures do not establish broad
compatibility with vendor decks or solver results. This is a source candidate, not a production-readiness
claim. See [RELEASING.md](RELEASING.md) before any registry publication or
release tag.

## Original package evidence

The package includes the pre-import [source checks](source-checks.json),
[static review](static-review.json), and [rename checks](rename-checks.json).
Those records document the original authoring environment, which lacked a Rust
toolchain. The local and CI execution results above supersede its unverified
Rust status. The Rust source has since been formatted and three lints fixed.
