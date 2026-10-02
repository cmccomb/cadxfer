use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use caxifer_core::{Error, Result};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Stage, flush and sync output before installing a new name with hard_link.
/// Never overwrite an existing file, even if another process creates it during
/// the write. No fallback to a truncating create or non-atomic copy.
/// Requires hard-link support in the destination filesystem.
pub fn create_new(
    path: &Path,
    write: impl FnOnce(&mut BufWriter<File>) -> Result<()>,
) -> Result<()> {
    let name = path
        .file_name()
        .ok_or_else(|| Error::new("E_OUTPUT", "output must be a file path"))?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if fs::symlink_metadata(path).is_ok() {
        return Err(Error::new(
            "E_EXISTS",
            format!("{} already exists; use a new output path", path.display()),
        ));
    }
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut staged = None;
    for _ in 0..32 {
        let mut temp_name = std::ffi::OsString::from(".");
        temp_name.push(name);
        temp_name.push(format!(
            ".caxifer-{}-{timestamp}-{}.tmp",
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
    fs::hard_link(&temporary.0, path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            Error::new("E_EXISTS", format!("{} appeared during the write; nothing overwritten", path.display()))
        } else {
            Error::new("E_OUTPUT", format!("could not install staged output (destination filesystem must support hard links): {error}"))
        }
    })?;
    // Temporary's Drop removes the staging link; the committed file remains.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn directory() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "caxifer-output-test-{}-{}",
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
}
