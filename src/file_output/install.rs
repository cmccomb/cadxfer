//! Stage one or two outputs and install completed files at new names.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use crate::core::{Error, Result};

use super::stage::{Staged, ensure_new};

/// Stage, flush and sync output before installing a new name with `hard_link`.
/// Never overwrite an existing file, even if another process creates it during
/// the write. Requires hard-link support in the destination filesystem.
///
/// # Errors
///
/// Returns an error if staging, writing, syncing, or installing the new file fails.
pub(crate) fn create_new(
    path: &Path,
    write: impl FnOnce(&mut BufWriter<File>) -> Result<()>,
) -> Result<()> {
    // Staged::new retains cleanup ownership until the install call returns.
    Staged::new(path, write)?.install()
}

/// Stage two outputs before installing either. Both paths must be new. If the
/// second install fails after the first succeeds, retain the staged second
/// file and name it in the error so the caller can recover the pair.
pub(crate) fn create_pair(
    first_path: &Path,
    first_write: impl FnOnce(&mut BufWriter<File>) -> Result<()>,
    second_path: &Path,
    second_write: impl FnOnce(&mut BufWriter<File>) -> Result<()>,
) -> Result<()> {
    // Both writes finish before either final path becomes visible.
    if first_path == second_path {
        return Err(Error::new("E_USAGE", "paired outputs need distinct paths"));
    }
    ensure_new(first_path)?;
    ensure_new(second_path)?;
    let first = Staged::new(first_path, first_write)?;
    let mut second = Staged::new(second_path, second_write)?;

    // The two installations cannot be atomic together; report partial
    // installation accurately if the second hard link fails.
    first.install()?;
    second.install().map_err(|error| {
        let recovery = second.retain_for_recovery().map_or_else(
            || "the staged second output is unavailable".to_owned(),
            |path| format!("completed second output retained at {}", path.display()),
        );
        Error::new(
            error.code,
            format!(
                "{} was created, but {} could not be installed: {}; {recovery}",
                first_path.display(),
                second_path.display(),
                error.message
            ),
        )
    })
}
