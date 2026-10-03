//! Format-aware conversion with explicit source and destination omissions.
//!
//! [`convert_path`] reads a supported source and writes its supported projection
//! to a caller-owned stream. The returned [`ConversionReport`] describes what
//! was carried and what was omitted. File persistence and overwrite policy
//! belong to the caller. Use [`read_path`] followed by [`convert`] when the
//! projected dataset needs inspection or modification between those steps.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::bdf::{self, Document, ParseOptions};
use crate::core::{Dataset, Error, FieldLocation, Mesh, Result};
use crate::{frd, inp, msh, op2, vtk, vtu};

/// Supported conversion format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Nastran Bulk Data deck.
    Bdf,
    /// VTK XML `UnstructuredGrid`.
    Vtu,
    /// ASCII legacy VTK unstructured grid.
    Vtk,
    /// Gmsh MSH 4.1.
    Msh,
    /// Abaqus or `CalculiX` input deck.
    Inp,
    /// `CalculiX` result file.
    Frd,
    /// Nastran displacement result file.
    Op2,
}

impl Format {
    /// Canonical lowercase format name.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Bdf => "bdf",
            Self::Vtu => "vtu",
            Self::Vtk => "vtk",
            Self::Msh => "msh",
            Self::Inp => "inp",
            Self::Frd => "frd",
            Self::Op2 => "op2",
        }
    }

    /// Parse a canonical format name, as used by `--from`.
    /// Names are case-insensitive; filename aliases such as `.nas` belong to
    /// [`Self::from_input_path`] instead.
    ///
    /// # Errors
    ///
    /// Returns `E_FORMAT` for an unknown canonical name.
    ///
    /// ```
    /// use caexfer::conversion::Format;
    /// assert_eq!(Format::parse("VTU")?, Format::Vtu);
    /// assert_eq!(Format::parse("nas").unwrap_err().code, "E_FORMAT");
    /// # Ok::<(), caexfer::core::Error>(())
    /// ```
    pub fn parse(name: &str) -> Result<Self> {
        match name.to_ascii_lowercase().as_str() {
            "bdf" => Ok(Self::Bdf),
            "vtu" => Ok(Self::Vtu),
            "vtk" => Ok(Self::Vtk),
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

    /// Infer a source format from its case-insensitive extension.
    /// `.nas`, `.dat`, and `.pch` are accepted BDF input aliases.
    ///
    /// # Errors
    ///
    /// Returns `E_FORMAT` when the suffix has no supported reader.
    ///
    /// ```
    /// use caexfer::conversion::Format;
    /// use std::path::Path;
    /// assert_eq!(Format::from_input_path(Path::new("model.NAS"))?, Format::Bdf);
    /// assert_eq!(Format::from_input_path(Path::new("result.op2"))?, Format::Op2);
    /// # Ok::<(), caexfer::core::Error>(())
    /// ```
    pub fn from_input_path(path: &Path) -> Result<Self> {
        let extension = extension(path);
        match extension.as_str() {
            "bdf" | "nas" | "dat" | "pch" => Ok(Self::Bdf),
            "vtu" | "vtk" | "msh" | "inp" | "frd" | "op2" => Self::parse(&extension),
            _ => Err(Error::new(
                "E_FORMAT",
                format!("no reader for extension {extension:?}; use --from"),
            )),
        }
    }

    /// Infer a writable destination from its extension. Only `.bdf` and `.nas`
    /// are BDF output aliases; `.dat` and `.pch` are input-only.
    ///
    /// # Errors
    ///
    /// Returns `E_FORMAT` when the suffix has no supported writer.
    ///
    /// ```
    /// use caexfer::conversion::Format;
    /// use std::path::Path;
    /// assert_eq!(Format::from_output_path(Path::new("mesh.NAS"))?, Format::Bdf);
    /// assert_eq!(Format::from_output_path(Path::new("mesh.dat")).unwrap_err().code,
    ///            "E_FORMAT");
    /// # Ok::<(), caexfer::core::Error>(())
    /// ```
    pub fn from_output_path(path: &Path) -> Result<Self> {
        let extension = extension(path);
        match extension.as_str() {
            "bdf" | "nas" => Ok(Self::Bdf),
            "vtu" | "vtk" | "msh" | "inp" | "frd" | "op2" => Self::parse(&extension),
            _ => Err(Error::new(
                "E_FORMAT",
                format!("no writer for extension {extension:?}"),
            )),
        }
    }
}

/// Normalize a path suffix for format dispatch without guessing from content.
fn extension(path: &Path) -> String {
    path.extension()
        .and_then(OsStr::to_str)
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// Source selection, limits, and explicit result assumptions.
/// Defaults detect the input format from its extension, bound reads to the BDF
/// parser's default byte limit, and do not silently assert an OP2 result frame
/// or missing rotation values.
#[derive(Debug, Clone)]
pub struct Options {
    /// Override source extension detection.
    pub input_format: Option<Format>,
    /// Matching BDF, VTU, VTK, MSH, INP, or FRD mesh required when reading OP2.
    pub mesh: Option<PathBuf>,
    /// Assert basic-frame coordinates and displacements for a non-BDF OP2 mesh.
    pub assume_basic_frame: bool,
    /// OP2 displacement subcase, if more than one exists.
    pub subcase: Option<i64>,
    /// Zero-based OP2 result step, or FRD step number.
    pub step: Option<usize>,
    /// Maximum source bytes; the same bound applies to an OP2 companion mesh.
    pub max_bytes: usize,
    /// Assert that absent OP2 R1/R2/R3 components are known float zero.
    pub zero_missing_rotations: bool,
}

impl Default for Options {
    /// Start with conservative read limits and no asserted OP2 assumptions.
    fn default() -> Self {
        Self {
            input_format: None,
            mesh: None,
            assume_basic_frame: false,
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
    #[must_use]
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
    /// Retain the origin of one omission for machine-readable reports.
    fn new(stage: Stage, detail: impl Into<String>) -> Self {
        Self {
            stage,
            detail: detail.into(),
        }
    }
}

/// Supported source data and its read-side omissions.
/// The `dataset` is a projection; `omissions` explains what was not carried
/// from the source format. Retain the original file when those details matter.
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
/// `fields` counts fields before destination filtering, while `omissions`
/// records source losses, destination losses, and explicit assumptions.
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

/// Read BDF through its source-preserving parser under the requested byte cap.
fn read_bdf(path: &Path, max_bytes: usize) -> Result<Document> {
    Document::read_with_options(
        File::open(path)?,
        ParseOptions {
            max_bytes,
            ..ParseOptions::default()
        },
    )
}

/// Bound non-BDF file reads with both a metadata check and an actual read cap.
/// The second check handles files growing between the metadata and read calls.
fn read_limited(path: &Path, max_bytes: usize) -> Result<Vec<u8>> {
    // Metadata catches ordinary oversize files without allocating for them.
    if std::fs::metadata(path)?.len() > max_bytes as u64 {
        return Err(Error::new(
            "E_LIMIT",
            format!("input exceeds {max_bytes} bytes"),
        ));
    }

    // The extra byte detects a file that grew after the metadata check.
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

/// Load an OP2 companion mesh and report any source-side projection losses.
/// BDF grid output frames are checked; other formats require the caller's
/// explicit basic-frame assertion because that metadata is unavailable.
fn read_op2_mesh(path: &Path, options: &Options) -> Result<(Mesh, Vec<Omission>)> {
    let format = Format::from_input_path(path)?;
    match format {
        Format::Bdf => {
            // BDF exposes GRID output frames, so verify them directly.
            let document = read_bdf(path, options.max_bytes)?;
            for grid in document.grids() {
                if grid?.cd != 0 {
                    return Err(Error::new(
                        "E_OP2",
                        "nonbasic GRID CD requires displacement frame transformation",
                    ));
                }
            }
            let projection = document.geometry()?;

            // Keep source losses from the companion deck visible in the
            // result conversion report.
            let omissions = projection
                .omissions
                .into_iter()
                .map(|item| {
                    Omission::new(
                        Stage::Source,
                        format!("BDF {} × {}: {}", item.category, item.count, item.detail),
                    )
                })
                .collect();
            Ok((projection.mesh, omissions))
        }
        Format::Op2 => Err(Error::new(
            "E_USAGE",
            "OP2 companion mesh must be a mesh-bearing format",
        )),
        _ => {
            // Other mesh formats carry no GRID CD, requiring an explicit
            // caller assertion before pairing them with OP2 displacements.
            if !options.assume_basic_frame {
                return Err(Error::new(
                    "E_USAGE",
                    "non-BDF OP2 mesh lacks GRID CD; pass --assume-basic-frame to assert basic-frame coordinates and displacements",
                ));
            }

            // A companion supplies geometry only; OP2 result-selection options
            // must not be reapplied while reading its mesh-bearing file.
            let mesh_options = Options {
                input_format: None,
                mesh: None,
                assume_basic_frame: false,
                subcase: None,
                step: None,
                ..options.clone()
            };
            let read = read_path(path, &mesh_options)?;

            // A companion contributes geometry only; report ignored fields.
            let mut omissions = read
                .omissions
                .into_iter()
                .map(|item| {
                    Omission::new(
                        item.stage,
                        format!("companion {}: {}", format.name(), item.detail),
                    )
                })
                .collect::<Vec<_>>();
            if !read.dataset.fields.is_empty() {
                omissions.push(Omission::new(
                    Stage::Source,
                    format!(
                        "{} numeric field(s) in companion mesh ignored",
                        read.dataset.fields.len()
                    ),
                ));
            }
            omissions.push(Omission::new(
                Stage::Assumption,
                format!("companion {} has no GRID CD; basic-frame coordinates and OP2 displacements asserted by caller", format.name()),
            ));
            Ok((read.dataset.mesh, omissions))
        }
    }
}

/// Read a supported source file into a mesh and fields, reporting omitted data.
/// OP2 input requires `options.mesh`. A non-BDF
/// companion additionally requires `options.assume_basic_frame`.
///
/// # Errors
///
/// Returns an option or format error, an I/O or size-limit error, or a source
/// reader error when the requested projection cannot be represented safely.
///
/// # Examples
///
/// ```
/// use caexfer::conversion::{read_path, Format, Options};
/// use std::path::Path;
/// let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/plate.bdf");
/// // The read result carries both the projected mesh and source omissions.
/// let source = read_path(&path, &Options::default())?;
/// assert_eq!(source.format, Format::Bdf);
/// assert_eq!(source.dataset.mesh.points.len(), 4);
/// assert!(!source.omissions.is_empty());
/// # Ok::<(), caexfer::core::Error>(())
/// ```
#[allow(clippy::too_many_lines)] // Format-specific read branches share projection reporting.
pub fn read_path(path: &Path, options: &Options) -> Result<ReadResult> {
    // An explicit source format overrides suffix-based selection.
    let format = options
        .input_format
        .map_or_else(|| Format::from_input_path(path), Ok)?;

    // Reject result-selection and frame options on formats that cannot use
    // them before opening or parsing any source file.
    if format != Format::Op2 && (options.mesh.is_some() || options.subcase.is_some()) {
        return Err(Error::new(
            "E_USAGE",
            "mesh and subcase apply only to OP2 input",
        ));
    }
    if format != Format::Op2 && options.assume_basic_frame {
        return Err(Error::new(
            "E_USAGE",
            "basic-frame assertion applies only to OP2 input",
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
            // BDF projection can lose solver cards while retaining geometry.
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
        Format::Vtk => {
            let bytes = read_limited(path, options.max_bytes)?;
            let text = std::str::from_utf8(&bytes)
                .map_err(|_| Error::new("E_VTK", "legacy VTK must be ASCII text"))?;
            let projection = vtk::read_projection(text)?;
            let mut omissions = Vec::new();
            if projection.generated_point_ids {
                omissions.push(Omission::new(
                    Stage::Source,
                    "legacy VTK has no original point IDs; assigned one-based IDs",
                ));
            }
            if projection.generated_cell_ids {
                omissions.push(Omission::new(
                    Stage::Source,
                    "legacy VTK has no original cell IDs; assigned one-based IDs",
                ));
            }
            (projection.dataset, omissions, false)
        }
        Format::Msh => {
            // The MSH reader handles geometry and data; other sections are
            // recorded as source omissions for the caller to review.
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
            // Step selection filters complete fields after parsing the file.
            let bytes = read_limited(path, options.max_bytes)?;
            let mut dataset = frd::read(&bytes)?;
            if let Some(step) = options.step {
                let step = i64::try_from(step)
                    .map_err(|_| Error::new("E_FRD", "selected step exceeds Int64"))?;
                dataset.fields.retain(|field| field.step == Some(step));
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
            // OP2 result tables carry no complete matching mesh on their own.
            let mesh_path = options
                .mesh
                .as_deref()
                .ok_or_else(|| Error::new("E_USAGE", "OP2 requires --mesh matching mesh file"))?;
            let (mesh, mut omissions) = read_op2_mesh(mesh_path, options)?;
            let bytes = read_limited(path, options.max_bytes)?;
            let (dataset, assumed_zero) =
                op2::read_displacements(&bytes, &mesh, options.subcase, options.step)?;

            // Preserve both explicit synthetic provenance and the results
            // omitted by the selected displacement-table projection.
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
/// persist the stream. The report identifies information lost or assumed. A
/// writer I/O error may leave partial bytes in the caller-owned stream.
///
/// # Errors
///
/// Returns a source read or destination conversion error, including writer
/// failures after partial bytes have reached the stream.
///
/// # Examples
///
/// ```
/// use caexfer::conversion::{convert_path, Format, Options};
/// use std::path::Path;
/// let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/plate.bdf");
/// let mut vtu = Vec::new();
/// // The report includes losses from both reading and writing.
/// let report = convert_path(&path, Format::Vtu, &Options::default(), &mut vtu)?;
/// assert_eq!(report.points, 4);
/// assert!(std::str::from_utf8(&vtu).unwrap().contains("<VTKFile"));
/// # Ok::<(), caexfer::core::Error>(())
/// ```
pub fn convert_path(
    path: &Path,
    target: Format,
    options: &Options,
    writer: impl Write,
) -> Result<ConversionReport> {
    // Keep read-side omissions attached to the source through destination
    // projection, so one report covers both stages.
    convert(read_path(path, options)?, target, options, writer)
}

/// Write a previously read source in another supported format.
///
/// OP2 output requires exactly one nodal displacement field. A caller may
/// supply a synthetic field, but must set
/// `source.assumed_zero` to mark that provenance in the OP2 title.
///
/// # Errors
///
/// Returns a validation, representability, adapter, or output-stream error
/// for the selected destination format.
///
/// # Examples
///
/// ```
/// use caexfer::bdf::Document;
/// use caexfer::conversion::{convert, Format, Options, ReadResult, Stage};
/// use caexfer::core::Dataset;
/// let document = Document::parse(
///     "GRID,10,,0,0,0\nGRID,20,,1,0,0\nCROD,30,7,10,20\n"
/// )?;
/// let source = ReadResult {
///     format: Format::Bdf,
///     dataset: Dataset { mesh: document.geometry()?.mesh, fields: vec![] },
///     omissions: vec![], assumed_zero: false,
/// };
/// let mut inp = Vec::new();
/// // INP carries this geometry but not generic numeric result fields.
/// let report = convert(source, Format::Inp, &Options::default(), &mut inp)?;
/// assert_eq!(report.cells, 1);
/// assert!(report.omissions.iter().any(|item| item.stage == Stage::Destination));
/// assert!(std::str::from_utf8(&inp).unwrap().contains("*ELEMENT"));
/// # Ok::<(), caexfer::core::Error>(())
/// ```
#[allow(clippy::too_many_lines)] // Destination branches share omission and assumption tracking.
pub fn convert(
    mut source: ReadResult,
    target: Format,
    options: &Options,
    mut writer: impl Write,
) -> Result<ConversionReport> {
    // Count fields before destination-specific filtering; the report describes
    // the source dataset as well as any losses during output.
    let source_fields = source.dataset.fields.len();
    let dataset = &mut source.dataset;
    let omissions = &mut source.omissions;
    match target {
        Format::Vtu => {
            // VTU has one array per location and name; multiple source steps
            // require the caller to select a step first.
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
        Format::Vtk => {
            for field in &dataset.fields {
                if field.step.is_some() || field.time.is_some() {
                    omissions.push(Omission::new(
                        Stage::Destination,
                        format!(
                            "field {} loses step/time metadata in legacy VTK",
                            field.name
                        ),
                    ));
                }
                if field
                    .components
                    .iter()
                    .enumerate()
                    .any(|(index, name)| name != &format!("C{}", index + 1))
                {
                    omissions.push(Omission::new(
                        Stage::Destination,
                        format!("field {} loses component labels in legacy VTK", field.name),
                    ));
                }
            }
            vtk::write_data(dataset, &mut writer)?;
        }
        Format::Msh => {
            // MSH stores numeric tuples but loses these component labels and
            // this exporter's unmapped property IDs.
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
            // These writers emit geometry-only solver input, so result fields
            // remain accounted for in the report instead of silently vanishing.
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
                // The bounded INP writer has no property definition mapping.
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
                // BDF requires a PID on these element cards; the writer uses
                // placeholder 1 where the source mesh has none.
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
                bdf::write_geometry(&dataset.mesh, &mut writer)?;
            }
        }
        Format::Frd => {
            // The supported FRD result blocks are nodal, so cell fields must
            // be removed before calling its writer.
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
                // FRD's displacement name and metadata layout differ from
                // the generic field model; record each normalization.
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
            // The adapter writes a single real nodal displacement table.
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
                return Err(Error::new(
                    "E_OP2",
                    "OP2 output requires exactly one 3- or 6-component nodal DISP field; select one result step",
                ));
            }
            let field = matches[0];

            // Recover a subcase encoded in the field name, defaulting to 1
            // for generic DISP names.
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
                "OP2 contains no mesh; retain or export a matching companion mesh",
            ));
            omissions.push(Omission::new(
                Stage::Destination,
                "OP2 real displacement values use float32 precision",
            ));

            // The adapter builds the binary table before it reaches the
            // caller's output stream.
            let bytes = op2::write_displacements(
                dataset,
                field,
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
