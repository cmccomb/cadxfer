use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::core::{Error, Result};
use crate::formats::msh;

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

    /// Nastran text punch displacement results (read only).
    Pch,

    /// ASCII or binary triangle surface.
    Stl,

    /// Single-zone SU2 ASCII mesh with named boundary markers.
    Su2,

    /// UNV 2411/2412 ASCII geometry datasets.
    Unv,

    /// NetCDF-3 classic Exodus II mesh and complete scalar results.
    Exodus,
}

impl Format {
    /// Every supported format in the order used by capability listings.
    pub const ALL: [Self; 12] = [
        Self::Bdf,
        Self::Vtu,
        Self::Vtk,
        Self::Msh,
        Self::Inp,
        Self::Frd,
        Self::Op2,
        Self::Pch,
        Self::Stl,
        Self::Su2,
        Self::Unv,
        Self::Exodus,
    ];

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
            Self::Pch => "pch",
            Self::Stl => "stl",
            Self::Su2 => "su2",
            Self::Unv => "unv",
            Self::Exodus => "exodus",
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
    /// use caexfer::Format;
    /// assert_eq!(Format::parse("VTU")?, Format::Vtu);
    /// assert_eq!(Format::parse("nas").unwrap_err().code, "E_FORMAT");
    /// # Ok::<(), caexfer::Error>(())
    /// ```
    pub fn parse(name: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|format| format.name().eq_ignore_ascii_case(name))
            .ok_or_else(|| Error::new("E_FORMAT", format!("unknown input format {name}")))
    }

    /// Infer a source format from its case-insensitive extension.
    /// `.nas` and `.dat` are accepted BDF input aliases.
    ///
    /// # Errors
    ///
    /// Returns `E_FORMAT` when the suffix has no supported reader.
    ///
    /// ```
    /// use caexfer::Format;
    /// use std::path::Path;
    /// assert_eq!(Format::from_input_path(Path::new("model.NAS"))?, Format::Bdf);
    /// assert_eq!(Format::from_input_path(Path::new("result.op2"))?, Format::Op2);
    /// # Ok::<(), caexfer::Error>(())
    /// ```
    pub fn from_input_path(path: &Path) -> Result<Self> {
        let extension = extension(path);
        match extension.as_str() {
            "nas" | "dat" => Ok(Self::Bdf),
            "exo" | "e" => Ok(Self::Exodus),
            _ => Self::parse(&extension).map_err(|_| {
                Error::new(
                    "E_FORMAT",
                    format!("no reader for extension {extension:?}; use --from"),
                )
            }),
        }
    }

    /// Infer a writable destination from its extension. Only `.bdf` and `.nas`
    /// are BDF output aliases; `.dat` and `.pch` have no writer.
    ///
    /// # Errors
    ///
    /// Returns `E_FORMAT` when the suffix has no supported writer.
    ///
    /// ```
    /// use caexfer::Format;
    /// use std::path::Path;
    /// assert_eq!(Format::from_output_path(Path::new("mesh.NAS"))?, Format::Bdf);
    /// assert_eq!(Format::from_output_path(Path::new("mesh.dat")).unwrap_err().code,
    ///            "E_FORMAT");
    /// # Ok::<(), caexfer::Error>(())
    /// ```
    pub fn from_output_path(path: &Path) -> Result<Self> {
        let extension = extension(path);
        let format = match extension.as_str() {
            "nas" => Some(Self::Bdf),
            "exo" | "e" => Some(Self::Exodus),
            _ => Self::parse(&extension).ok(),
        };
        format
            .filter(|format| *format != Self::Pch)
            .ok_or_else(|| Error::new("E_FORMAT", format!("no writer for extension {extension:?}")))
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
/// Defaults detect the input format from its extension, bound reads to 256 MiB,
/// and do not silently assert a Nastran result frame
/// or missing rotation values.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)] // Independent conversion assertions and acceptance choices.
pub struct Options {
    /// Override source extension detection.
    pub input_format: Option<Format>,

    /// Companion mesh with original node IDs for OP2/PCH input.
    pub mesh: Option<PathBuf>,

    /// Assert basic-frame coordinates and displacements for a non-BDF result mesh.
    pub assume_basic_frame: bool,

    /// OP2/PCH displacement subcase, if more than one exists.
    pub subcase: Option<i64>,

    /// Zero-based OP2/PCH result step, or FRD step number.
    pub step: Option<usize>,

    /// Maximum source bytes; the same bound applies to a result companion mesh.
    pub max_bytes: usize,

    /// Requested MSH output dialect; `None` uses 4.1.
    pub msh_version: Option<msh::Version>,

    /// Assert that absent OP2 R1/R2/R3 components are known float zero.
    pub zero_missing_rotations: bool,

    /// Accept reported source and destination omissions when writing a file.
    pub accept_omissions: bool,

    /// Permit a synthetic all-zero OP2 table from a result-free BDF or INP.
    pub accept_synthetic_zero: bool,

    /// Optional geometry companion to write alongside an OP2 result.
    pub mesh_output: Option<PathBuf>,

    /// Fail validation when the projection reports an omission or assumption.
    pub strict: bool,

    /// Accept every reported omission and explicit assumption for file output.
    pub accept_all: bool,
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
            max_bytes: 256 * 1024 * 1024,
            msh_version: None,
            zero_missing_rotations: false,
            accept_omissions: false,
            accept_synthetic_zero: false,
            mesh_output: None,
            strict: false,
            accept_all: false,
        }
    }
}
