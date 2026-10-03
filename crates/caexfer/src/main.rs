mod json;
mod output;

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use caexfer_core::{Dataset, Error, FieldLocation, Result, Severity, ValidationReport};
use caexfer_formats::{
    bdf::{parse_real, Document, ParseOptions},
    bdf_mesh, frd, inp, msh, op2, vtu,
};
use json::{array, object, quote};

const HELP: &str = "caexfer 0.1.0 — inspect, preserve, and explicitly project engineering files

USAGE
  caexfer formats [--json]
  caexfer info INPUT [--json]
  caexfer validate INPUT [--strict] [--json]
  caexfer roundtrip INPUT OUTPUT [--json]
  caexfer set-grid INPUT OUTPUT --id ID --xyz X Y Z [--json]
  caexfer convert INPUT OUTPUT --geometry-only [--json]

COMMON OPTIONS
  --from FORMAT    bdf, vtu, msh, inp, frd, or op2; otherwise infer extension
  --mesh BDF       Required BDF geometry for OP2 results
  --python PATH    Python with pyNastran installed for OP2 (default: python3)
  --zero-missing-rotations  Confirm absent R1/R2/R3 are known float 0.0 for OP2 output
  --subcase N      OP2 displacement subcase if more than one exists
  --step N         Zero-based OP2 result step, or FRD step number
  --max-bytes N    Input byte limit (default: 268435456)
  --json           Machine-readable output (schema_version=1)
  --               Treat remaining arguments as file paths
  -h, --help       Show this help
  -V, --version    Show version

SCOPE
  BDF: source-preserving small/large/free fields, GRID editing, linear geometry.
  VTU: ASCII linear mesh and numeric fields. MSH: ASCII 4.1 subset.
  INP: flat mesh subset. FRD: ASCII mesh and nodal results subset.
  OP2: real displacement table via optional pyNastran; read needs matching BDF.
  validate checks the GEOMETRY SUBSET, not complete solver validity.
  set-grid coordinates are in the GRID's native CP frame.
  convert requires --geometry-only and reports omitted solver information.
  Unresolved INCLUDEs, nonbasic CP, GRDSET, higher-order/unknown geometry fail.
  OP2 output contains results only; export a matching BDF separately.
  Output files must not already exist.
";

#[derive(Debug, Default)]
struct Args {
    command: String,
    paths: Vec<PathBuf>,
    json: bool,
    strict: bool,
    geometry_only: bool,
    zero_missing_rotations: bool,
    from: Option<String>,
    mesh: Option<PathBuf>,
    python: Option<PathBuf>,
    subcase: Option<i64>,
    step: Option<usize>,
    max_bytes: Option<usize>,
    id: Option<u64>,
    xyz: Option<[f64; 3]>,
}

fn usage(message: impl Into<String>) -> Error {
    Error::new("E_USAGE", message)
}

