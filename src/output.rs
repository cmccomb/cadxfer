use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use caexfer::core::{Error, Result};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn ensure_new(path: &Path) -> Result<()> {
    if fs::symlink_metadata(path).is_ok() {
        return Err(Error::new(
            "E_EXISTS",
            format!("{} already exists; use a new output path", path.display()),
        ));
    }
    Ok(())
}

struct Staged {
    path: PathBuf,
    temporary: Temporary,
}

impl Staged {
    fn new(path: &Path, write: impl FnOnce(&mut BufWriter<File>) -> Result<()>) -> Result<Self> {
        let name = path
            .file_name()
            .ok_or_else(|| Error::new("E_OUTPUT", "output must be a file path"))?;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        ensure_new(path)?;
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
                    staged = Some((Temporary(temp), file));
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
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
        write(&mut writer)?;
        writer.flush()?;
        writer.get_ref().sync_all()?;
        drop(writer);
        Ok(Self {
            path: path.to_path_buf(),
            temporary,
        })
    }

    fn install(&self) -> Result<()> {
        fs::hard_link(&self.temporary.0, &self.path).map_err(|error| {
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
}

/// Stage, flush and sync output before installing a new name with hard_link.
/// Never overwrite an existing file, even if another process creates it during
/// the write. Requires hard-link support in the destination filesystem.
pub fn create_new(
    path: &Path,
    write: impl FnOnce(&mut BufWriter<File>) -> Result<()>,
) -> Result<()> {
    Staged::new(path, write)?.install()
}

/// Stage two outputs before installing either. Both paths must be new. If the
/// second install fails after the first succeeds, the error names the first
/// output that remains; a two-file commit cannot be atomic.
pub fn create_pair(
    first_path: &Path,
    first_write: impl FnOnce(&mut BufWriter<File>) -> Result<()>,
    second_path: &Path,
    second_write: impl FnOnce(&mut BufWriter<File>) -> Result<()>,
) -> Result<()> {
    if first_path == second_path {
        return Err(Error::new("E_USAGE", "paired outputs need distinct paths"));
    }
    ensure_new(first_path)?;
    ensure_new(second_path)?;
    let first = Staged::new(first_path, first_write)?;
    let second = Staged::new(second_path, second_write)?;
    first.install()?;
    second.install().map_err(|error| {
        Error::new(
            error.code,
            format!(
                "{} was created, but {} could not be installed: {}",
                first_path.display(),
                second_path.display(),
                error.message
            ),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn directory() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "caexfer-output-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn refuses_overwrite() {
        let dir = directory();
        let dest = dir.join("output");
        fs::write(&dest, "original").unwrap();
        assert_eq!(
            create_new(&dest, |writer| {
                writer.write_all(b"new")?;
                Ok(())
            })
            .unwrap_err()
            .code,
            "E_EXISTS"
        );
        assert_eq!(fs::read_to_string(&dest).unwrap(), "original");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_write_leaves_no_output_or_temp() {
        let dir = directory();
        let dest = dir.join("output");
        assert!(create_new(&dest, |writer| {
            writer.write_all(b"partial")?;
            Err(Error::new("E_TEST", "deliberate failure"))
        })
        .is_err());
        assert!(!dest.exists());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn success_commits_complete_file() {
        let dir = directory();
        let dest = dir.join("output");
        create_new(&dest, |writer| {
            writer.write_all(b"complete")?;
            Ok(())
        })
        .unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"complete");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn pair_stages_both_before_committing_either() {
        let dir = directory();
        let first = dir.join("results.op2");
        let second = dir.join("mesh.bdf");
        assert!(create_pair(
            &first,
            |writer| {
                writer.write_all(b"results")?;
                Ok(())
            },
            &second,
            |_writer| Err(Error::new("E_TEST", "mesh write failed")),
        )
        .is_err());
        assert!(!first.exists() && !second.exists());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
        fs::remove_dir_all(dir).unwrap();
    }
}
