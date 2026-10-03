# Publishing caexfer

Version `0.1.0` is the first planned crates.io release. Publishing is a manual
step because a registry version cannot be replaced. Do not tag a candidate as
published until the registry confirms the upload.

1. Confirm the working tree is clean and `main` contains the intended release.
2. Confirm CI and Coverage passed for that exact commit; Coverage must report at
   least 90% Rust source-line coverage.
3. Run `cargo publish --dry-run` without `--allow-dirty`. Check the packaged
   file list with `cargo package --list` and inspect the README, license files,
   source files, and fixture provenance included in the archive.
4. Confirm the `caexfer` name and version are still available on crates.io, and
   confirm the publishing account has a verified email and a suitable token.
5. Publish with `cargo publish`, then independently confirm that version
   `0.1.0` appears on crates.io and docs.rs. If the upload times out, inspect
   the registry before retrying.
6. Tag the published commit `v0.1.0` and push the tag only after confirmation.

The [support contract](docs/SUPPORT.md) defines the release's format scope.
The [changelog](CHANGELOG.md) summarizes user-visible behavior.