fn text(arg: &OsStr) -> Result<&str> {
    arg.to_str()
        .ok_or_else(|| usage("option value is not valid UTF-8"))
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
        "formats" | "info" | "validate" | "roundtrip" | "set-grid" | "convert"
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
            if args.json {
                return Err(usage("duplicate --json"));
            }
            args.json = true;
        } else if options && arg == "--strict" {
            if args.strict {
                return Err(usage("duplicate --strict"));
            }
            args.strict = true;
        } else if options && arg == "--geometry-only" {
            if args.geometry_only {
                return Err(usage("duplicate --geometry-only"));
            }
            args.geometry_only = true;
        } else if options && arg == "--zero-missing-rotations" {
            if args.zero_missing_rotations {
                return Err(usage("duplicate --zero-missing-rotations"));
            }
            args.zero_missing_rotations = true;
        } else if options && arg == "--from" {
            if args.from.is_some() {
                return Err(usage("duplicate --from"));
            }
            args.from = Some(value(1)?.to_string());
            index += 1;
        } else if options && arg == "--mesh" {
            if args.mesh.is_some() {
                return Err(usage("duplicate --mesh"));
            }
            args.mesh = Some(PathBuf::from(value(1)?));
            index += 1;
        } else if options && arg == "--python" {
            if args.python.is_some() {
                return Err(usage("duplicate --python"));
            }
            args.python = Some(PathBuf::from(value(1)?));
            index += 1;
        } else if options && arg == "--subcase" {
            if args.subcase.is_some() {
                return Err(usage("duplicate --subcase"));
            }
            args.subcase = Some(
                value(1)?
                    .parse()
                    .map_err(|_| usage("--subcase requires an integer"))?,
            );
            index += 1;
        } else if options && arg == "--step" {
            if args.step.is_some() {
                return Err(usage("duplicate --step"));
            }
            args.step = Some(
                value(1)?
                    .parse()
                    .map_err(|_| usage("--step requires a nonnegative integer"))?,
            );
            index += 1;
        } else if options && arg == "--max-bytes" {
            if args.max_bytes.is_some() {
                return Err(usage("duplicate --max-bytes"));
            }
            let limit = value(1)?
                .parse()
                .map_err(|_| usage("--max-bytes requires a positive integer"))?;
            if limit == 0 {
                return Err(usage("--max-bytes must be positive"));
            }
            args.max_bytes = Some(limit);
            index += 1;
        } else if options && arg == "--id" {
            if args.id.is_some() {
                return Err(usage("duplicate --id"));
            }
            let id = value(1)?
                .parse()
                .map_err(|_| usage("--id requires a positive integer"))?;
            if id == 0 {
                return Err(usage("--id must be positive"));
            }
            args.id = Some(id);
            index += 1;
        } else if options && arg == "--xyz" {
            if args.xyz.is_some() {
                return Err(usage("duplicate --xyz"));
            }
            args.xyz = Some([
                parse_real(value(1)?)?,
                parse_real(value(2)?)?,
                parse_real(value(3)?)?,
            ]);
            index += 3;
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
    if args.zero_missing_rotations
        && (args.command != "convert"
            || args
                .paths
                .get(1)
                .and_then(|path| path.extension())
                .and_then(OsStr::to_str)
                .is_none_or(|extension| !extension.eq_ignore_ascii_case("op2")))
    {
        return Err(usage("--zero-missing-rotations applies only to OP2 output"));
    }
    if args.command == "convert" && !args.geometry_only {
        return Err(usage("conversion projects the supported mesh/field subset; pass --geometry-only to acknowledge omitted information"));
    }
    if args.command == "set-grid" {
        if args.id.is_none() || args.xyz.is_none() {
            return Err(usage("set-grid requires --id and --xyz"));
        }
    } else if args.id.is_some() || args.xyz.is_some() {
        return Err(usage("--id and --xyz are only for set-grid"));
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

fn open_document(args: &Args) -> Result<Document> {
    let path = &args.paths[0];
    if let Some(format) = &args.from {
        if format != "bdf" {
            return Err(Error::new(
                "E_FORMAT",
                "set-grid and native BDF document operations require --from bdf",
            ));
        }
    } else {
        let extension = path
            .extension()
            .and_then(OsStr::to_str)
            .unwrap_or("")
            .to_ascii_lowercase();
        if !matches!(extension.as_str(), "bdf" | "nas" | "dat" | "pch") {
            return Err(Error::new("E_FORMAT", format!("BDF document operations require a BDF extension or --from bdf; got {extension:?}")));
        }
    }
    let options = ParseOptions {
        max_bytes: args.max_bytes.unwrap_or(ParseOptions::default().max_bytes),
        ..ParseOptions::default()
    };
    Document::read_with_options(File::open(path)?, options)
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
    let writing_op2 = args.command == "convert"
        && args
            .paths
            .get(1)
            .and_then(|path| path.extension())
            .and_then(OsStr::to_str)
            .is_some_and(|extension| extension.eq_ignore_ascii_case("op2"));
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
            let doc = Document::read_with_options(
                File::open(&args.paths[0])?,
                ParseOptions {
                    max_bytes: max,
                    ..ParseOptions::default()
                },
            )?;
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
            let mesh_doc = Document::read_with_options(
                File::open(mesh_path)?,
                ParseOptions {
                    max_bytes: max,
                    ..ParseOptions::default()
                },
            )?;
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
            let python = args
                .python
                .clone()
                .or_else(|| std::env::var_os("CAEXFER_PYTHON").map(PathBuf::from))
                .unwrap_or_else(|| PathBuf::from("python3"));
            let dataset = op2::read_displacements(
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
                    bdf_mesh::write(&dataset.mesh, writer)
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
            let python = args
                .python
                .clone()
                .or_else(|| std::env::var_os("CAEXFER_PYTHON").map(PathBuf::from))
                .unwrap_or_else(|| PathBuf::from("python3"));
            let bytes = op2::write_displacements(
                &dataset,
                field,
                &python,
                subcase,
                args.zero_missing_rotations,
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
                        "real displacement via pyNastran; no embedded mesh",
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
                emit("bdf  document + linear mesh; document copy and geometry export\nvtu  ASCII mesh + numeric fields, read/write\nmsh  ASCII 4.1 mesh + numeric fields, read/write\ninp  flat mesh subset, read/geometry write\nfrd  ASCII mesh + nodal fields, read/write\nop2  real displacement via pyNastran, read/write; separate BDF mesh")?;
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
    let mut doc = open_document(&args)?;
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
        "roundtrip" | "set-grid" => {
            if args.command == "set-grid" {
                let id = args.id.ok_or_else(|| usage("missing --id"))?;
                let xyz = args.xyz.ok_or_else(|| usage("missing --xyz"))?;
                doc.set_grid_coordinates(id, xyz)?;
            }
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
                    if args.command == "roundtrip" {
                        "source bytes unchanged"
                    } else {
                        "only requested native-frame GRID coordinate fields changed"
                    }
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
    fn negative_coordinate_values_are_not_options() {
        assert_eq!(
            args(&["set-grid", "x.bdf", "y.bdf", "--id", "1", "--xyz", "-1", "2", "-3"])
                .unwrap()
                .xyz,
            Some([-1., 2., -3.])
        );
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
}
