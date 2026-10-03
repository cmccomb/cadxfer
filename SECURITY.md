# Handling untrusted files

`caexfer` uses safe Rust and bounded input reads, but is not a sandbox or a
strict total-memory budget. OP2 parsing runs in Rust and rejects unsupported
record layouts; use OS process limits when processing hostile or very large files.

Report vulnerabilities privately through the repository's
[Security Advisories](https://github.com/cmccomb/caexfer/security/advisories)
page using **Report a vulnerability**.
