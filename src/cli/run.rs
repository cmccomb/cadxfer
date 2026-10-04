//! Dispatch CLI commands and map errors to process exit codes.

use std::ffi::OsString;
use std::io::Write;

use caexfer::core::Result;

use super::args::{Args, parse_args};
use super::common::emit;
use super::convert::run_convert;
use super::help;
use super::json::{object, quote};
use super::validate::run_validate;

/// Dispatch the parsed CLI command and return its process exit status.
/// Every format uses the same projected-dataset inspection path.
#[allow(clippy::too_many_lines)] // Command branches keep exit codes and output together.
fn dispatch(args: &Args) -> Result<u8> {
    if args.help_requested {
        emit(help::for_command(&args.command))?;
        return Ok(0);
    }
    // Commands without input return before any format or file selection.
    match args.command.as_str() {
        "help" => {
            emit(help::OVERVIEW)?;
            return Ok(0);
        }
        "version" => {
            emit(concat!("caexfer ", env!("CARGO_PKG_VERSION")))?;
            return Ok(0);
        }
        "formats" => {
            emit(
                "bdf  linear geometry subset, read/write\nvtu  ASCII XML mesh + numeric fields, read/write\nvtk  ASCII legacy unstructured grid + numeric fields, read/write\nmsh  ASCII 4.1/2.2 mesh + numeric fields, read/write (output defaults to 4.1)\ninp  flat mesh subset, read/geometry write\nfrd  ASCII mesh + nodal fields, read/write\nop2  32-bit real OUGV1 displacement, read/write; explicit synthetic-zero option; optional companion mesh\npch  ASCII real SORT1 displacement, read-only; matching mesh required\nstl  ASCII/binary triangle surface, binary write; no IDs or fields\nsu2  ASCII mesh and named boundary markers, read/write\nunv  ASCII 2411/2412 linear geometry, read/write; no pyramids\nexodus  NetCDF-3 classic mesh + complete scalar fields, read/write",
            )?;
            return Ok(0);
        }
        _ => {}
    }

    if args.command == "convert" {
        return run_convert(args);
    }
    run_validate(args)
}

/// Convert structured failures into human or JSON diagnostics and exit codes.
pub(crate) fn run() {
    // Retain JSON error formatting even when argument parsing itself fails.
    let raw: Vec<OsString> = std::env::args_os().skip(1).collect();
    let wants_json = raw
        .first()
        .is_some_and(|command| command == "validate" || command == "convert")
        && raw.iter().any(|arg| arg == "--json");
    let status = match parse_args(&raw).and_then(|args| dispatch(&args)) {
        Ok(status) => status,
        Err(error) => {
            let message = if wants_json {
                object([
                    ("schema_version", "1".into()),
                    (
                        "error",
                        object([
                            ("code", quote(error.code)),
                            ("message", quote(&error.message)),
                            (
                                "line",
                                error
                                    .line
                                    .map_or_else(|| "null".into(), |line| line.to_string()),
                            ),
                        ]),
                    ),
                ])
            } else {
                error.to_string()
            };
            if wants_json {
                let _ = emit(&message);
            } else {
                let _ = writeln!(std::io::stderr().lock(), "{message}");
            }

            // Usage failures use the conventional distinct CLI exit code.
            if error.code == "E_USAGE" { 2 } else { 1 }
        }
    };
    std::process::exit(i32::from(status));
}
