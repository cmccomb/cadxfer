//! Private staged files and exclusive destination installation.

use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::core::{Error, Result};

// Distinguish staged filenames created by this process, even within one clock tick.
static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Staged pathname removed on drop unless a partial pair needs recovery.
struct Temporary {
    path: PathBuf,
    retain: bool,
}
impl Drop for Temporary {
    /// Remove the staged file when it falls out of scope, including on error.
    /// A successfully installed hard link remains at the destination path.
    fn drop(&mut self) {
        // Once installed, only the temporary name is removed; the destination
        // hard link keeps the complete file alive.
        if !self.retain {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Reject any existing destination, including a dangling symlink.
/// The later hard-link install repeats the no-clobber guarantee atomically.
pub(super) fn ensure_new(path: &Path) -> Result<()> {
    // symlink_metadata also detects a dangling symlink at the destination.
    match fs::symlink_metadata(path) {
        Ok(_) => Err(Error::new(
            "E_EXISTS",
            format!("{} already exists; use a new output path", path.display()),
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// Complete synced output waiting for an exclusive destination install.
pub(super) struct Staged {
    path: PathBuf,
    temporary: Temporary,
}

impl Staged {
    /// Write, flush, and sync to an exclusive temporary file beside `path`.
    /// Nothing is visible at the destination until `install` succeeds.
    pub(super) fn new(
        path: &Path,
        write: impl FnOnce(&mut BufWriter<File>) -> Result<()>,
    ) -> Result<Self> {
        // Stage beside the destination so a later hard link stays on the
        // same filesystem and can refuse replacement atomically.
        let name = path
            .file_name()
            .ok_or_else(|| Error::new("E_OUTPUT", "output must be a file path"))?;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        ensure_new(path)?;

        // Exclusive creation avoids collisions with another invocation.
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let mut staged = None;
        for _ in 0..32 {
            let mut temp_name = std::ffi::OsString::from(".");
            temp_name.push(name);
            temp_name.push(format!(
                ".caexfer-{}-{timestamp}-{}.tmp",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            let temp = parent.join(temp_name);
            match OpenOptions::new().write(true).create_new(true).open(&temp) {
                Ok(file) => {
                    staged = Some((
                        Temporary {
                            path: temp,
                            retain: false,
                        },
                        file,
                    ));
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
        }
        let (temporary, file) = staged.ok_or_else(|| {
            Error::new(
                "E_OUTPUT",
                "could not allocate an exclusive temporary output",
            )
        })?;
        let mut writer = BufWriter::new(file);

        // Flush buffered bytes and sync the file before making a destination
        // name visible; Temporary cleans up if any step fails.
        write(&mut writer)?;
        writer.flush()?;
        writer.get_ref().sync_all()?;
        drop(writer);
        Ok(Self {
            path: path.to_path_buf(),
            temporary,
        })
    }

    /// Link a complete staged file into a still-new destination name.
    /// Hard links make the no-overwrite step atomic on supported filesystems.
    pub(super) fn install(&self) -> Result<()> {
        // hard_link fails if the destination name appeared during staging.
        fs::hard_link(&self.temporary.path, &self.path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                Error::new(
                    "E_EXISTS",
                    format!("{} appeared during the write; nothing overwritten", self.path.display()),
                )
            } else {
                Error::new("E_OUTPUT", format!("could not install staged output (destination filesystem must support hard links): {error}"))
            }
        })
    }

    /// Keep a completed second output when its installation fails.
    pub(super) fn retain_for_recovery(&mut self) -> Option<&Path> {
        if fs::symlink_metadata(&self.temporary.path).is_ok_and(|metadata| metadata.is_file()) {
            self.temporary.retain = true;
            Some(&self.temporary.path)
        } else {
            None
        }
    }
}
