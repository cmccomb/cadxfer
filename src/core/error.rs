use std::fmt;

/// An actionable failure with a stable machine-readable code.
///
/// Match on [`Self::code`] rather than the human-readable message. Source
/// locations, when known, use one-based physical line numbers.
///
/// # Examples
///
/// ```
/// use caexfer::Error;
/// let error = Error::new("E_SAMPLE", "bad field").at(4);
/// assert_eq!(error.code, "E_SAMPLE");
/// assert_eq!(error.line, Some(4));
/// assert_eq!(error.to_string(), "E_SAMPLE at line 4: bad field");
/// ```

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    /// Stable diagnostic identifier; callers should branch on this, not `message`.
    pub code: &'static str,

    /// Human-readable explanation; wording may change between releases.
    pub message: String,

    /// One-based physical source line, when available.
    pub line: Option<usize>,
}

impl Error {
    /// Build an error without a source location.
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            line: None,
        }
    }

    /// Attach a one-based physical source line.
    #[must_use]
    pub fn at(mut self, line: usize) -> Self {
        self.line = Some(line);
        self
    }
}

impl fmt::Display for Error {
    /// Render the stable code, optional physical line, and human message.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.code)?;
        if let Some(line) = self.line {
            write!(f, " at line {line}")?;
        }
        write!(f, ": {}", self.message)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    /// Retain an I/O failure's explanation under caexfer's `E_IO` code.
    fn from(value: std::io::Error) -> Self {
        Self::new("E_IO", value.to_string())
    }
}

/// Result returned by caexfer operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Severity of a scoped validation finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Information was omitted or could not be validated in this scope.
    Warning,

    /// The requested scoped operation cannot proceed.
    Error,
}

/// One finding from scoped validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Whether the finding blocks the scoped operation.
    pub severity: Severity,

    /// Stable diagnostic identifier.
    pub code: &'static str,

    /// Human-readable explanation.
    pub message: String,

    /// One-based physical source line, when available.
    pub line: Option<usize>,
}

impl From<Error> for Diagnostic {
    /// Convert a blocking error into one scoped validation finding.
    fn from(error: Error) -> Self {
        Self {
            severity: Severity::Error,
            code: error.code,
            message: error.message,
            line: error.line,
        }
    }
}

/// Findings from validation of a documented subset, not solver correctness.
/// Warnings leave a report valid *within its stated scope*; errors do not.
/// The default report has no findings.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ValidationReport {
    /// Findings in source order.
    pub diagnostics: Vec<Diagnostic>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Include one-based source lines in formatted diagnostics.
    #[test]
    fn error_display_carries_line() {
        assert_eq!(
            Error::new("E_TEST", "bad field").at(3).to_string(),
            "E_TEST at line 3: bad field"
        );
    }
}
