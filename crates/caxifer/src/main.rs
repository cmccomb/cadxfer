mod json;
mod output;

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use caxifer_core::{Error, Result, Severity, ValidationReport};
use caxifer_formats::{bdf::{Document, ParseOptions, parse_real}, vtu};
use json::{array, object, quote};

const HELP: &str = "caxifer 0.1.0 — inspect, preserve, and explicitly project engineering files

USAGE
  caxifer formats [--json]
  caxifer info INPUT [--json]
  caxifer validate INPUT [--strict] [--json]
  caxifer roundtrip INPUT OUTPUT [--json]
  caxifer set-grid INPUT OUTPUT --id ID --xyz X Y Z [--json]
  caxifer convert INPUT OUTPUT.vtu --geometry-only [--json]

COMMON OPTIONS
  --from bdf       Explicit input format; otherwise .bdf/.nas/.dat/.pch required
  --max-bytes N    Input byte limit (default: 268435456)
  --json           Machine-readable output (schema_version=1)
  --               Treat remaining arguments as file paths
  -h, --help       Show this help
  -V, --version    Show version

SCOPE
  BDF: source-preserving small/large/free fields, GRID editing, linear geometry.
  VTU: ASCII geometry export only, with original IDs. No unit inference.
  validate checks the GEOMETRY SUBSET, not complete solver validity.
  set-grid coordinates are in the GRID's native CP frame.
  convert requires --geometry-only and reports omitted solver information.
  Unresolved INCLUDEs, nonbasic CP, GRDSET, higher-order/unknown geometry fail.
  Output files must not already exist. No OP2/FRD readers in this release.
";

#[derive(Debug, Default)]
struct Args {
    command: String,
    paths: Vec<PathBuf>,
    json: bool,
    strict: bool,
    geometry_only: bool,
    from: Option<String>,
    max_bytes: Option<usize>,
    id: Option<u64>,
    xyz: Option<[f64; 3]>,
}

fn usage(message: impl Into<String>) -> Error { Error::new("E_USAGE", message) }

fn text(arg: &OsStr) -> Result<&str> { arg.to_str().ok_or_else(|| usage("option value is not valid UTF-8")) }

fn parse_args(raw: Vec<OsString>) -> Result<Args> {
    if raw.is_empty() { return Ok(Args { command: "help".into(), ..Args::default() }); }
    let command = text(&raw[0])?.to_string();
    if matches!(command.as_str(), "-h" | "--help" | "help") { return Ok(Args { command: "help".into(), ..Args::default() }); }
    if matches!(command.as_str(), "-V" | "--version") { return Ok(Args { command: "version".into(), ..Args::default() }); }
    if !matches!(command.as_str(), "formats" | "info" | "validate" | "roundtrip" | "set-grid" | "convert") {
        return Err(usage(format!("unknown command {command:?}; use --help")));
    }
    let mut args = Args { command, ..Args::default() };
    let mut index = 1;
    let mut options = true;
    while index < raw.len() {
        let arg = &raw[index];
        if options && arg == "--" { options = false; index += 1; continue; }
        if options && (arg == "--help" || arg == "-h") { return Ok(Args { command: "help".into(), ..args }); }
        let value = |offset: usize| -> Result<&str> {
            raw.get(index + offset).ok_or_else(|| usage(format!("missing value after {}", arg.to_string_lossy()))).and_then(|v| text(v))
        };
        if options && arg == "--json" { if args.json { return Err(usage("duplicate --json")); } args.json = true; }
        else if options && arg == "--strict" { if args.strict { return Err(usage("duplicate --strict")); } args.strict = true; }
        else if options && arg == "--geometry-only" { if args.geometry_only { return Err(usage("duplicate --geometry-only")); } args.geometry_only = true; }
        else if options && arg == "--from" {
            if args.from.is_some() { return Err(usage("duplicate --from")); }
            args.from = Some(value(1)?.to_string()); index += 1;
        } else if options && arg == "--max-bytes" {
            if args.max_bytes.is_some() { return Err(usage("duplicate --max-bytes")); }
            let limit = value(1)?.parse().map_err(|_| usage("--max-bytes requires a positive integer"))?;
            if limit == 0 { return Err(usage("--max-bytes must be positive")); }
            args.max_bytes = Some(limit); index += 1;
        } else if options && arg == "--id" {
            if args.id.is_some() { return Err(usage("duplicate --id")); }
            let id = value(1)?.parse().map_err(|_| usage("--id requires a positive integer"))?;
            if id == 0 { return Err(usage("--id must be positive")); }
            args.id = Some(id); index += 1;
        } else if options && arg == "--xyz" {
            if args.xyz.is_some() { return Err(usage("duplicate --xyz")); }
            args.xyz = Some([parse_real(value(1)?)?, parse_real(value(2)?)?, parse_real(value(3)?)?]);
            index += 3;
        } else if options && arg.to_string_lossy().starts_with('-') {
            return Err(usage(format!("unknown option {}; use -- before a path beginning with '-'", arg.to_string_lossy())));
        } else { args.paths.push(PathBuf::from(arg)); }
        index += 1;
    }
    let expected = match args.command.as_str() { "formats" => 0, "info" | "validate" => 1, _ => 2 };
    if args.paths.len() != expected { return Err(usage(format!("{} expects {expected} path argument(s)", args.command))); }
    if args.strict && args.command != "validate" { return Err(usage("--strict is only for validate")); }
    if args.geometry_only && args.command != "convert" { return Err(usage("--geometry-only is only for convert")); }
    if args.command == "convert" && !args.geometry_only { return Err(usage("BDF → VTU is a geometry projection, not a solver-model conversion; pass --geometry-only to acknowledge omitted information")); }
    if args.command == "set-grid" {
        if args.id.is_none() || args.xyz.is_none() { return Err(usage("set-grid requires --id and --xyz")); }
    } else if args.id.is_some() || args.xyz.is_some() { return Err(usage("--id and --xyz are only for set-grid")); }
    if args.command == "formats" && (args.from.is_some() || args.max_bytes.is_some()) { return Err(usage("formats does not read an input")); }
    Ok(args)
}

