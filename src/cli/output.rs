//! Hold library conversion output privately until CLI approval.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use caexfer::{Error, Result};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A unique sibling directory whose completed output is not yet installed.
pub(super) struct PendingOutput {
    directory: PathBuf,
    temporary: PathBuf,
    destination: PathBuf,
    source: PathBuf,
    overwrite: bool,
}

impl PendingOutput {
    /// Reserve a private path with the destination's extension for format inference.
    pub(super) fn new(destination: &Path, source: &Path, overwrite: bool) -> Result<Self> {
        let name = destination
            .file_name()
            .ok_or_else(|| Error::new("E_OUTPUT", "output must be a file path"))?;
        let parent = destination
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        check_destination(destination, source, overwrite)?;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        for _ in 0..32 {
            let directory = parent.join(format!(
                ".caexfer-cli-{}-{timestamp}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            match create_private_dir(&directory) {
                Ok(()) => {
                    return Ok(Self {
                        temporary: directory.join(name),
                        directory,
                        destination: destination.to_path_buf(),
                        source: source.to_path_buf(),
                        overwrite,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
        }
        Err(Error::new(
            "E_OUTPUT",
            "could not allocate a private conversion directory",
        ))
    }

    /// Path given to `caexfer::convert` for staged conversion.
    pub(super) fn temporary(&self) -> &Path {
        &self.temporary
    }

    /// Install a complete, approved file using the requested destination policy.
    pub(super) fn install(&self) -> Result<()> {
        if self.overwrite {
            check_destination(&self.destination, &self.source, true)?;
            return fs::rename(&self.temporary, &self.destination).map_err(|error| {
                Error::new(
                    "E_OUTPUT",
                    format!("could not replace {}: {error}", self.destination.display()),
                )
            });
        }
        fs::hard_link(&self.temporary, &self.destination).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                Error::new(
                    "E_EXISTS",
                    format!(
                        "{} appeared during the write; nothing overwritten",
                        self.destination.display()
                    ),
                )
            } else {
                Error::new(
                    "E_OUTPUT",
                    format!("could not install staged output (destination filesystem must support hard links): {error}"),
                )
            }
        })
    }
}

impl Drop for PendingOutput {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

/// Create the temporary directory without exposing its contents to other users.
fn create_private_dir(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        builder.create(path)
    }
    #[cfg(not(unix))]
    {
        fs::create_dir(path)
    }
}

/// Catch existing files and dangling symlinks before conversion begins.
fn ensure_new(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(Error::new(
            "E_EXISTS",
            format!("{} already exists; use a new output path", path.display()),
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// Reject paths that an explicit replacement must never consume.
fn check_destination(destination: &Path, source: &Path, overwrite: bool) -> Result<()> {
    if !overwrite {
        return ensure_new(destination);
    }
    let metadata = match fs::symlink_metadata(destination) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if !metadata.file_type().is_file() {
        return Err(Error::new(
            "E_USAGE",
            format!(
                "--overwrite requires a regular file destination: {}",
                destination.display()
            ),
        ));
    }
    if same_file(source, destination)? {
        return Err(Error::new(
            "E_USAGE",
            format!(
                "input and output refer to the same file: {}",
                destination.display()
            ),
        ));
    }
    Ok(())
}

/// Compare resolved paths and, on Unix, device/inode identity for hard links.
fn same_file(first: &Path, second: &Path) -> Result<bool> {
    if fs::canonicalize(first)? == fs::canonicalize(second)? {
        return Ok(true);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let first = fs::metadata(first)?;
        let second = fs::metadata(second)?;
        Ok(first.dev() == second.dev() && first.ino() == second.ino())
    }
    #[cfg(not(unix))]
    Ok(false)
}

/// Install an approved OP2 and companion, reporting a partial pair precisely.
pub(super) fn install_pair(first: &PendingOutput, second: &PendingOutput) -> Result<()> {
    first.install()?;
    second.install().map_err(|error| {
        Error::new(
            error.code,
            format!(
                "{} was created, but {} could not be installed: {}",
                first.destination.display(),
                second.destination.display(),
                error.message
            ),
        )
    })
}
