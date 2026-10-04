//! Output installation races and failure cleanup.

use super::*;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "caexfer-cli-output-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn no_clobber_install_handles_races_and_missing_staging() {
    let scratch = Scratch::new();
    let source = scratch.path("source.bdf");
    let destination = scratch.path("output.vtu");
    fs::write(&source, b"source").unwrap();

    let pending = PendingOutput::new(&destination, &source, false).unwrap();
    fs::write(pending.temporary(), b"staged").unwrap();
    fs::write(&destination, b"another writer").unwrap();
    assert_eq!(pending.install().unwrap_err().code, "E_EXISTS");
    assert_eq!(fs::read(&destination).unwrap(), b"another writer");
    drop(pending);

    let fresh = scratch.path("fresh.vtu");
    let pending = PendingOutput::new(&fresh, &source, false).unwrap();
    assert_eq!(pending.install().unwrap_err().code, "E_OUTPUT");
    assert!(!fresh.exists());
}

#[test]
fn failed_replacement_preserves_existing_bytes() {
    let scratch = Scratch::new();
    let source = scratch.path("source.bdf");
    let destination = scratch.path("output.vtu");
    fs::write(&source, b"source").unwrap();
    fs::write(&destination, b"existing").unwrap();

    let pending = PendingOutput::new(&destination, &source, true).unwrap();
    fs::write(pending.temporary(), b"staged").unwrap();
    fs::remove_file(pending.temporary()).unwrap();
    assert_eq!(pending.install().unwrap_err().code, "E_OUTPUT");
    assert_eq!(fs::read(&destination).unwrap(), b"existing");
}

#[test]
fn paired_install_checks_both_destinations_before_install() {
    let scratch = Scratch::new();
    let source = scratch.path("source.bdf");
    let first_path = scratch.path("results.op2");
    let second_path = scratch.path("mesh.bdf");
    fs::write(&source, b"source").unwrap();
    let mut first = PendingOutput::new(&first_path, &source, false).unwrap();
    let mut second = PendingOutput::new(&second_path, &source, false).unwrap();
    fs::write(first.temporary(), b"results").unwrap();
    fs::write(second.temporary(), b"mesh").unwrap();
    fs::write(&second_path, b"another writer").unwrap();

    let error = install_pair(&mut first, &mut second).unwrap_err();
    assert_eq!(error.code, "E_EXISTS");
    assert!(!first_path.exists());
    assert_eq!(fs::read(&second_path).unwrap(), b"another writer");
}

#[test]
fn paired_replacement_updates_both_outputs() {
    let scratch = Scratch::new();
    let source = scratch.path("source.bdf");
    let first_path = scratch.path("results.op2");
    let second_path = scratch.path("mesh.bdf");
    fs::write(&source, b"source").unwrap();
    fs::write(&first_path, b"old results").unwrap();
    fs::write(&second_path, b"old mesh").unwrap();
    let mut first = PendingOutput::new(&first_path, &source, true).unwrap();
    let mut second = PendingOutput::new(&second_path, &source, true).unwrap();
    fs::write(first.temporary(), b"new results").unwrap();
    fs::write(second.temporary(), b"new mesh").unwrap();

    install_pair(&mut first, &mut second).unwrap();
    assert_eq!(fs::read(&first_path).unwrap(), b"new results");
    assert_eq!(fs::read(&second_path).unwrap(), b"new mesh");
}

#[test]
fn paired_replacement_rechecks_both_before_changing_either() {
    let scratch = Scratch::new();
    let source = scratch.path("source.bdf");
    let first_path = scratch.path("results.op2");
    let second_path = scratch.path("mesh.bdf");
    fs::write(&source, b"source").unwrap();
    fs::write(&first_path, b"old results").unwrap();
    fs::write(&second_path, b"old mesh").unwrap();
    let mut first = PendingOutput::new(&first_path, &source, true).unwrap();
    let mut second = PendingOutput::new(&second_path, &source, true).unwrap();
    fs::write(first.temporary(), b"new results").unwrap();
    fs::write(second.temporary(), b"new mesh").unwrap();
    fs::remove_file(&second_path).unwrap();
    fs::create_dir(&second_path).unwrap();

    assert_eq!(
        install_pair(&mut first, &mut second).unwrap_err().code,
        "E_USAGE"
    );
    assert_eq!(fs::read(&first_path).unwrap(), b"old results");
    assert!(second_path.is_dir());
}