fn emit(value: &str) -> Result<()> {
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{value}")?;
    Ok(())
}

fn report_json(report: &ValidationReport) -> String {
    object([
        ("scope", quote("geometry-subset")),
        ("full_solver_validation", "false".into()),
        ("valid_in_scope", report.valid_in_scope().to_string()),
        ("errors", report.error_count().to_string()),
        ("warnings", report.warning_count().to_string()),
        ("diagnostics", array(report.diagnostics.iter().map(|d| object([
            ("severity", quote(if d.severity == Severity::Error { "error" } else { "warning" })),
            ("code", quote(d.code)), ("message", quote(&d.message)),
            ("line", d.line.map_or_else(|| "null".into(), |line| line.to_string())),
        ])))),
    ])
}

fn open_document(args: &Args) -> Result<Document> {
    let path = &args.paths[0];
    if let Some(format) = &args.from {
        if format != "bdf" { return Err(Error::new("E_FORMAT", "only --from bdf is implemented in v0.1.0")); }
    } else {
        let extension = path.extension().and_then(OsStr::to_str).unwrap_or("").to_ascii_lowercase();
        if !matches!(extension.as_str(), "bdf" | "nas" | "dat" | "pch") {
            return Err(Error::new("E_FORMAT", format!("no reader for extension {extension:?}; OP2/FRD/VTU input is not implemented; use --from bdf only for a known text BDF")));
        }
    }
    let options = ParseOptions { max_bytes: args.max_bytes.unwrap_or(ParseOptions::default().max_bytes), ..ParseOptions::default() };
    Document::read_with_options(File::open(path)?, options)
}

fn path_json(path: &Path) -> String { quote(&path.to_string_lossy()) }

