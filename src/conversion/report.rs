use crate::core::Dataset;

use super::Format;

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
    pub(super) fn new(stage: Stage, detail: impl Into<String>) -> Self {
        Self {
            stage,
            detail: detail.into(),
        }
    }
}

/// Supported source data and its read-side omissions.
/// The `dataset` is a projection; `omissions` explains what was not carried
/// from the source format. Retain the original file when those details matter.
#[derive(Debug, Clone, PartialEq)]
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
/// The default is an empty report with zero counts.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
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