#[test]
fn paired_overwrite_retains_previous_files_and_staged_companion_after_race() {
    let scratch = Scratch::new();
    let source = scratch.path("source.bdf");
    let first_path = scratch.path("results.op2");
    let second_path = scratch.path("mesh.bdf");
    fs::write(&source, b"source").unwrap();
    fs::write(&first_path, b"old results").unwrap();
    fs::write(&second_path, b"old mesh").unwrap();
    let mut first = PendingOutput::new(&first_path, &source, true).unwrap();
    let mut second = PendingOutput::new(&second_path, &source, true).unwrap();
    fs::write(first.temporary(), b"new results").unwrap();
    fs::write(second.temporary(), b"new mesh").unwrap();
    let first_backup = first.previous();
    let second_backup = second.previous();
    let staged_second = second.temporary().to_path_buf();

    let error = install_pair_with(&mut first, &mut second, || {
        fs::remove_file(&second_path).unwrap();
        fs::create_dir(&second_path).unwrap();
    })
    .unwrap_err();
    assert_eq!(error.code, "E_USAGE");
    assert_eq!(fs::read(&first_path).unwrap(), b"new results");
    assert!(second_path.is_dir());
    drop(first);
    drop(second);
    assert_eq!(fs::read(&first_backup).unwrap(), b"old results");
    assert_eq!(fs::read(&second_backup).unwrap(), b"old mesh");
    assert_eq!(fs::read(&staged_second).unwrap(), b"new mesh");
    for path in [&first_backup, &second_backup, &staged_second] {
        assert!(error.message.contains(&path.display().to_string()));
    }
}

#[test]
fn paired_no_clobber_retains_staged_companion_after_race() {
    let scratch = Scratch::new();
    let source = scratch.path("source.bdf");
    let first_path = scratch.path("results.op2");
    let second_path = scratch.path("mesh.bdf");
    fs::write(&source, b"source").unwrap();
    let mut first = PendingOutput::new(&first_path, &source, false).unwrap();
    let mut second = PendingOutput::new(&second_path, &source, false).unwrap();
    fs::write(first.temporary(), b"new results").unwrap();
    fs::write(second.temporary(), b"new mesh").unwrap();
    let staged_second = second.temporary().to_path_buf();

    let error = install_pair_with(&mut first, &mut second, || {
        fs::write(&second_path, b"another writer").unwrap();
    })
    .unwrap_err();
    assert_eq!(error.code, "E_EXISTS");
    assert_eq!(fs::read(&first_path).unwrap(), b"new results");
    assert_eq!(fs::read(&second_path).unwrap(), b"another writer");
    drop(first);
    drop(second);
    assert_eq!(fs::read(&staged_second).unwrap(), b"new mesh");
    assert!(error.message.contains(&staged_second.display().to_string()));
    fs::remove_file(&second_path).unwrap();
    fs::hard_link(&staged_second, &second_path).unwrap();
    assert_eq!(fs::read(&second_path).unwrap(), b"new mesh");
}

#[test]
fn staging_reports_missing_parent() {
    let scratch = Scratch::new();
    let source = scratch.path("source.bdf");
    fs::write(&source, b"source").unwrap();
    let destination = scratch.path("missing").join("output.vtu");
    assert_eq!(
        PendingOutput::new(&destination, &source, false)
            .err()
            .unwrap()
            .code,
        "E_IO"
    );
    assert!(!destination.exists());
}

#[cfg(unix)]
#[test]
fn invalid_parent_reports_io_for_both_destination_modes() {
    let scratch = Scratch::new();
    let source = scratch.path("source.bdf");
    let parent = scratch.path("ordinary-file");
    fs::write(&source, b"source").unwrap();
    fs::write(&parent, b"keep").unwrap();
    let destination = parent.join("output.vtu");
    for overwrite in [false, true] {
        assert_eq!(
            PendingOutput::new(&destination, &source, overwrite)
                .err()
                .unwrap()
                .code,
            "E_IO"
        );
    }
    assert_eq!(fs::read(&parent).unwrap(), b"keep");
}

#[cfg(unix)]
#[test]
fn overwrite_rechecks_for_a_symlink_created_during_staging() {
    use std::os::unix::fs::symlink;

    let scratch = Scratch::new();
    let source = scratch.path("source.bdf");
    let target = scratch.path("target.vtu");
    let destination = scratch.path("output.vtu");
    fs::write(&source, b"source").unwrap();
    fs::write(&target, b"keep").unwrap();
    let pending = PendingOutput::new(&destination, &source, true).unwrap();
    fs::write(pending.temporary(), b"staged").unwrap();
    symlink("target.vtu", &destination).unwrap();

    assert_eq!(pending.install().unwrap_err().code, "E_USAGE");
    assert_eq!(fs::read(&target).unwrap(), b"keep");
    assert!(
        fs::symlink_metadata(&destination)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}
