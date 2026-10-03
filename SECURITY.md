# Handling untrusted files

`caexfer` uses safe Rust and bounded input reads, but is not a sandbox or a
strict total-memory budget. OP2 operations explicitly run the selected Python interpreter and
pyNastran, so use an interpreter and input files you trust.

Report vulnerabilities privately through the repository's
[Security Advisories](https://github.com/cmccomb/caexfer/security/advisories)
page using **Report a vulnerability**.
