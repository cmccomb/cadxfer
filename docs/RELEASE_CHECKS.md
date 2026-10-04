# Release candidate checks

Use the packaged crate as a consumer would, in addition to testing the
repository checkout. These checks apply to each candidate release.

1. Confirm the intended source commit, version, and a clean working tree.
2. Build the `.crate` package and extract it into a fresh directory. Build and
   install the CLI from those extracted files, then exercise `--formats`,
   `validate`, and a representative `convert` route using package contents.
   This catches examples or fixtures that work only from the repository.
3. Run unit, integration, doctest, documentation, formatting, Clippy, package,
   coverage, and independent interoperability checks. Review any known output
   recovery limitations and the support-contract wording before release.
4. Confirm the supported-platform CI matrix and coverage workflow pass for the
   exact source commit. Publish the crate, then verify its recorded source
   commit before tagging and creating the matching GitHub release.

For the first update after 0.1.0, prioritize recoverable paired-output failure
handling and redistributable, independently produced solver fixtures. Keep
their provenance and external-reader comparisons with the relevant tests.
