//! Parse command-line syntax and validate option combinations.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use caexfer::{Error, Format, MshVersion, Result};

/// Parsed CLI command, paths, and explicitly supplied option flags.
#[derive(Debug, Default)]
#[allow(clippy::struct_excessive_bools)] // CLI switches are independent user flags.
pub(super) struct Args {
    pub(super) command: String,
    pub(super) help_requested: bool,
    pub(super) paths: Vec<PathBuf>,
    pub(super) json: bool,
    pub(super) strict: bool,
    pub(super) overwrite: bool,
    pub(super) accept_omissions: bool,
    pub(super) accept_basic_frame: bool,
    pub(super) accept_zero_rotations: bool,
    pub(super) accept_synthetic_zero: bool,
    pub(super) accept_all: bool,
    pub(super) from: Option<String>,
    pub(super) mesh: Option<PathBuf>,
    pub(super) mesh_out: Option<PathBuf>,
    pub(super) subcase: Option<i64>,
    pub(super) step: Option<usize>,
    pub(super) max_bytes: Option<usize>,
    pub(super) msh_version: Option<MshVersion>,
}

/// Wrap a command-line usage failure with the exit-code-selecting `E_USAGE`.
pub(super) fn usage(message: impl Into<String>) -> Error {
    Error::new("E_USAGE", message)
}

/// Require UTF-8 for option syntax while leaving file paths as `OsString`.
fn text(arg: &OsStr) -> Result<&str> {
    arg.to_str()
        .ok_or_else(|| usage("option value is not valid UTF-8"))
}

/// Set a boolean option exactly once; duplicates indicate likely CLI mistakes.
fn set_flag(slot: &mut bool, name: &str) -> Result<()> {
    // Repeated flags are treated as user errors instead of silently accepted.
    if *slot {
        return Err(usage(format!("duplicate {name}")));
    }
    *slot = true;
    Ok(())
}

/// Parse and set one valued option, rejecting a repeated spelling.
/// The closure runs only after the duplicate check passes.
fn set_option<T>(
    slot: &mut Option<T>,
    name: &str,
    parse: impl FnOnce() -> Result<T>,
) -> Result<()> {
    // Delay value parsing until uniqueness is known, so errors stay specific.
    if slot.is_some() {
        return Err(usage(format!("duplicate {name}")));
    }
    *slot = Some(parse()?);
    Ok(())
}

/// Detect an OP2 destination when validating OP2-only options before dispatch.
fn is_op2_output(args: &Args) -> bool {
    // Inspect only the destination suffix; the full format check runs later.
    args.command == "convert"
        && args
            .paths
            .get(1)
            .and_then(|path| path.extension())
            .and_then(OsStr::to_str)
            .is_some_and(|extension| extension.eq_ignore_ascii_case("op2"))
}

/// Recognize OP2 input when accepting provenance on a synthetic reread.
fn is_op2_input(args: &Args) -> bool {
    args.command == "convert"
        && args.from.as_deref().map_or_else(
            || {
                args.paths
                    .first()
                    .is_some_and(|path| Format::from_input_path(path).ok() == Some(Format::Op2))
            },
            |name| Format::parse(name).ok() == Some(Format::Op2),
        )
}

