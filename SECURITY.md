# Handling untrusted files

This initial source package has not completed a security audit or fuzz campaign.
It uses safe Rust, bounded input reads, explicit parser limits, and no automatic
INCLUDE traversal or external process invocation. These measures do not make it
a sandbox or impose a strict total-memory budget.

Run unknown/large files with operating-system resource limits and ordinary
unprivileged permissions. Use a trusted destination directory for output.
A malformed file must produce an error, not a guessed engineering model.

The project does not yet have a published repository or security-reporting
endpoint. Before public distribution, establish a real private reporting route
and replace this paragraph; no fictional contact or URL is supplied here.
