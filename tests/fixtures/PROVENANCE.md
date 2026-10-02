# Fixture provenance

All files in this directory and `examples/` were authored specifically for this
package. No vendor models, proprietary engineering data, or copied third-party
regression corpora are included. They are released under the package license.

`mixed-linear.expected.json` records independent, explicit topology expectations
for `mixed-linear.bdf`; it is not output claimed to come from a Rust execution.
The cells overlap on purpose. They are an I/O fixture, not a simulation model.
`mixed-newlines.bdf` deliberately contains a non-UTF-8 comment byte.
