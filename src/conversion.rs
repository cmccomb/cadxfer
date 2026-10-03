//! Format-aware conversion with explicit source and destination omissions.
//!
//! [`convert_path`] reads a supported source and writes its supported projection
//! to a caller-owned stream. The returned [`ConversionReport`] describes what
//! was carried and what was omitted. File persistence and overwrite policy
//! belong to the caller.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::bdf::{self, Document, ParseOptions};
use crate::core::{Dataset, Error, FieldLocation, Result};
use crate::{frd, inp, msh, op2, vtu};

/// Supported conversion format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Nastran Bulk Data deck.
    Bdf,
    /// VTK XML UnstructuredGrid.
    Vtu,
    /// Gmsh MSH 4.1.
    Msh,
    /// Abaqus or CalculiX input deck.
    Inp,
    /// CalculiX result file.
    Frd,
    /// Nastran displacement result file.
    Op2,
}

impl Format {
    /// Canonical lowercase format name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Bdf => "bdf",
            Self::Vtu => "vtu",
            Self::Msh => "msh",
            Self::Inp => "inp",
            Self::Frd => "frd",
            Self::Op2 => "op2",
        }
    }

    /// Parse a canonical format name, as used by `--from`.
    pub fn parse(name: &str) -> Result<Self> {
        match name.to_ascii_lowercase().as_str() {
            "bdf" => Ok(Self::Bdf),
            "vtu" => Ok(Self::Vtu),
            "msh" => Ok(Self::Msh),
            "inp" => Ok(Self::Inp),
            "frd" => Ok(Self::Frd),
            "op2" => Ok(Self::Op2),
            _ => Err(Error::new(
                "E_FORMAT",
                format!("unknown input format {name}"),
            )),
        }
    }

    /// Infer a source format from its extension. BDF aliases are accepted.
    pub fn from_input_path(path: &Path) -> Result<Self> {
        let extension = extension(path);
        match extension.as_str() {
            "bdf" | "nas" | "dat" | "pch" => Ok(Self::Bdf),
            "vtu" | "msh" | "inp" | "frd" | "op2" => Self::parse(&extension),
            _ => Err(Error::new(
                "E_FORMAT",
                format!("no reader for extension {extension:?}; use --from"),
            )),
        }
    }

    /// Infer a writable destination from its extension.
    pub fn from_output_path(path: &Path) -> Result<Self> {
        let extension = extension(path);
        match extension.as_str() {
            "bdf" | "nas" => Ok(Self::Bdf),
            "vtu" | "msh" | "inp" | "frd" | "op2" => Self::parse(&extension),
            _ => Err(Error::new(
                "E_FORMAT",
                format!("no writer for extension {extension:?}"),
            )),
        }
    }
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(OsStr::to_str)
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// Source selection, limits, and explicit result assumptions.
#[derive(Debug, Clone)]
pub struct Options {
    /// Override source extension detection.
    pub input_format: Option<Format>,
    /// Matching BDF geometry required when reading OP2.
    pub mesh: Option<PathBuf>,
    /// Python interpreter with pyNastran for OP2 reads and writes.
    pub python: PathBuf,
    /// OP2 displacement subcase, if more than one exists.
    pub subcase: Option<i64>,
    /// Zero-based OP2 result step, or FRD step number.
    pub step: Option<usize>,
    /// Maximum source bytes; the same bound applies to a matching BDF.
    pub max_bytes: usize,
    /// Assert that absent OP2 R1/R2/R3 components are known float zero.
    pub zero_missing_rotations: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            input_format: None,
            mesh: None,
            python: PathBuf::from("python3"),
            subcase: None,
            step: None,
            max_bytes: ParseOptions::default().max_bytes,
            zero_missing_rotations: false,
        }
    }
}

/// Where an omission or assumption enters a conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Information not represented when reading the source.
    Source,
    /// Information not representable by the destination.
    Destination,
    /// A value supplied by an explicit caller assumption.
    Assumption,
}

impl Stage {
    /// Stable lowercase name for machine-readable reports.
    pub fn name(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Destination => "destination",
            Self::Assumption => "assumption",
        }
    }
}

/// One piece of information omitted or explicitly assumed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Omission {
    /// Origin of the omission or assumption.
    pub stage: Stage,
    /// Human-readable detail; wording is not a stable API.
    pub detail: String,
}