/// Parse commands and options, then reject contradictory or irrelevant flags.
/// Paths remain operating-system strings; only command/option tokens need UTF-8.
#[allow(clippy::too_many_lines)] // Parsing and cross-option checks share one Args value.
pub(super) fn parse_args(raw: &[OsString]) -> Result<Args> {
    // Bare invocation and global help/version switches need no path parsing.
    if raw.is_empty() {
        return Ok(Args {
            command: "help".into(),
            ..Args::default()
        });
    }
    let command = text(&raw[0])?.to_string();
    if matches!(command.as_str(), "-h" | "--help") {
        return Ok(Args {
            command: "help".into(),
            ..Args::default()
        });
    }
    if command == "help" {
        return match raw.get(1) {
            None => Ok(Args {
                command,
                ..Args::default()
            }),
            Some(topic) if raw.len() == 2 => {
                let topic = text(topic)?;
                if !matches!(topic, "validate" | "convert") {
                    return Err(usage(format!("unknown help topic {topic:?}; use --help")));
                }
                Ok(Args {
                    command: topic.into(),
                    help_requested: true,
                    ..Args::default()
                })
            }
            Some(_) => Err(usage("help expects one command name")),
        };
    }
    if matches!(command.as_str(), "-V" | "--version") {
        return Ok(Args {
            command: "version".into(),
            ..Args::default()
        });
    }
    if matches!(command.as_str(), "-f" | "--formats") {
        if raw.len() != 1 {
            return Err(usage("--formats takes no options or paths"));
        }
        return Ok(Args {
            command: "formats".into(),
            ..Args::default()
        });
    }
    if !matches!(command.as_str(), "validate" | "convert") {
        return Err(usage(format!("unknown command {command:?}; use --help")));
    }
    // A command name alone requests its help, like the bare executable.
    if raw.len() == 1 {
        return Ok(Args {
            command,
            help_requested: true,
            ..Args::default()
        });
    }
    let mut args = Args {
        command,
        ..Args::default()
    };

    // After --, even a leading dash belongs to a path rather than an option.
    let mut index = 1;
    let mut options = true;
    while index < raw.len() {
        let arg = &raw[index];
        if options && arg == "--" {
            options = false;
            index += 1;
            continue;
        }
        if options && (arg == "--help" || arg == "-h") {
            args.help_requested = true;
            return Ok(args);
        }
        let value = |offset: usize| -> Result<&str> {
            raw.get(index + offset)
                .ok_or_else(|| usage(format!("missing value after {}", arg.to_string_lossy())))
                .and_then(|v| text(v))
        };

        // Valued options advance past their following token; all other
        // non-options are collected as OS-native paths.
        if options && arg == "--json" {
            set_flag(&mut args.json, "--json")?;
        } else if options && arg == "--strict" {
            set_flag(&mut args.strict, "--strict")?;
        } else if options && arg == "--overwrite" {
            set_flag(&mut args.overwrite, "--overwrite")?;
        } else if options && arg == "--accept-omissions" {
            set_flag(&mut args.accept_omissions, "--accept-omissions")?;
        } else if options && arg == "--accept-basic-frame" {
            set_flag(&mut args.accept_basic_frame, "--accept-basic-frame")?;
        } else if options && arg == "--accept-zero-rotations" {
            set_flag(&mut args.accept_zero_rotations, "--accept-zero-rotations")?;
        } else if options && arg == "--accept-synthetic-zero" {
            set_flag(&mut args.accept_synthetic_zero, "--accept-synthetic-zero")?;
        } else if options && arg == "--accept-all" {
            set_flag(&mut args.accept_all, "--accept-all")?;
        } else if options && arg == "--from" {
            set_option(&mut args.from, "--from", || Ok(value(1)?.to_string()))?;
            index += 1;
        } else if options && arg == "--mesh" {
            set_option(&mut args.mesh, "--mesh", || Ok(PathBuf::from(value(1)?)))?;
            index += 1;
        } else if options && arg == "--mesh-out" {
            set_option(&mut args.mesh_out, "--mesh-out", || {
                Ok(PathBuf::from(value(1)?))
            })?;
            index += 1;
        } else if options && arg == "--subcase" {
            set_option(&mut args.subcase, "--subcase", || {
                value(1)?
                    .parse()
                    .map_err(|_| usage("--subcase requires an integer"))
            })?;
            index += 1;
        } else if options && arg == "--msh-version" {
            set_option(&mut args.msh_version, "--msh-version", || match value(1)? {
                "2.2" => Ok(MshVersion::V2_2),
                "4.1" => Ok(MshVersion::V4_1),
                _ => Err(usage("--msh-version requires 2.2 or 4.1")),
            })?;
            index += 1;
        } else if options && arg == "--step" {
            set_option(&mut args.step, "--step", || {
                value(1)?
                    .parse()
                    .map_err(|_| usage("--step requires a nonnegative integer"))
            })?;
            index += 1;
        } else if options && arg == "--max-bytes" {
            set_option(&mut args.max_bytes, "--max-bytes", || {
                let limit = value(1)?
                    .parse()
                    .map_err(|_| usage("--max-bytes requires a positive integer"))?;
                if limit == 0 {
                    return Err(usage("--max-bytes must be positive"));
                }
                Ok(limit)
            })?;
            index += 1;
        } else if options && arg.to_string_lossy().starts_with('-') {
            return Err(usage(format!(
                "unknown option {}; use -- before a path beginning with '-'",
                arg.to_string_lossy()
            )));
        } else {
            args.paths.push(PathBuf::from(arg));
        }
        index += 1;
    }

    // Command arity and cross-option rules are checked after collection so
    // flags may appear before or after path arguments.
    let expected = if args.command == "validate" { 1 } else { 2 };
    if args.paths.len() != expected {
        return Err(usage(format!(
            "{} expects {expected} path argument(s)",
            args.command
        )));
    }
    if args.strict && args.command != "validate" {
        return Err(usage("--strict is only for validate"));
    }
    if args.overwrite && args.command != "convert" {
        return Err(usage("--overwrite is only for convert"));
    }
    if args.overwrite && args.mesh_out.is_some() {
        return Err(usage("--overwrite cannot be used with --mesh-out"));
    }
    if (args.accept_omissions
        || args.accept_zero_rotations
        || args.accept_synthetic_zero
        || args.accept_all)
        && args.command != "convert"
    {
        return Err(usage("conversion acceptance flags are only for convert"));
    }
    let op2_output = is_op2_output(&args);
    if args.mesh_out.is_some() && !op2_output {
        return Err(usage("--mesh-out applies only to OP2 output"));
    }
    if args.accept_basic_frame && args.mesh.is_none() {
        return Err(usage(
            "--accept-basic-frame requires --mesh for OP2/PCH input",
        ));
    }
    if args.accept_zero_rotations && !op2_output {
        return Err(usage("--accept-zero-rotations applies only to OP2 output"));
    }
    if args.accept_synthetic_zero && !op2_output && !is_op2_input(&args) {
        return Err(usage(
            "--accept-synthetic-zero applies only to OP2 input or output",
        ));
    }
    if args.accept_synthetic_zero && args.accept_zero_rotations {
        return Err(usage(
            "synthetic zero already supplies all six displacement components",
        ));
    }
    if args.msh_version.is_some() {
        let msh_target = args.command == "convert"
            && Format::from_output_path(&args.paths[1]).is_ok_and(|format| format == Format::Msh);
        let msh_companion = args.mesh_out.as_deref().is_some_and(|path| {
            Format::from_output_path(path).is_ok_and(|format| format == Format::Msh)
        });
        if !msh_target && !msh_companion {
            return Err(usage("--msh-version applies only to MSH output"));
        }
    }
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse CLI tokens without spawning the executable.
    fn args(values: &[&str]) -> Result<Args> {
        let raw: Vec<OsString> = values.iter().map(OsString::from).collect();
        parse_args(&raw)
    }

    /// Allow a conversion command without a preemptive acceptance flag.
    #[test]
    fn conversion_can_request_confirmation_later() {
        assert!(args(&["convert", "x.bdf", "x.vtu"]).is_ok());
    }

    /// Reject repeated option flags so command intent is unambiguous.
    #[test]
    fn duplicate_option_rejected() {
        assert!(args(&["validate", "x.bdf", "--json", "--json"]).is_err());
    }

    /// Reject options that do not apply to the selected command or format.
    #[test]
    fn irrelevant_flag_rejected() {
        assert!(args(&["convert", "x.bdf", "x.vtu", "--strict"]).is_err());
    }

    /// Accept the short catchall spelling and reject the retired long spelling.
    #[test]
    fn catchall_acceptance_uses_short_spelling() {
        assert!(
            args(&["convert", "x.bdf", "x.vtu", "--accept-all"])
                .unwrap()
                .accept_all
        );
        assert!(
            args(&[
                "convert",
                "x.bdf",
                "x.vtu",
                "--accept-all-approximations-and-infill",
            ])
            .is_err()
        );
    }

    /// Apply the MSH version switch only when writing an MSH destination.
    #[test]
    fn msh_dialect_is_selected_only_for_msh_output() {
        let selected = args(&[
            "convert",
            "x.bdf",
            "x.msh",
            "--msh-version",
            "2.2",
            "--accept-all",
        ])
        .unwrap();
        assert_eq!(selected.msh_version, Some(MshVersion::V2_2));
        assert!(
            args(&[
                "convert",
                "x.msh",
                "x.vtu",
                "--msh-version",
                "2.2",
                "--accept-all"
            ])
            .is_err()
        );
        assert!(
            args(&[
                "convert",
                "x.bdf",
                "x.msh",
                "--msh-version",
                "9.9",
                "--accept-all"
            ])
            .is_err()
        );
    }

    /// Treat paths after -- as paths even when they begin with a hyphen.
    #[test]
    fn double_dash_supports_dash_path() {
        assert_eq!(
            args(&["validate", "--", "-mesh.bdf"]).unwrap().paths[0],
            PathBuf::from("-mesh.bdf")
        );
    }

    /// Return a usage error for an unknown command name.
    #[test]
    fn typo_command_rejected() {
        assert!(args(&["convertx"]).is_err());
    }

    /// Reject commands with missing or extra positional paths.
    #[test]
    fn bad_arity_rejected() {
        assert!(args(&["convert", "only.bdf", "--accept-all"]).is_err());
    }

    /// The global formats switch takes no additional options or paths.
    #[test]
    fn format_report_is_a_global_switch() {
        assert_eq!(args(&["-f"]).unwrap().command, "formats");
        assert!(args(&["--formats", "--json"]).is_err());
        assert!(args(&["--json", "-f"]).is_err());
        assert!(args(&["-f", "--formats"]).is_err());
        assert!(args(&["--formats", "x.bdf"]).is_err());
        assert!(args(&["formats"]).is_err());
    }

    /// Constrain synthetic zero displacement to its explicit OP2 output path.
    #[test]
    fn assumed_zero_requires_op2_output_and_no_rotation_fill_flag() {
        assert!(
            args(&[
                "convert",
                "x.bdf",
                "x.op2",
                "--accept-all",
                "--accept-synthetic-zero"
            ])
            .is_ok()
        );
        assert!(
            args(&[
                "convert",
                "x.bdf",
                "x.vtu",
                "--accept-all",
                "--accept-synthetic-zero"
            ])
            .is_err()
        );
        assert!(
            args(&[
                "convert",
                "x.bdf",
                "x.op2",
                "--accept-all",
                "--accept-synthetic-zero",
                "--accept-zero-rotations"
            ])
            .is_err()
        );
    }
}
