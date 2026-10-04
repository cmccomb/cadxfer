//! File-installation behavior and cleanup checks.

use super::*;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::Error;

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Give staged-output checks a distinct temporary destination directory.
fn directory() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "caexfer-output-test-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&path).unwrap();
    path
}

/// Refuse to replace a destination file that already exists.
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

/// A dangling link is occupied; a broken parent reports I/O rather than absence.
#[cfg(unix)]
#[test]
fn destination_metadata_errors_are_distinguished() {
    use std::os::unix::fs::symlink;

    let dir = directory();
    let link = dir.join("dangling");
    symlink("missing", &link).unwrap();
    assert_eq!(create_new(&link, |_| Ok(())).unwrap_err().code, "E_EXISTS");

    let parent = dir.join("regular-file");
    fs::write(&parent, b"keep").unwrap();
    let child = parent.join("child");
    assert_eq!(create_new(&child, |_| Ok(())).unwrap_err().code, "E_IO");
    assert_eq!(fs::read(&parent).unwrap(), b"keep");
    fs::remove_dir_all(dir).unwrap();
}

/// Clean staged output after a writer fails before installation.
#[test]
fn failed_write_leaves_no_output_or_temp() {
    let dir = directory();
    let dest = dir.join("output");
    assert!(
        create_new(&dest, |writer| {
            writer.write_all(b"partial")?;
            Err(Error::new("E_TEST", "deliberate failure"))
        })
        .is_err()
    );
    assert!(!dest.exists());
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
    fs::remove_dir_all(dir).unwrap();
}

/// Install a fully written staged file at the requested path.
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

/// Stage both companion outputs before either final path is installed.
#[test]
fn pair_stages_both_before_committing_either() {
    let dir = directory();
    let first = dir.join("results.op2");
    let second = dir.join("mesh.bdf");
    assert!(
        create_pair(
            &first,
            |writer| {
                writer.write_all(b"results")?;
                Ok(())
            },
            &second,
            |_writer| Err(Error::new("E_TEST", "mesh write failed")),
        )
        .is_err()
    );
    assert!(!first.exists() && !second.exists());
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
    fs::remove_dir_all(dir).unwrap();
}

/// Reject a paired output that names the same destination twice.
#[test]
fn pair_requires_distinct_destinations() {
    let dir = directory();
    let dest = dir.join("same.bdf");
    assert_eq!(
        create_pair(&dest, |_| Ok(()), &dest, |_| Ok(()))
            .unwrap_err()
            .code,
        "E_USAGE"
    );
    assert!(!dest.exists());
    fs::remove_dir_all(dir).unwrap();
}

/// Report the installed first file if another writer claims the second name.
#[test]
fn pair_reports_a_second_destination_race() {
    let dir = directory();
    let first = dir.join("results.op2");
    let second = dir.join("mesh.bdf");
    let error = create_pair(
        &first,
        |writer| {
            writer.write_all(b"results")?;
            Ok(())
        },
        &second,
        |writer| {
            writer.write_all(b"staged mesh")?;
            fs::write(&second, b"another writer")?;
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "E_EXISTS");
    assert!(error.message.contains(&first.display().to_string()));
    assert_eq!(fs::read(&first).unwrap(), b"results");
    assert_eq!(fs::read(&second).unwrap(), b"another writer");
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);
    fs::remove_dir_all(dir).unwrap();
}

/// An absent destination parent fails before a partial file is created.
#[test]
fn missing_parent_reports_io() {
    let dir = directory();
    let dest = dir.join("missing").join("output");
    assert_eq!(create_new(&dest, |_| Ok(())).unwrap_err().code, "E_IO");
    assert!(!dest.exists());
    fs::remove_dir_all(dir).unwrap();
}

/// A removed staging directory cannot produce a partial final output.
#[cfg(unix)]
#[test]
fn removed_staging_directory_reports_install_failure() {
    let dir = directory();
    let dest = dir.join("output");
    let error = create_new(&dest, |writer| {
        writer.write_all(b"staged")?;
        fs::remove_dir_all(&dir)?;
        Ok(())
    })
    .unwrap_err();
    assert_eq!(error.code, "E_OUTPUT");
    assert!(!dest.exists());
}
