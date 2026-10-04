#[path = "cli/json.rs"]
mod json;
#[path = "cli/output.rs"]
mod output;

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use caexfer::bdf::{Document, ParseOptions};
use caexfer::conversion::{self, Format, Omission, Options, ReadResult, Stage};
use caexfer::core::{Error, Field, FieldLocation, Result, Severity, ValidationReport};
use caexfer::msh;
use json::{array, object, quote};

const HELP: &str = "caexfer — inspect, preserve, and explicitly project engineering files

USAGE
  caexfer formats [--json]
  caexfer info INPUT [--json]
  caexfer validate INPUT [--strict] [--json]
  caexfer convert INPUT OUTPUT --accept-projection [--json]

OPTIONS
  --from FORMAT               bdf, vtu, vtk, msh, inp, frd, op2, pch; else infer extension
  --strict                    Fail validation on warnings or omissions
  --accept-projection         Required for conversion into the supported subset
  --step N                    Zero-based OP2/PCH result step or FRD step number
  --max-bytes N               Input byte limit (default: 268435456)
  --msh-version 2.2|4.1        MSH output dialect (default: 4.1)
  --json                      Machine-readable output (schema_version=1)
  --                          Treat remaining arguments as file paths
  -h, --help                  Show this help
  -V, --version               Show version

NASTRAN RESULT OPTIONS
  --mesh FILE                 Matching BDF, VTU, VTK, MSH, INP, or FRD mesh for OP2/PCH input
  --assume-basic-frame        Assert basic frame for non-BDF mesh and result
  --subcase N                 Select a displacement subcase when reading
OP2 OUTPUT OPTIONS
  --mesh-out FILE             Also write a separate mesh with OP2 output
  --zero-missing-rotations    Assert absent R1/R2/R3 are zero when writing
  --assume-zero-displacement  Synthesize zero OP2 from BDF/INP (no solver)

EXAMPLES
  caexfer info model.bdf
  caexfer convert model.bdf model.vtu --accept-projection
  caexfer convert results.op2 results.vtu --mesh model.bdf --accept-projection
";

/// Parsed CLI command, paths, and explicitly supplied option flags.
#[derive(Debug, Default)]
#[allow(clippy::struct_excessive_bools)] // CLI switches are independent user flags.
struct Args {
    command: String,
    paths: Vec<PathBuf>,
    json: bool,
    strict: bool,
    accept_projection: bool,
    zero_missing_rotations: bool,
    assume_zero_displacement: bool,
    from: Option<String>,
    mesh: Option<PathBuf>,
    mesh_out: Option<PathBuf>,
    assume_basic_frame: bool,
    subcase: Option<i64>,
    step: Option<usize>,
    max_bytes: Option<usize>,
    msh_version: Option<msh::Version>,
}