fn run(args: Args) -> Result<u8> {
    match args.command.as_str() {
        "help" => { emit(HELP)?; return Ok(0); }
        "version" => { emit(concat!("caxifer ", env!("CARGO_PKG_VERSION")))?; return Ok(0); }
        "formats" => {
            if args.json {
                emit(&object([("schema_version", "1".into()), ("formats", array([
                    object([("format", quote("bdf")), ("read", "true".into()), ("write", quote("source-preserving document")), ("scope", quote("linear geometry subset"))]),
                    object([("format", quote("vtu")), ("read", "false".into()), ("write", quote("ASCII geometry only")), ("scope", quote("linear cells and IDs"))]),
                ]))]))?;
            } else { emit("bdf  read/document-write  small, large, free fields; limited geometry semantics\nvtu  write-only           ASCII unstructured geometry + original IDs\nOP2, FRD, INCLUDE expansion, and Python bindings are not implemented.")?; }
            return Ok(0);
        }
        _ => {}
    }
    let mut doc = open_document(&args)?;
    match args.command.as_str() {
        "info" => {
            let counts = doc.card_counts();
            let report = doc.validate_geometry();
            if args.json {
                emit(&object([
                    ("schema_version", "1".into()), ("format", quote("bdf")), ("path", path_json(&args.paths[0])),
                    ("bytes", doc.to_bytes().len().to_string()), ("full_deck", doc.is_full_deck().to_string()),
                    ("card_counts", object(counts.iter().map(|(key, value)| (key.as_str(), value.to_string())))),
                    ("units", quote("unspecified")), ("geometry", report_json(&report)),
                ]))?;
            } else {
                emit(&format!("BDF document: {}\nBytes: {}\nFull deck: {}\nUnits: unspecified", args.paths[0].display(), doc.to_bytes().len(), doc.is_full_deck()))?;
                for (name, count) in counts { emit(&format!("  {name:<10} {count}"))?; }
                emit(&format!("Geometry projection available: {} ({} error(s), {} warning(s))", report.valid_in_scope(), report.error_count(), report.warning_count()))?;
                for d in report.diagnostics.iter().filter(|d| d.severity == Severity::Error).take(5) { emit(&format!("  {}: {}", d.code, d.message))?; }
            }
        }
        "validate" => {
            let report = doc.validate_geometry();
            let passed = report.valid_in_scope() && (!args.strict || report.warning_count() == 0);
            if args.json {
                emit(&object([("schema_version", "1".into()), ("strict", args.strict.to_string()), ("passed", passed.to_string()), ("validation", report_json(&report))]))?;
            } else {
                emit(&format!("Geometry-subset checks: {}. Not a full Nastran solver validation.", if passed { "passed" } else { "failed" }))?;
                for d in &report.diagnostics {
                    emit(&format!("{:?} {}{}: {}", d.severity, d.code, d.line.map_or_else(String::new, |line| format!(" at line {line}")), d.message))?;
                }
            }
            return Ok(if passed { 0 } else { 1 });
        }
        "roundtrip" | "set-grid" => {
            if args.command == "set-grid" {
                let id = args.id.ok_or_else(|| usage("missing --id"))?;
                let xyz = args.xyz.ok_or_else(|| usage("missing --xyz"))?;
                doc.set_grid_coordinates(id, xyz)?;
            }
            output::create_new(&args.paths[1], |writer| doc.write_to(writer))?;
            if args.json {
                emit(&object([("schema_version", "1".into()), ("operation", quote(&args.command)), ("output", path_json(&args.paths[1])), ("bytes", doc.to_bytes().len().to_string())]))?;
            } else {
                emit(&format!("Wrote {} ({} bytes; {}).", args.paths[1].display(), doc.to_bytes().len(), if args.command == "roundtrip" { "source bytes unchanged" } else { "only requested native-frame GRID coordinate fields changed" }))?;
            }
        }
        "convert" => {
            if !args.paths[1].extension().and_then(OsStr::to_str).is_some_and(|e| e.eq_ignore_ascii_case("vtu")) {
                return Err(Error::new("E_FORMAT", "v0.1 geometry export requires an output ending in .vtu"));
            }
            let projection = doc.geometry()?;
            output::create_new(&args.paths[1], |writer| vtu::write(&projection.mesh, writer))?;
            if args.json {
                emit(&object([
                    ("schema_version", "1".into()), ("operation", quote("geometry-projection")), ("output", path_json(&args.paths[1])),
                    ("points", projection.mesh.points.len().to_string()), ("cells", projection.mesh.cells.len().to_string()),
                    ("units", quote("unspecified")),
                    ("omissions", array(projection.omissions.iter().map(|item| object([
                        ("category", quote(&item.category)),
                        ("count", item.count.to_string()),
                        ("detail", quote(&item.detail)),
                    ])))),
                ]))?;
            } else {
                emit(&format!("Wrote {}: {} points, {} cells. Geometry only; units unspecified.", args.paths[1].display(), projection.mesh.points.len(), projection.mesh.cells.len()))?;
                let mut stderr = std::io::stderr().lock();
                for item in &projection.omissions { writeln!(stderr, "Omission [{} × {}]: {}", item.category, item.count, item.detail)?; }
            }
        }
        _ => return Err(usage("unknown command")),
    }
    Ok(0)
}

fn main() {
    let raw: Vec<OsString> = std::env::args_os().skip(1).collect();
    let wants_json = raw.iter().any(|arg| arg == "--json");
    let status = match parse_args(raw).and_then(run) {
        Ok(status) => status,
        Err(error) => {
            let message = if wants_json {
                object([("schema_version", "1".into()), ("error", object([("code", quote(error.code)), ("message", quote(&error.message)), ("line", error.line.map_or_else(|| "null".into(), |line| line.to_string()))]))])
            } else { error.to_string() };
            if wants_json { let _ = emit(&message); }
            else { let _ = writeln!(std::io::stderr().lock(), "{message}"); }
            if error.code == "E_USAGE" { 2 } else { 1 }
        }
    };
    std::process::exit(i32::from(status));
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(values: &[&str]) -> Result<Args> { parse_args(values.iter().map(OsString::from).collect()) }
    #[test]
    fn conversion_requires_acknowledgement() { assert!(args(&["convert", "x.bdf", "x.vtu"]).is_err()); }
    #[test]
    fn negative_coordinate_values_are_not_options() {
        assert_eq!(args(&["set-grid", "x.bdf", "y.bdf", "--id", "1", "--xyz", "-1", "2", "-3"]).unwrap().xyz, Some([-1., 2., -3.]));
    }
    #[test]
    fn duplicate_option_rejected() { assert!(args(&["info", "x.bdf", "--json", "--json"]).is_err()); }
    #[test]
    fn irrelevant_flag_rejected() { assert!(args(&["info", "x.bdf", "--strict"]).is_err()); }
    #[test]
    fn double_dash_supports_dash_path() { assert_eq!(args(&["info", "--", "-mesh.bdf"]).unwrap().paths[0], PathBuf::from("-mesh.bdf")); }
    #[test]
    fn typo_command_rejected() { assert!(args(&["convertx"]).is_err()); }
    #[test]
    fn bad_arity_rejected() { assert!(args(&["roundtrip", "only.bdf"]).is_err()); }
    #[test]
    fn format_report_takes_no_paths() { assert!(args(&["formats", "x.bdf"]).is_err()); }
}
