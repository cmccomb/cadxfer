mod json;
mod output;

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use caexfer::core::{Dataset, Error, Field, FieldLocation, Result, Severity, ValidationReport};
use caexfer::{
    bdf::{self, Document, ParseOptions},
    frd, inp, msh, op2, vtu,
};
use json::{array, object, quote};

const SYNTHETIC_OP2_SOURCE_NOTICE: &str =
    "OP2 title marks this as a synthetic all-zero displacement table, not solver results";

const HELP: &str = "caexfer — inspect, preserve, and explicitly project engineering files

USAGE
  caexfer formats [--json]
  caexfer info INPUT [--json]
  caexfer validate INPUT [--strict] [--json]
  caexfer roundtrip INPUT OUTPUT [--json]
  caexfer convert INPUT OUTPUT --geometry-only [--json]

COMMON OPTIONS
  --from FORMAT    bdf, vtu, msh, inp, frd, or op2; otherwise infer extension
  --mesh BDF       Required BDF geometry for OP2 results
  --python PATH    Python with pyNastran installed for OP2 (or CAEXFER_PYTHON; default: python3)
  --zero-missing-rotations  Confirm absent R1/R2/R3 are known float 0.0 for OP2 output
  --assume-zero-displacement  Create a synthetic all-zero OP2 displacement table from BDF/INP
  --subcase N      OP2 displacement subcase if more than one exists
  --step N         Zero-based OP2 result step, or FRD step number
  --max-bytes N    Input byte limit (default: 268435456)
  --json           Machine-readable output (schema_version=1)
  --               Treat remaining arguments as file paths
  -h, --help       Show this help
  -V, --version    Show version

SCOPE
  BDF copy preserves the native document; conversion projects a subset.
  validate is scoped, not full solver validation. Conversion reports omissions.
  OP2 needs pyNastran; reading needs a matching BDF; output contains no mesh.
  Assumed-zero OP2 values are hypothetical, not solver results.
  Output paths must be new.

EXAMPLES
  caexfer info model.bdf
  caexfer convert model.bdf model.vtu --geometry-only
  caexfer convert results.op2 results.vtu --mesh model.bdf --geometry-only

Format limits: https://github.com/cmccomb/caexfer/blob/main/docs/SUPPORT.md
";

#[derive(Debug, Default)]
struct Args {
    command: String,
    paths: Vec<PathBuf>,
    json: bool,
    strict: bool,
    geometry_only: bool,
    zero_missing_rotations: bool,
    assume_zero_displacement: bool,
    from: Option<String>,
    mesh: Option<PathBuf>,
    python: Option<PathBuf>,
    subcase: Option<i64>,
    step: Option<usize>,
    max_bytes: Option<usize>,
}

fn usage(message: impl Into<String>) -> Error {
    Error::new("E_USAGE", message)
}

fn text(arg: &OsStr) -> Result<&str> {
    arg.to_str()
        .ok_or_else(|| usage("option value is not valid UTF-8"))
}

fn set_flag(slot: &mut bool, name: &str) -> Result<()> {
    if *slot {
        return Err(usage(format!("duplicate {name}")));
    }
    *slot = true;
    Ok(())
}

fn set_option<T>(
    slot: &mut Option<T>,
    name: &str,
    parse: impl FnOnce() -> Result<T>,
) -> Result<()> {
    if slot.is_some() {
        return Err(usage(format!("duplicate {name}")));
    }
    *slot = Some(parse()?);
    Ok(())
}

fn is_op2_output(args: &Args) -> bool {
    args.command == "convert"
        && args
            .paths
            .get(1)
            .and_then(|path| path.extension())
            .and_then(OsStr::to_str)
            .is_some_and(|extension| extension.eq_ignore_ascii_case("op2"))
}

fn python(args: &Args) -> PathBuf {
    args.python
        .clone()
        .or_else(|| std::env::var_os("CAEXFER_PYTHON").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("python3"))
}