/// Wrap a command-line usage failure with the exit-code-selecting `E_USAGE`.
fn usage(message: impl Into<String>) -> Error {
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

/// Parse commands and options, then reject contradictory or irrelevant flags.
/// Paths remain operating-system strings; only command/option tokens need UTF-8.
#[allow(clippy::too_many_lines)] // Parsing and cross-option checks share one Args value.
fn parse_args(raw: &[OsString]) -> Result<Args> {
    // Bare invocation and global help/version switches need no path parsing.
    if raw.is_empty() {
        return Ok(Args {
            command: "help".into(),
            ..Args::default()
        });
    }
    let command = text(&raw[0])?.to_string();
    if matches!(command.as_str(), "-h" | "--help" | "help") {
        return Ok(Args {
            command: "help".into(),
            ..Args::default()
        });
    }
    if matches!(command.as_str(), "-V" | "--version") {
        return Ok(Args {
            command: "version".into(),
            ..Args::default()
        });
    }
    if !matches!(
        command.as_str(),
        "formats" | "info" | "validate" | "convert"
    ) {
        return Err(usage(format!("unknown command {command:?}; use --help")));
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
            return Ok(Args {
                command: "help".into(),
                ..args
            });
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
        } else if options && arg == "--accept-projection" {
            set_flag(&mut args.accept_projection, "--accept-projection")?;
        } else if options && arg == "--zero-missing-rotations" {
            set_flag(&mut args.zero_missing_rotations, "--zero-missing-rotations")?;
        } else if options && arg == "--assume-zero-displacement" {
            set_flag(
                &mut args.assume_zero_displacement,
                "--assume-zero-displacement",
            )?;
        } else if options && arg == "--assume-basic-frame" {
            set_flag(&mut args.assume_basic_frame, "--assume-basic-frame")?;
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
                "2.2" => Ok(msh::Version::V2_2),
                "4.1" => Ok(msh::Version::V4_1),
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
    let expected = match args.command.as_str() {
        "formats" => 0,
        "info" | "validate" => 1,
        _ => 2,
    };
    if args.paths.len() != expected {
        return Err(usage(format!(
            "{} expects {expected} path argument(s)",
            args.command
        )));
    }
    if args.strict && args.command != "validate" {
        return Err(usage("--strict is only for validate"));
    }
    if args.accept_projection && args.command != "convert" {
        return Err(usage("--accept-projection is only for convert"));
    }
    let op2_output = is_op2_output(&args);
    if args.mesh_out.is_some() && !op2_output {
        return Err(usage("--mesh-out applies only to OP2 output"));
    }
    if args.assume_basic_frame && args.mesh.is_none() {
        return Err(usage(
            "--assume-basic-frame requires --mesh for OP2/PCH input",
        ));
    }
    if args.zero_missing_rotations && !op2_output {
        return Err(usage("--zero-missing-rotations applies only to OP2 output"));
    }
    if args.assume_zero_displacement && !op2_output {
        return Err(usage(
            "--assume-zero-displacement applies only to OP2 output",
        ));
    }
    if args.assume_zero_displacement && args.zero_missing_rotations {
        return Err(usage(
            "--assume-zero-displacement already supplies all six components",
        ));
    }
    if args.command == "convert" && !args.accept_projection {
        return Err(usage(
            "conversion projects the supported mesh/field subset; pass --accept-projection to acknowledge omitted information",
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
    if args.command == "formats"
        && (args.from.is_some()
            || args.max_bytes.is_some()
            || args.mesh.is_some()
            || args.mesh_out.is_some()
            || args.assume_basic_frame
            || args.subcase.is_some()
            || args.step.is_some())
    {
        return Err(usage("formats does not read an input"));
    }
    Ok(args)
}

/// Write one complete output line to locked stdout with I/O error propagation.
fn emit(value: &str) -> Result<()> {
    // Lock stdout for the whole line so other writes cannot interleave it.
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{value}")?;
    Ok(())
}

/// Encode scoped geometry diagnostics, counts, and validity as CLI JSON.
/// This explicitly says validation is not full solver validation.
fn report_json(report: &ValidationReport) -> String {
    // Make the geometry-only validation scope explicit in machine output.
    object([
        ("scope", quote("geometry-subset")),
        ("full_solver_validation", "false".into()),
        ("valid_in_scope", report.valid_in_scope().to_string()),
        ("errors", report.error_count().to_string()),
        ("warnings", report.warning_count().to_string()),
        (
            "diagnostics",
            array(report.diagnostics.iter().map(|d| {
                object([
                    (
                        "severity",
                        quote(if d.severity == Severity::Error {
                            "error"
                        } else {
                            "warning"
                        }),
                    ),
                    ("code", quote(d.code)),
                    ("message", quote(&d.message)),
                    (
                        "line",
                        d.line
                            .map_or_else(|| "null".into(), |line| line.to_string()),
                    ),
                ])
            })),
        ),
    ])
}

/// Read a source-preserving BDF under the CLI's selected byte cap.
fn read_bdf(path: &Path, max_bytes: usize) -> Result<Document> {
    // Apply the CLI cap while preserving the BDF parser's other limits.
    Document::read_with_options(
        File::open(path)?,
        ParseOptions {
            max_bytes,
            ..ParseOptions::default()
        },
    )
}

/// Quote an OS path for JSON, using a displayable lossy form when needed.
fn path_json(path: &Path) -> String {
    quote(&path.to_string_lossy())
}

/// Prefer an explicit source format; otherwise use the input path extension.
fn format_of(args: &Args) -> Result<Format> {
    args.from
        .as_deref()
        .map_or_else(|| Format::from_input_path(&args.paths[0]), Format::parse)
}

/// Translate validated CLI flags into the library's typed conversion options.
fn conversion_options(args: &Args) -> Result<Options> {
    let format = format_of(args)?;
    Ok(Options {
        input_format: Some(format),
        mesh: args.mesh.clone(),
        assume_basic_frame: args.assume_basic_frame,
        subcase: args.subcase,
        step: args.step,
        max_bytes: args.max_bytes.unwrap_or(ParseOptions::default().max_bytes),
        msh_version: args.msh_version,
        zero_missing_rotations: args.zero_missing_rotations,
    })
}

/// Add an explicitly synthetic six-component zero field to a geometry source.
/// Preserve its assumption in the conversion report and OP2 provenance title.
fn assume_zero_displacement(args: &Args, source: &mut ReadResult, max_bytes: usize) -> Result<()> {
    // Synthetic displacement is an explicit geometry-to-result assumption,
    // limited to sources that carry no numeric results of their own.
    if !matches!(source.format, Format::Bdf | Format::Inp) {
        return Err(usage(
            "--assume-zero-displacement currently accepts BDF or INP input",
        ));
    }
    if !source.dataset.fields.is_empty() {
        return Err(Error::new(
            "E_OP2",
            "zero-displacement assumption requires a result-free input",
        ));
    }
    if source.format == Format::Bdf {
        // OP2 rereads against the BDF mesh, so its GRID output frame must be
        // basic even though the synthetic displacement values are all zero.
        for grid in read_bdf(&args.paths[0], max_bytes)?.grids() {
            if grid?.cd != 0 {
                return Err(Error::new(
                    "E_OP2",
                    "synthetic OP2 output requires basic-frame GRID CD=0 for matching BDF reread",
                ));
            }
        }
    }
    let count = source
        .dataset
        .mesh
        .points
        .len()
        .checked_mul(6)
        .ok_or_else(|| Error::new("E_LIMIT", "too many nodes for synthetic displacement"))?;

    // Record provenance alongside the values for both report and OP2 title.
    source.dataset.fields.push(Field {
        name: "DISP".into(),
        location: FieldLocation::Point,
        components: ["T1", "T2", "T3", "R1", "R2", "R3"]
            .map(str::to_owned)
            .to_vec(),
        values: vec![0.0; count],
        step: None,
        time: None,
    });
    source.omissions.push(Omission {
        stage: Stage::Assumption,
        detail: "SYNTHETIC ASSUMPTION: all six displacement components set to float 0.0 for every node; no solver analysis was performed".into(),
    });
    source.assumed_zero = true;
    Ok(())
}

/// Convert a source into a staged destination and optional companion mesh.
/// The library reports omissions; this layer owns no-clobber file installation
/// and human or JSON presentation of the result.
fn run_convert(args: &Args) -> Result<u8> {
    // Read and project before staging output, so source failures leave no
    // destination path behind.
    let target = Format::from_output_path(&args.paths[1])?;
    let options = conversion_options(args)?;
    let mut source = conversion::read_path(&args.paths[0], &options)?;
    if args.assume_zero_displacement {
        assume_zero_displacement(args, &mut source, options.max_bytes)?;
    }
    let mut report = None;
    let mesh_output = if let Some(path) = &args.mesh_out {
        // OP2 has no embedded mesh; stage the result and its companion
        // together before installing either final path.
        let format = Format::from_output_path(path)?;
        if format == Format::Op2 {
            return Err(usage("--mesh-out requires a mesh-bearing output format"));
        }
        let mut mesh_source = source.clone();
        let excluded = mesh_source.dataset.fields.len();

        // The companion is geometry-only, with excluded fields reported.
        mesh_source.dataset.fields.clear();
        if excluded > 0 {
            mesh_source.omissions.push(Omission {
                stage: Stage::Destination,
                detail: format!("{excluded} result field(s) excluded from companion mesh"),
            });
        }
        let mut mesh_report = None;
        output::create_pair(
            &args.paths[1],
            |writer| {
                report = Some(conversion::convert(source, target, &options, writer)?);
                Ok(())
            },
            path,
            |writer| {
                mesh_report = Some(conversion::convert(mesh_source, format, &options, writer)?);
                Ok(())
            },
        )?;
        Some((
            path,
            format,
            mesh_report
                .ok_or_else(|| Error::new("E_OUTPUT", "companion mesh produced no report"))?,
        ))
    } else {
        output::create_new(&args.paths[1], |writer| {
            report = Some(conversion::convert(source, target, &options, writer)?);
            Ok(())
        })?;
        None
    };

    // Present the same source/destination omission report in either output
    // mode after successful file installation.
    let report = report.ok_or_else(|| Error::new("E_OUTPUT", "conversion produced no report"))?;
    if args.json {
        let mut fields = vec![
            ("schema_version", "1".into()),
            ("operation", quote("mesh-and-fields-projection")),
            ("output", path_json(&args.paths[1])),
            ("points", report.points.to_string()),
            ("cells", report.cells.to_string()),
            ("fields", report.fields.to_string()),
            ("units", quote("unspecified")),
            ("omissions", omissions_json(&report.omissions)),
        ];
        if let Some((path, format, mesh_report)) = &mesh_output {
            fields.push((
                "mesh_output",
                object([
                    ("path", path_json(path)),
                    ("format", quote(format.name())),
                    ("omissions", omissions_json(&mesh_report.omissions)),
                ]),
            ));
        }
        emit(&object(fields))?;
    } else {
        emit(&format!(
            "Wrote {}: {} points, {} cells, {} source field(s). Units unspecified.",
            args.paths[1].display(),
            report.points,
            report.cells,
            report.fields
        ))?;
        let mut stderr = std::io::stderr().lock();
        for omission in report.omissions {
            writeln!(stderr, "Omission: {}", omission.detail)?;
        }
        if let Some((path, _, mesh_report)) = mesh_output {
            emit(&format!("Wrote companion mesh {}.", path.display()))?;
            for omission in mesh_report.omissions {
                writeln!(stderr, "Companion omission: {}", omission.detail)?;
            }
        }
    }
    Ok(0)
}

/// Encode omission stages and details for the versioned CLI JSON schema.
fn omissions_json(omissions: &[Omission]) -> String {
    // Each omission is one report row with its originating conversion stage.
    array(omissions.iter().map(|omission| {
        object([
            ("category", quote("source-or-destination")),
            ("stage", quote(omission.stage.name())),
            ("count", "1".into()),
            ("detail", quote(&omission.detail)),
        ])
    }))
}

/// Inspect or validate a non-BDF source through the shared format reader.
/// Strict validation fails when the supported projection reports omissions.
fn run_generic_info(args: &Args) -> Result<u8> {
    // Non-BDF readers return projected datasets and explicit source losses.
    let read = conversion::read_path(&args.paths[0], &conversion_options(args)?)?;
    let format = read.format.name();
    let dataset = read.dataset;
    let omissions = read.omissions;
    if args.command == "validate" {
        // Strict mode treats any documented omission as a failed validation.
        let passed = !args.strict || omissions.is_empty();
        if args.json {
            emit(&object([
                ("schema_version", "1".into()),
                ("format", quote(format)),
                ("passed", passed.to_string()),
                ("strict", args.strict.to_string()),
                ("scope", quote("supported-mesh-and-fields-subset")),
                (
                    "omissions",
                    array(omissions.iter().map(|item| quote(&item.detail))),
                ),
            ]))?;
        } else {
            emit(&format!(
                "{} supported-subset checks {}: {} points, {} cells, {} fields, {} omission(s)",
                format.to_uppercase(),
                if passed { "passed" } else { "failed" },
                dataset.mesh.points.len(),
                dataset.mesh.cells.len(),
                dataset.fields.len(),
                omissions.len()
            ))?;
        }
        return Ok(u8::from(!passed));
    } else if args.json {
        emit(&object([
            ("schema_version", "1".into()),
            ("format", quote(format)),
            ("path", path_json(&args.paths[0])),
            ("points", dataset.mesh.points.len().to_string()),
            ("cells", dataset.mesh.cells.len().to_string()),
            ("fields", dataset.fields.len().to_string()),
            (
                "omissions",
                array(omissions.iter().map(|item| quote(&item.detail))),
            ),
        ]))?;
    } else {
        emit(&format!(
            "{}: {} points, {} cells, {} fields",
            format.to_uppercase(),
            dataset.mesh.points.len(),
            dataset.mesh.cells.len(),
            dataset.fields.len()
        ))?;
    }
    Ok(0)
}

/// Dispatch the parsed CLI command and return its process exit status.
/// BDF uses its richer document inspection path; other formats use datasets.
#[allow(clippy::too_many_lines)] // Command branches keep exit codes and output together.
fn run(args: &Args) -> Result<u8> {
    // Commands without input return before any format or file selection.
    match args.command.as_str() {
        "help" => {
            emit(HELP)?;
            return Ok(0);
        }
        "version" => {
            emit(concat!("caexfer ", env!("CARGO_PKG_VERSION")))?;
            return Ok(0);
        }
        "formats" => {
            if args.json {
                let rows = [
                    ("bdf", "document + linear mesh", "geometry mesh projection"),
                    (
                        "vtu",
                        "ASCII one-piece linear mesh + numeric fields",
                        "ASCII one-piece linear mesh + numeric fields",
                    ),
                    (
                        "vtk",
                        "ASCII legacy unstructured grid + numeric fields",
                        "ASCII legacy unstructured grid + numeric fields",
                    ),
                    (
                        "msh",
                        "ASCII MSH 4.1/2.2 linear mesh + complete numeric fields",
                        "ASCII MSH 4.1 by default; 2.2 with --msh-version",
                    ),
                    (
                        "inp",
                        "flat global-node/element mesh",
                        "flat geometry-only INP",
                    ),
                    (
                        "frd",
                        "ASCII linear mesh + nodal fields",
                        "ASCII mesh + complete nodal fields",
                    ),
                    (
                        "op2",
                        "32-bit real OUGV1 displacement and matching mesh",
                        "32-bit real OUGV1 displacement; optional companion mesh export; no embedded mesh",
                    ),
                    (
                        "pch",
                        "ASCII real SORT1 displacement and matching mesh",
                        "read-only; no PCH writer",
                    ),
                    (
                        "stl",
                        "ASCII or binary triangle surface; facet-local generated IDs",
                        "binary triangle surface; IDs and fields omitted",
                    ),
                    (
                        "su2",
                        "single-zone ASCII mesh with named boundary markers",
                        "single-zone ASCII mesh with named boundary markers",
                    ),
                ];
                emit(&object([
                    ("schema_version", "1".into()),
                    (
                        "formats",
                        array(rows.iter().map(|(name, read, write)| {
                            object([
                                ("format", quote(name)),
                                ("read", "true".into()),
                                ("read_scope", quote(read)),
                                ("writable", (name != &"pch").to_string()),
                                ("write", quote(write)),
                            ])
                        })),
                    ),
                ]))?;
            } else {
                emit(
                    "bdf  document + linear mesh; geometry export\nvtu  ASCII XML mesh + numeric fields, read/write\nvtk  ASCII legacy unstructured grid + numeric fields, read/write\nmsh  ASCII 4.1/2.2 mesh + numeric fields, read/write (output defaults to 4.1)\ninp  flat mesh subset, read/geometry write\nfrd  ASCII mesh + nodal fields, read/write\nop2  32-bit real OUGV1 displacement, read/write; explicit synthetic-zero option; optional companion mesh\npch  ASCII real SORT1 displacement, read-only; matching mesh required\nstl  ASCII/binary triangle surface, binary write; no IDs or fields\nsu2  ASCII mesh and named boundary markers, read/write",
                )?;
            }
            return Ok(0);
        }
        _ => {}
    }

    // Conversion owns staged output; inspection dispatches BDF documents to
    // their richer source-preserving report path.
    if args.command == "convert" {
        return run_convert(args);
    }
    let format = format_of(args)?;
    if format != Format::Bdf && matches!(args.command.as_str(), "info" | "validate") {
        return run_generic_info(args);
    }
    if args.mesh.is_some()
        || args.assume_basic_frame
        || args.subcase.is_some()
        || args.step.is_some()
    {
        return Err(usage(
            "result mesh, frame, subcase, and step options do not apply to BDF inspection",
        ));
    }
    let doc = read_bdf(
        &args.paths[0],
        args.max_bytes.unwrap_or(ParseOptions::default().max_bytes),
    )?;
    match args.command.as_str() {
        "info" => {
            // Report document counts separately from scoped geometry checks.
            let counts = doc.card_counts();
            let report = doc.validate_geometry();
            if args.json {
                emit(&object([
                    ("schema_version", "1".into()),
                    ("format", quote("bdf")),
                    ("path", path_json(&args.paths[0])),
                    ("bytes", doc.to_bytes().len().to_string()),
                    ("full_deck", doc.is_full_deck().to_string()),
                    (
                        "card_counts",
                        object(
                            counts
                                .iter()
                                .map(|(key, value)| (key.as_str(), value.to_string())),
                        ),
                    ),
                    ("units", quote("unspecified")),
                    ("geometry", report_json(&report)),
                ]))?;
            } else {
                emit(&format!(
                    "BDF document: {}\nBytes: {}\nFull deck: {}\nUnits: unspecified",
                    args.paths[0].display(),
                    doc.to_bytes().len(),
                    doc.is_full_deck()
                ))?;
                for (name, count) in counts {
                    emit(&format!("  {name:<10} {count}"))?;
                }
                emit(&format!(
                    "Geometry projection available: {} ({} error(s), {} warning(s))",
                    report.valid_in_scope(),
                    report.error_count(),
                    report.warning_count()
                ))?;
                for d in report
                    .diagnostics
                    .iter()
                    .filter(|d| d.severity == Severity::Error)
                    .take(5)
                {
                    emit(&format!("  {}: {}", d.code, d.message))?;
                }
            }
        }
        "validate" => {
            // Warnings only fail when strict validation was requested.
            let report = doc.validate_geometry();
            let passed = report.valid_in_scope() && (!args.strict || report.warning_count() == 0);
            if args.json {
                emit(&object([
                    ("schema_version", "1".into()),
                    ("strict", args.strict.to_string()),
                    ("passed", passed.to_string()),
                    ("validation", report_json(&report)),
                ]))?;
            } else {
                emit(&format!(
                    "Geometry-subset checks: {}. Not a full Nastran solver validation.",
                    if passed { "passed" } else { "failed" }
                ))?;
                for d in &report.diagnostics {
                    emit(&format!(
                        "{:?} {}{}: {}",
                        d.severity,
                        d.code,
                        d.line
                            .map_or_else(String::new, |line| format!(" at line {line}")),
                        d.message
                    ))?;
                }
            }
            return Ok(u8::from(!passed));
        }
        _ => return Err(usage("unknown command")),
    }
    Ok(0)
}

/// Convert structured failures into human or JSON diagnostics and exit codes.
fn main() {
    // Retain JSON error formatting even when argument parsing itself fails.
    let raw: Vec<OsString> = std::env::args_os().skip(1).collect();
    let wants_json = raw.iter().any(|arg| arg == "--json");
    let status = match parse_args(&raw).and_then(|args| run(&args)) {
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

#[cfg(test)]
mod tests {
    use super::*;
    fn args(values: &[&str]) -> Result<Args> {
        let raw: Vec<OsString> = values.iter().map(OsString::from).collect();
        parse_args(&raw)
    }
    #[test]
    fn conversion_requires_acknowledgement() {
        assert!(args(&["convert", "x.bdf", "x.vtu"]).is_err());
    }
    #[test]
    fn grid_edit_is_not_a_cli_command() {
        assert!(args(&["set-grid", "x.bdf", "y.bdf"]).is_err());
    }
    #[test]
    fn duplicate_option_rejected() {
        assert!(args(&["info", "x.bdf", "--json", "--json"]).is_err());
    }
    #[test]
    fn irrelevant_flag_rejected() {
        assert!(args(&["info", "x.bdf", "--strict"]).is_err());
    }
    #[test]
    fn msh_dialect_is_selected_only_for_msh_output() {
        let selected = args(&[
            "convert",
            "x.bdf",
            "x.msh",
            "--msh-version",
            "2.2",
            "--accept-projection",
        ])
        .unwrap();
        assert_eq!(selected.msh_version, Some(msh::Version::V2_2));
        assert!(
            args(&[
                "convert",
                "x.msh",
                "x.vtu",
                "--msh-version",
                "2.2",
                "--accept-projection"
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
                "--accept-projection"
            ])
            .is_err()
        );
    }
    #[test]
    fn double_dash_supports_dash_path() {
        assert_eq!(
            args(&["info", "--", "-mesh.bdf"]).unwrap().paths[0],
            PathBuf::from("-mesh.bdf")
        );
    }
    #[test]
    fn typo_command_rejected() {
        assert!(args(&["convertx"]).is_err());
    }
    #[test]
    fn bad_arity_rejected() {
        assert!(args(&["convert", "only.bdf", "--accept-projection"]).is_err());
    }
    #[test]
    fn format_report_takes_no_paths() {
        assert!(args(&["formats", "x.bdf"]).is_err());
    }
    #[test]
    fn assumed_zero_requires_op2_output_and_no_rotation_fill_flag() {
        assert!(
            args(&[
                "convert",
                "x.bdf",
                "x.op2",
                "--accept-projection",
                "--assume-zero-displacement"
            ])
            .is_ok()
        );
        assert!(
            args(&[
                "convert",
                "x.bdf",
                "x.vtu",
                "--accept-projection",
                "--assume-zero-displacement"
            ])
            .is_err()
        );
        assert!(
            args(&[
                "convert",
                "x.bdf",
                "x.op2",
                "--accept-projection",
                "--assume-zero-displacement",
                "--zero-missing-rotations"
            ])
            .is_err()
        );
    }
}
