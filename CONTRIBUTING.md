# Contributing

Treat original engineering files as user data, not expendable parser input.
A format addition should provide a focused module, a documented read/write
support matrix, source-preservation tests where promised, explicit unsupported
cases, a redistributable fixture with provenance, and downstream-reader checks.

Do not add a supported-format badge for a stub or a file-header detector. Do not
silently skip unfamiliar geometry, invent units, downgrade higher-order cells,
round IDs through floats, or broaden `validate` into a claim the implementation
cannot establish. Update docs/SUPPORT.md and the CLI `formats` output together.

Keep generated outputs and target/ out of commits. Run the commands in README,
format with rustfmt, and preserve byte-sensitive fixtures using .gitattributes.
Proprietary customer models must not be added to the public test corpus without
permission. Synthetic reduced reproductions are preferred.
