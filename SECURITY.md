# Handling untrusted files

This initial source package has not completed a security audit or fuzz campaign.
It uses safe Rust, bounded input reads, explicit parser limits, and no automatic
INCLUDE traversal or external process invocation. These measures do not make it
a sandbox or impose a strict total-memory budget.

Run unknown/large files with operating-system resource limits and ordinary
unprivileged permissions. Use a trusted destination directory for output.
A malformed file must produce an error, not a guessed engineering model.

Report vulnerabilities privately through the repository's
[Security Advisories](https://github.com/cmccomb/cadxfer/security/advisories)
page using **Report a vulnerability**. Please avoid opening a public issue
before the maintainers have had a chance to review the report.