impl Omission {
    fn new(stage: Stage, detail: impl Into<String>) -> Self {
        Self {
            stage,
            detail: detail.into(),
        }
    }
}

/// Supported source data and its read-side omissions.
#[derive(Debug, Clone)]
pub struct ReadResult {
    /// Detected or explicitly selected source format.
    pub format: Format,
    /// Projected mesh and fields.
    pub dataset: Dataset,
    /// Source information absent from the projected dataset.
    pub omissions: Vec<Omission>,
    /// Whether an OP2 title marks its result as synthetic all-zero data.
    pub assumed_zero: bool,
}

/// Counts and omissions for one conversion.
#[derive(Debug, Clone)]
pub struct ConversionReport {
    /// Number of source mesh points.
    pub points: usize,
    /// Number of source mesh cells.
    pub cells: usize,
    /// Number of source numeric fields before destination filtering.
    pub fields: usize,
    /// Source and destination omissions, including explicit assumptions.
    pub omissions: Vec<Omission>,
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

fn read_limited(path: &Path, max_bytes: usize) -> Result<Vec<u8>> {
    if std::fs::metadata(path)?.len() > max_bytes as u64 {
        return Err(Error::new(
            "E_LIMIT",
            format!("input exceeds {max_bytes} bytes"),
        ));
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take(max_bytes.saturating_add(1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > max_bytes {
        return Err(Error::new(
            "E_LIMIT",
            format!("input exceeds {max_bytes} bytes"),
        ));
    }
    Ok(bytes)
}

/// Read a supported source file into a mesh and fields, reporting omitted data.
/// OP2 input requires `options.mesh` and a pyNastran interpreter.
pub fn read_path(path: &Path, options: &Options) -> Result<ReadResult> {
    let format = options
        .input_format
        .map_or_else(|| Format::from_input_path(path), Ok)?;
    if format != Format::Op2 && (options.mesh.is_some() || options.subcase.is_some()) {
        return Err(Error::new(
            "E_USAGE",
            "mesh and subcase apply only to OP2 input",
        ));
    }
    if !matches!(format, Format::Op2 | Format::Frd) && options.step.is_some() {
        return Err(Error::new(
            "E_USAGE",
            "step applies only to OP2 or FRD input",
        ));
    }
    let (dataset, omissions, assumed_zero) = match format {
        Format::Bdf => {
            let projection = read_bdf(path, options.max_bytes)?.geometry()?;
            let omissions = projection
                .omissions
                .into_iter()
                .map(|item| {
                    Omission::new(
                        Stage::Source,
                        format!("{} × {}: {}", item.category, item.count, item.detail),
                    )
                })
                .collect();
            (
                Dataset {
                    mesh: projection.mesh,
                    fields: Vec::new(),
                },
                omissions,
                false,
            )
        }
        Format::Vtu => {
            let bytes = read_limited(path, options.max_bytes)?;
            let text = std::str::from_utf8(&bytes)
                .map_err(|_| Error::new("E_VTU", "VTU must be UTF-8 XML"))?;
            (vtu::read(text)?, Vec::new(), false)
        }
        Format::Msh => {
            let bytes = read_limited(path, options.max_bytes)?;
            let text = std::str::from_utf8(&bytes)
                .map_err(|_| Error::new("E_MSH", "MSH must be UTF-8 ASCII"))?;
            let dataset = msh::read(text)?;
            let omissions = text
                .lines()
                .filter_map(|line| line.trim().strip_prefix('$'))
                .filter(|name| !name.starts_with("End"))
                .filter(|section| {
                    !matches!(
                        *section,
                        "MeshFormat" | "Nodes" | "Elements" | "NodeData" | "ElementData"
                    )
                })
                .map(|section| {
                    Omission::new(
                        Stage::Source,
                        format!("MSH ${section} section is not represented in the projection"),
                    )
                })
                .collect();
            (dataset, omissions, false)
        }
        Format::Inp => {
            let bytes = read_limited(path, options.max_bytes)?;
            let text = std::str::from_utf8(&bytes)
                .map_err(|_| Error::new("E_INP", "INP must be UTF-8 text"))?;
            let parsed = inp::read(text)?;
            let omissions = parsed
                .omitted_keywords
                .into_iter()
                .map(|keyword| {
                    Omission::new(
                        Stage::Source,
                        format!("INP keyword *{keyword} is absent from mesh projection"),
                    )
                })
                .collect();
            (
                Dataset {
                    mesh: parsed.mesh,
                    fields: Vec::new(),
                },
                omissions,
                false,
            )
        }
        Format::Frd => {
            let bytes = read_limited(path, options.max_bytes)?;
            let mut dataset = frd::read(&bytes)?;
            if let Some(step) = options.step {
                dataset
                    .fields
                    .retain(|field| field.step == Some(step as i64));
                if dataset.fields.is_empty() {
                    return Err(Error::new("E_FRD", "selected step has no fields"));
                }
            }
            let mut omissions = Vec::new();
            if bytes.windows(2).any(|pair| pair == b"1U" || pair == b"1P") {
                omissions.push(Omission::new(
                    Stage::Source,
                    "FRD user/model parameter metadata is not represented in the projection",
                ));
            }
            (dataset, omissions, false)
        }
        Format::Op2 => {
            let mesh_path = options
                .mesh
                .as_deref()
                .ok_or_else(|| Error::new("E_USAGE", "OP2 requires --mesh matching.bdf"))?;
            let mesh_doc = read_bdf(mesh_path, options.max_bytes)?;
            for grid in mesh_doc.grids() {
                if grid?.cd != 0 {
                    return Err(Error::new(
                        "E_OP2",
                        "nonbasic GRID CD requires displacement frame transformation",
                    ));
                }
            }
            let projection = mesh_doc.geometry()?;
            if std::fs::metadata(path)?.len() > options.max_bytes as u64 {
                return Err(Error::new("E_LIMIT", "OP2 exceeds input byte limit"));
            }
            let (dataset, assumed_zero) = op2::read_displacements(
                path,
                &projection.mesh,
                &options.python,
                options.subcase,
                options.step,
            )?;
            let mut omissions: Vec<_> = projection
                .omissions
                .into_iter()
                .map(|item| {
                    Omission::new(
                        Stage::Source,
                        format!("BDF {} × {}: {}", item.category, item.count, item.detail),
                    )
                })
                .collect();
            if assumed_zero {
                omissions.push(Omission::new(
                    Stage::Assumption,
                    "OP2 title marks this as a synthetic all-zero displacement table, not solver results",
                ));
            }
            omissions.push(Omission::new(
                Stage::Source,
                "OP2 result tables other than the selected real displacement table are not exported",
            ));
            (dataset, omissions, assumed_zero)
        }
    };
    Ok(ReadResult {
        format,
        dataset,
        omissions,
        assumed_zero,
    })
}

/// Read a file and write its supported projection to `writer`.
///
/// This does not create or replace a destination file. Callers decide how to
/// persist the stream. The report identifies information lost or assumed.
pub fn convert_path(
    path: &Path,
    target: Format,
    options: &Options,
    writer: impl Write,
) -> Result<ConversionReport> {
    convert(read_path(path, options)?, target, options, writer)
}

/// Write a previously read source in another supported format.
///
/// OP2 output requires exactly one nodal displacement field and a pyNastran
/// interpreter. A caller may supply a synthetic field, but must set
/// `source.assumed_zero` to mark that provenance in the OP2 title.
pub fn convert(
    mut source: ReadResult,
    target: Format,
    options: &Options,
    mut writer: impl Write,
) -> Result<ConversionReport> {
    let source_fields = source.dataset.fields.len();
    let dataset = &mut source.dataset;
    let omissions = &mut source.omissions;
    match target {
        Format::Vtu => {
            let mut seen = BTreeSet::new();
            for field in &dataset.fields {
                if !seen.insert((field.location as u8, field.name.clone())) {
                    return Err(Error::new(
                        "E_VTU",
                        "multiple steps of one field; choose --step",
                    ));
                }
            }
            vtu::write_data(dataset, &mut writer)?;
        }
        Format::Msh => {
            let missing_steps = dataset
                .fields
                .iter()
                .filter(|field| field.step.is_none())
                .count();
            if missing_steps > 0 {
                omissions.push(Omission::new(
                    Stage::Destination,
                    format!("{missing_steps} field(s) without step metadata use MSH step 0"),
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
                omissions.push(Omission::new(
                    Stage::Destination,
                    format!("{labels} field(s) lose component labels in MSH NodeData/ElementData"),
                ));
            }
            let props = dataset
                .mesh
                .cells
                .iter()
                .filter(|cell| cell.property_id.is_some())
                .count();
            if props > 0 {
                omissions.push(Omission::new(
                    Stage::Destination,
                    format!("{props} property IDs have no MSH entity mapping in this exporter"),
                ));
            }
            for cell in &mut dataset.mesh.cells {
                cell.property_id = None;
            }
            msh::write(dataset, &mut writer)?;
        }
        Format::Inp | Format::Bdf => {
            if !dataset.fields.is_empty() {
                omissions.push(Omission::new(
                    Stage::Destination,
                    format!(
                        "{} numeric field(s) omitted from geometry-only solver input",
                        dataset.fields.len()
                    ),
                ));
            }
            if target == Format::Inp {
                let props = dataset
                    .mesh
                    .cells
                    .iter()
                    .filter(|cell| cell.property_id.is_some())
                    .count();
                if props > 0 {
                    omissions.push(Omission::new(
                        Stage::Destination,
                        format!("{props} property IDs omitted from INP"),
                    ));
                }
                for cell in &mut dataset.mesh.cells {
                    cell.property_id = None;
                }
                inp::write(&dataset.mesh, &mut writer)?;
            } else {
                let missing = dataset
                    .mesh
                    .cells
                    .iter()
                    .filter(|cell| cell.property_id.is_none())
                    .count();
                if missing > 0 {
                    omissions.push(Omission::new(
                        Stage::Destination,
                        format!("{missing} BDF element(s) use placeholder PID 1; no property cards are emitted"),
                    ));
                }
                bdf::mesh::write(&dataset.mesh, &mut writer)?;
            }
        }
        Format::Frd => {
            let before = dataset.fields.len();
            dataset
                .fields
                .retain(|field| field.location == FieldLocation::Point);
            if dataset.fields.len() != before {
                omissions.push(Omission::new(
                    Stage::Destination,
                    format!(
                        "{} cell field(s) have no direct FRD nodal representation",
                        before - dataset.fields.len()
                    ),
                ));
            }
            for field in &mut dataset.fields {
                if field.name.starts_with("DISPLACEMENT_SUBCASE_") {
                    omissions.push(Omission::new(
                        Stage::Destination,
                        format!(
                            "field {} is named DISP in FRD; subcase name is not retained",
                            field.name
                        ),
                    ));
                    field.name = "DISP".into();
                }
                if field.step.is_none() || field.time.is_none() {
                    omissions.push(Omission::new(
                        Stage::Destination,
                        format!(
                            "field {} uses FRD step/time 0 where metadata is absent",
                            field.name
                        ),
                    ));
                }
            }
            if dataset
                .mesh
                .cells
                .iter()
                .any(|cell| cell.property_id.is_some())
            {
                omissions.push(Omission::new(
                    Stage::Destination,
                    "BDF property IDs have no direct FRD mesh mapping",
                ));
            }
            omissions.push(Omission::new(
                Stage::Destination,
                "FRD ASCII E12.5 rounds coordinates and field values to six significant digits",
            ));
            frd::write(dataset, &mut writer)?;
        }
        Format::Op2 => {
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
            if field.components.len() == 3 && options.zero_missing_rotations {
                omissions.push(Omission::new(Stage::Assumption, "rotational displacement components R1/R2/R3 filled with typed float 0.0 by explicit request"));
            }
            if field.step.is_some_and(|step| step != 0) {
                omissions.push(Omission::new(
                    Stage::Destination,
                    "source step number is not encoded in the one-step OP2 table",
                ));
            }
            if dataset.fields.len() > 1 {
                omissions.push(Omission::new(
                    Stage::Destination,
                    format!(
                        "{} other numeric field(s) omitted from OP2 displacement output",
                        dataset.fields.len() - 1
                    ),
                ));
            }
            omissions.push(Omission::new(
                Stage::Destination,
                "OP2 contains no mesh; export and keep a matching BDF separately",
            ));
            omissions.push(Omission::new(
                Stage::Destination,
                "OP2 real displacement values use float32 precision",
            ));
            let bytes = op2::write_displacements(
                dataset,
                field,
                &options.python,
                subcase,
                options.zero_missing_rotations,
                source.assumed_zero,
            )?;
            writer.write_all(&bytes)?;
        }
    }
    Ok(ConversionReport {
        points: dataset.mesh.points.len(),
        cells: dataset.mesh.cells.len(),
        fields: source_fields,
        omissions: source.omissions,
    })
}