fn parse_args(raw: Vec<OsString>) -> Result<Args> {
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
        "formats" | "info" | "validate" | "roundtrip" | "convert"
    ) {
        return Err(usage(format!("unknown command {command:?}; use --help")));
    }
    let mut args = Args {
        command,
        ..Args::default()
    };
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
        if options && arg == "--json" {
            set_flag(&mut args.json, "--json")?;
        } else if options && arg == "--strict" {
            set_flag(&mut args.strict, "--strict")?;
        } else if options && arg == "--geometry-only" {
            set_flag(&mut args.geometry_only, "--geometry-only")?;
        } else if options && arg == "--zero-missing-rotations" {
            set_flag(&mut args.zero_missing_rotations, "--zero-missing-rotations")?;
        } else if options && arg == "--assume-zero-displacement" {
            set_flag(
                &mut args.assume_zero_displacement,
                "--assume-zero-displacement",
            )?;
        } else if options && arg == "--from" {
            set_option(&mut args.from, "--from", || Ok(value(1)?.to_string()))?;
            index += 1;
        } else if options && arg == "--mesh" {
            set_option(&mut args.mesh, "--mesh", || Ok(PathBuf::from(value(1)?)))?;
            index += 1;
        } else if options && arg == "--python" {
            set_option(&mut args.python, "--python", || {
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
    if args.geometry_only && args.command != "convert" {
        return Err(usage("--geometry-only is only for convert"));
    }
    let op2_output = is_op2_output(&args);
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
    if args.command == "convert" && !args.geometry_only {
        return Err(usage("conversion projects the supported mesh/field subset; pass --geometry-only to acknowledge omitted information"));
    }
    if args.command == "formats"
        && (args.from.is_some()
            || args.max_bytes.is_some()
            || args.mesh.is_some()
            || args.python.is_some()
            || args.subcase.is_some()
            || args.step.is_some())
    {
        return Err(usage("formats does not read an input"));
    }
    if !matches!(args.command.as_str(), "convert" | "info" | "validate")
        && (args.mesh.is_some()
            || args.python.is_some()
            || args.subcase.is_some()
            || args.step.is_some())
    {
        return Err(usage(
            "--mesh, --python, --subcase and --step apply to conversion or inspection",
        ));
    }
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

fn read_bdf(path: &Path, max_bytes: usize) -> Result<Document> {
    Document::read_with_options(
        File::open(path)?,
        ParseOptions {
            max_bytes,
            ..ParseOptions::default()
        },
    )
}

fn path_json(path: &Path) -> String {
    quote(&path.to_string_lossy())
}

fn format_of(args: &Args) -> Result<String> {
    if let Some(format) = &args.from {
        return Ok(format.to_ascii_lowercase());
    }
    let ext = args.paths[0]
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "bdf" | "nas" | "dat" | "pch" => Ok("bdf".into()),
        "vtu" | "msh" | "inp" | "frd" | "op2" => Ok(ext),
        _ => Err(Error::new(
            "E_FORMAT",
            format!("no reader for extension {ext:?}; use --from"),
        )),
    }
}
fn read_limited(path: &Path, max: usize) -> Result<Vec<u8>> {
    let len = std::fs::metadata(path)?.len();
    if len > max as u64 {
        return Err(Error::new("E_LIMIT", format!("input exceeds {max} bytes")));
    }
    Ok(std::fs::read(path)?)
}
fn read_dataset(args: &Args) -> Result<(Dataset, Vec<String>)> {
    let format = format_of(args)?;
    let max = args.max_bytes.unwrap_or(ParseOptions::default().max_bytes);
    let writing_op2 = is_op2_output(args);
    if format != "op2"
        && (args.mesh.is_some()
            || args.subcase.is_some()
            || (args.python.is_some() && !writing_op2))
    {
        return Err(usage(
            "--mesh and --subcase apply to OP2 input; --python also applies to OP2 output",
        ));
    }
    if !matches!(format.as_str(), "op2" | "frd") && args.step.is_some() {
        return Err(usage("--step applies only to OP2 or FRD"));
    }
    match format.as_str() {
        "bdf" => {
            let doc = read_bdf(&args.paths[0], max)?;
            if args.assume_zero_displacement {
                for grid in doc.grids() {
                    if grid?.cd != 0 {
                        return Err(Error::new(
                            "E_OP2",
                            "synthetic OP2 output requires basic-frame GRID CD=0 for matching BDF reread",
                        ));
                    }
                }
            }
            let projection = doc.geometry()?;
            let omitted = projection
                .omissions
                .iter()
                .map(|v| format!("{} × {}: {}", v.category, v.count, v.detail))
                .collect();
            Ok((
                Dataset {
                    mesh: projection.mesh,
                    fields: Vec::new(),
                },
                omitted,
            ))
        }
        "vtu" => {
            let bytes = read_limited(&args.paths[0], max)?;
            let text = std::str::from_utf8(&bytes)
                .map_err(|_| Error::new("E_VTU", "VTU must be UTF-8 XML"))?;
            Ok((vtu::read(text)?, Vec::new()))
        }
        "msh" => {
            let bytes = read_limited(&args.paths[0], max)?;
            let text = std::str::from_utf8(&bytes)
                .map_err(|_| Error::new("E_MSH", "MSH must be UTF-8 ASCII"))?;
            let dataset = msh::read(text)?;
            let mut omissions = Vec::new();
            for section in text
                .lines()
                .filter_map(|line| line.trim().strip_prefix('$'))
                .filter(|name| !name.starts_with("End"))
            {
                if !matches!(
                    section,
                    "MeshFormat" | "Nodes" | "Elements" | "NodeData" | "ElementData"
                ) {
                    omissions.push(format!(
                        "MSH ${section} section is not represented in the projection"
                    ));
                }
            }
            Ok((dataset, omissions))
        }
        "inp" => {
            let bytes = read_limited(&args.paths[0], max)?;
            let text = std::str::from_utf8(&bytes)
                .map_err(|_| Error::new("E_INP", "INP must be UTF-8 text"))?;
            let parsed = inp::read(text)?;
            let omissions = parsed
                .omitted_keywords
                .into_iter()
                .map(|v| format!("INP keyword *{v} is absent from mesh projection"))
                .collect();
            Ok((
                Dataset {
                    mesh: parsed.mesh,
                    fields: Vec::new(),
                },
                omissions,
            ))
        }
        "frd" => {
            let bytes = read_limited(&args.paths[0], max)?;
            let mut dataset = frd::read(&bytes)?;
            if let Some(step) = args.step {
                dataset.fields.retain(|f| f.step == Some(step as i64));
                if dataset.fields.is_empty() {
                    return Err(Error::new("E_FRD", "selected step has no fields"));
                }
            }
            let mut omissions = Vec::new();
            if bytes.windows(2).any(|pair| pair == b"1U" || pair == b"1P") {
                omissions.push(
                    "FRD user/model parameter metadata is not represented in the projection".into(),
                );
            }
            Ok((dataset, omissions))
        }
        "op2" => {
            let mesh_path = args
                .mesh
                .as_ref()
                .ok_or_else(|| usage("OP2 requires --mesh matching.bdf"))?;
            let mesh_doc = read_bdf(mesh_path, max)?;
            for grid in mesh_doc.grids() {
                if grid?.cd != 0 {
                    return Err(Error::new(
                        "E_OP2",
                        "nonbasic GRID CD requires displacement frame transformation",
                    ));
                }
            }
            let projection = mesh_doc.geometry()?;
            if std::fs::metadata(&args.paths[0])?.len() > max as u64 {
                return Err(Error::new("E_LIMIT", "OP2 exceeds input byte limit"));
            }
            let python = python(args);
            let (dataset, assumed_zero) = op2::read_displacements(
                &args.paths[0],
                &projection.mesh,
                &python,
                args.subcase,
                args.step,
            )?;
            let mut omissions: Vec<String> = projection
                .omissions
                .iter()
                .map(|v| format!("BDF {} × {}: {}", v.category, v.count, v.detail))
                .collect();
            if assumed_zero {
                omissions.push(SYNTHETIC_OP2_SOURCE_NOTICE.into());
            }
            omissions.push("OP2 result tables other than the selected real displacement table are not exported".into());
            Ok((dataset, omissions))
        }
        _ => Err(Error::new(
            "E_FORMAT",
            format!("unknown input format {format}"),
        )),
    }
}
fn run_convert(args: &Args) -> Result<u8> {
    let (mut dataset, mut omissions) = read_dataset(args)?;
    let synthetic_op2_source = omissions
        .iter()
        .any(|notice| notice == SYNTHETIC_OP2_SOURCE_NOTICE);
    let ext = args.paths[1]
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "vtu" => {
            // A single VTU piece cannot carry multiple versions of the same named field.
            let mut seen = std::collections::BTreeSet::new();
            for field in &dataset.fields {
                if !seen.insert((field.location as u8, field.name.clone())) {
                    return Err(Error::new(
                        "E_VTU",
                        "multiple steps of one field; choose --step",
                    ));
                }
            }
            output::create_new(&args.paths[1], |writer| vtu::write_data(&dataset, writer))?;
        }
        "msh" => {
            let missing_steps = dataset
                .fields
                .iter()
                .filter(|field| field.step.is_none())
                .count();
            if missing_steps > 0 {
                omissions.push(format!(
                    "{missing_steps} field(s) without step metadata use MSH step 0"
                ));
            }
            let labels = dataset
                .fields
                .iter()
                .filter(|field| {
                    field
                        .components
                        .iter()
                        .enumerate()
                        .any(|(i, name)| name != &format!("C{}", i + 1))
                })
                .count();
            if labels > 0 {
                omissions.push(format!(
                    "{labels} field(s) lose component labels in MSH NodeData/ElementData"
                ));
            }
            let props = dataset
                .mesh
                .cells
                .iter()
                .filter(|c| c.property_id.is_some())
                .count();
            if props > 0 {
                omissions.push(format!(
                    "{props} property IDs have no MSH entity mapping in this exporter"
                ));
            }
            for cell in &mut dataset.mesh.cells {
                cell.property_id = None;
            }
            output::create_new(&args.paths[1], |writer| msh::write(&dataset, writer))?;
        }
        "inp" | "bdf" | "nas" => {
            if !dataset.fields.is_empty() {
                omissions.push(format!(
                    "{} numeric field(s) omitted from geometry-only solver input",
                    dataset.fields.len()
                ));
            }
            if ext == "inp" {
                let props = dataset
                    .mesh
                    .cells
                    .iter()
                    .filter(|c| c.property_id.is_some())
                    .count();
                if props > 0 {
                    omissions.push(format!("{props} property IDs omitted from INP"));
                }
                for cell in &mut dataset.mesh.cells {
                    cell.property_id = None;
                }
                output::create_new(&args.paths[1], |writer| inp::write(&dataset.mesh, writer))?;
            } else {
                let missing = dataset
                    .mesh
                    .cells
                    .iter()
                    .filter(|cell| cell.property_id.is_none())
                    .count();
                if missing > 0 {
                    omissions.push(format!("{missing} BDF element(s) use placeholder PID 1; no property cards are emitted"));
                }
                output::create_new(&args.paths[1], |writer| {
                    bdf::mesh::write(&dataset.mesh, writer)
                })?;
            }
        }
        "frd" => {
            let before = dataset.fields.len();
            dataset
                .fields
                .retain(|field| field.location == FieldLocation::Point);
            if dataset.fields.len() != before {
                omissions.push(format!(
                    "{} cell field(s) have no direct FRD nodal representation",
                    before - dataset.fields.len()
                ));
            }
            for field in &mut dataset.fields {
                if field.name.starts_with("DISPLACEMENT_SUBCASE_") {
                    omissions.push(format!(
                        "field {} is named DISP in FRD; subcase name is not retained",
                        field.name
                    ));
                    field.name = "DISP".into();
                }
                if field.step.is_none() || field.time.is_none() {
                    omissions.push(format!(
                        "field {} uses FRD step/time 0 where metadata is absent",
                        field.name
                    ));
                }
            }
            if dataset
                .mesh
                .cells
                .iter()
                .any(|cell| cell.property_id.is_some())
            {
                omissions.push("BDF property IDs have no direct FRD mesh mapping".into());
            }
            omissions.push(
                "FRD ASCII E12.5 rounds coordinates and field values to six significant digits"
                    .into(),
            );
            output::create_new(&args.paths[1], |writer| frd::write(&dataset, writer))?;
        }
        "op2" => {
            if args.assume_zero_displacement {
                let source_format = format_of(args)?;
                if !matches!(source_format.as_str(), "bdf" | "inp") {
                    return Err(usage(
                        "--assume-zero-displacement currently accepts BDF or INP input",
                    ));
                }
                if !dataset.fields.is_empty() {
                    return Err(Error::new(
                        "E_OP2",
                        "zero-displacement assumption requires a result-free input",
                    ));
                }
                let count = dataset.mesh.points.len().checked_mul(6).ok_or_else(|| {
                    Error::new("E_LIMIT", "too many nodes for synthetic displacement")
                })?;
                dataset.fields.push(Field {
                    name: "DISP".into(),
                    location: FieldLocation::Point,
                    components: ["T1", "T2", "T3", "R1", "R2", "R3"]
                        .map(str::to_owned)
                        .to_vec(),
                    values: vec![0.0; count],
                    step: None,
                    time: None,
                });
                omissions.push("SYNTHETIC ASSUMPTION: all six displacement components set to float 0.0 for every node; no solver analysis was performed".into());
            }
            let matches: Vec<_> = dataset
                .fields
                .iter()
                .filter(|field| {
                    let name = field.name.to_ascii_uppercase();
                    field.location == FieldLocation::Point
                        && (name == "DISP"
                            || name == "DISPLACEMENT"
                            || name.starts_with("DISPLACEMENT_SUBCASE_"))
                        && matches!(field.components.len(), 3 | 6)
                })
                .collect();
            if matches.len() != 1 {
                return Err(Error::new("E_OP2", "OP2 output requires exactly one 3- or 6-component nodal DISP field; select one result step"));
            }
            let field = matches[0];
            let subcase = field
                .name
                .to_ascii_uppercase()
                .strip_prefix("DISPLACEMENT_SUBCASE_")
                .and_then(|text| text.parse().ok())
                .unwrap_or(1);
            if field.components.len() == 3 && args.zero_missing_rotations {
                omissions.push(
                    "rotational displacement components R1/R2/R3 filled with typed float 0.0 by explicit request"
                        .into(),
                );
            }
            if field.step.is_some_and(|step| step != 0) {
                omissions
                    .push("source step number is not encoded in the one-step OP2 table".into());
            }
            if dataset.fields.len() > 1 {
                omissions.push(format!(
                    "{} other numeric field(s) omitted from OP2 displacement output",
                    dataset.fields.len() - 1
                ));
            }
            omissions
                .push("OP2 contains no mesh; export and keep a matching BDF separately".into());
            omissions.push("OP2 real displacement values use float32 precision".into());
            let python = python(args);
            let bytes = op2::write_displacements(
                &dataset,
                field,
                &python,
                subcase,
                args.zero_missing_rotations,
                args.assume_zero_displacement || synthetic_op2_source,
            )?;
            output::create_new(&args.paths[1], |writer| {
                writer.write_all(&bytes)?;
                Ok(())
            })?;
        }
        _ => {
            return Err(Error::new(
                "E_FORMAT",
                format!("no writer for extension {ext:?}"),
            ))
        }
    }
    if args.json {
        emit(&object([
            ("schema_version", "1".into()),
            ("operation", quote("mesh-and-fields-projection")),
            ("output", path_json(&args.paths[1])),
            ("points", dataset.mesh.points.len().to_string()),
            ("cells", dataset.mesh.cells.len().to_string()),
            ("fields", dataset.fields.len().to_string()),
            ("units", quote("unspecified")),
            (
                "omissions",
                array(omissions.iter().map(|detail| {
                    object([
                        ("category", quote("source-or-destination")),
                        ("count", "1".into()),
                        ("detail", quote(detail)),
                    ])
                })),
            ),
        ]))?;
    } else {
        emit(&format!(
            "Wrote {}: {} points, {} cells, {} source field(s). Units unspecified.",
            args.paths[1].display(),
            dataset.mesh.points.len(),
            dataset.mesh.cells.len(),
            dataset.fields.len()
        ))?;
        let mut stderr = std::io::stderr().lock();
        for omission in omissions {
            writeln!(stderr, "Omission: {omission}")?;
        }
    }
    Ok(0)
}
fn run_generic_info(args: &Args) -> Result<u8> {
    let format = format_of(args)?;
    let (dataset, omissions) = read_dataset(args)?;
    if args.command == "validate" {
        let passed = !args.strict || omissions.is_empty();
        if args.json {
            emit(&object([
                ("schema_version", "1".into()),
                ("format", quote(&format)),
                ("passed", passed.to_string()),
                ("strict", args.strict.to_string()),
                ("scope", quote("supported-mesh-and-fields-subset")),
                ("omissions", array(omissions.iter().map(|item| quote(item)))),
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
        return Ok(if passed { 0 } else { 1 });
    } else if args.json {
        emit(&object([
            ("schema_version", "1".into()),
            ("format", quote(&format)),
            ("path", path_json(&args.paths[0])),
            ("points", dataset.mesh.points.len().to_string()),
            ("cells", dataset.mesh.cells.len().to_string()),
            ("fields", dataset.fields.len().to_string()),
            ("omissions", array(omissions.iter().map(|item| quote(item)))),
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

fn run(args: Args) -> Result<u8> {
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
                    (
                        "bdf",
                        "document + linear mesh",
                        "document copy + geometry mesh",
                    ),
                    (
                        "vtu",
                        "ASCII one-piece linear mesh + numeric fields",
                        "ASCII one-piece linear mesh + numeric fields",
                    ),
                    (
                        "msh",
                        "ASCII MSH 4.1 linear mesh + complete numeric fields",
                        "ASCII MSH 4.1 linear mesh + numeric fields",
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
                        "real displacement via pyNastran and matching BDF",
                        "real displacement via pyNastran; explicit synthetic all-zero option; no embedded mesh",
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
                                ("write", quote(write)),
                            ])
                        })),
                    ),
                ]))?;
            } else {
                emit("bdf  document + linear mesh; document copy and geometry export\nvtu  ASCII mesh + numeric fields, read/write\nmsh  ASCII 4.1 mesh + numeric fields, read/write\ninp  flat mesh subset, read/geometry write\nfrd  ASCII mesh + nodal fields, read/write\nop2  real displacement via pyNastran, read/write; explicit synthetic-zero option; separate BDF mesh")?;
            }
            return Ok(0);
        }
        _ => {}
    }
    if args.command == "convert" {
        return run_convert(&args);
    }
    let format = format_of(&args)?;
    if format != "bdf" && matches!(args.command.as_str(), "info" | "validate") {
        return run_generic_info(&args);
    }
    if format != "bdf" && args.command == "roundtrip" {
        let bytes = read_limited(
            &args.paths[0],
            args.max_bytes.unwrap_or(ParseOptions::default().max_bytes),
        )?;
        output::create_new(&args.paths[1], |writer| {
            writer.write_all(&bytes)?;
            Ok(())
        })?;
        if args.json {
            emit(&object([
                ("schema_version", "1".into()),
                ("operation", quote("roundtrip")),
                ("bytes", bytes.len().to_string()),
            ]))?;
        } else {
            emit(&format!(
                "Wrote {} ({} source bytes unchanged).",
                args.paths[1].display(),
                bytes.len()
            ))?;
        }
        return Ok(0);
    }
    let doc = read_bdf(
        &args.paths[0],
        args.max_bytes.unwrap_or(ParseOptions::default().max_bytes),
    )?;
    match args.command.as_str() {
        "info" => {
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
            return Ok(if passed { 0 } else { 1 });
        }
        "roundtrip" => {
            output::create_new(&args.paths[1], |writer| doc.write_to(writer))?;
            if args.json {
                emit(&object([
                    ("schema_version", "1".into()),
                    ("operation", quote(&args.command)),
                    ("output", path_json(&args.paths[1])),
                    ("bytes", doc.to_bytes().len().to_string()),
                ]))?;
            } else {
                emit(&format!(
                    "Wrote {} ({} bytes; {}).",
                    args.paths[1].display(),
                    doc.to_bytes().len(),
                    "source bytes unchanged"
                ))?;
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
            if error.code == "E_USAGE" {
                2
            } else {
                1
            }
        }
    };
    std::process::exit(i32::from(status));
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(values: &[&str]) -> Result<Args> {
        parse_args(values.iter().map(OsString::from).collect())
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
        assert!(args(&["roundtrip", "only.bdf"]).is_err());
    }
    #[test]
    fn format_report_takes_no_paths() {
        assert!(args(&["formats", "x.bdf"]).is_err());
    }
    #[test]
    fn assumed_zero_requires_op2_output_and_no_rotation_fill_flag() {
        assert!(args(&[
            "convert",
            "x.bdf",
            "x.op2",
            "--geometry-only",
            "--assume-zero-displacement"
        ])
        .is_ok());
        assert!(args(&[
            "convert",
            "x.bdf",
            "x.vtu",
            "--geometry-only",
            "--assume-zero-displacement"
        ])
        .is_err());
        assert!(args(&[
            "convert",
            "x.bdf",
            "x.op2",
            "--geometry-only",
            "--assume-zero-displacement",
            "--zero-missing-rotations"
        ])
        .is_err());
    }
}
