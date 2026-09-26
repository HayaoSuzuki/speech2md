use std::fs;
use std::io::Write as _;
use std::path::Path;

#[cfg(windows)]
use tempfile::PersistError;
use tempfile::{Builder, NamedTempFile};

use crate::RuntimeError;

pub fn atomic_write(path: &Path, bytes: &[u8], force: bool) -> Result<(), RuntimeError> {
    let exists = validate_output_target(path)?;
    if exists && !force {
        return Err(RuntimeError::OutputExists);
    }
    let parent = path
        .parent()
        .filter(|parent| parent.is_dir())
        .ok_or_else(|| RuntimeError::Output("output directory does not exist".into()))?;
    let mut temporary = Builder::new()
        .prefix(".speech2md-output-")
        .tempfile_in(parent)
        .map_err(output_error)?;
    temporary.write_all(bytes).map_err(output_error)?;
    temporary.flush().map_err(output_error)?;
    temporary.as_file().sync_all().map_err(output_error)?;

    commit(temporary, path, force)
}

#[allow(
    clippy::filetype_is_file,
    reason = "an output replacement must reject directories, symlinks, and special files"
)]
pub fn validate_output_target(path: &Path) -> Result<bool, RuntimeError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => Ok(true),
        Ok(_metadata) => Err(RuntimeError::Output(
            "output target is not a regular file".into(),
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(output_error(error)),
    }
}

#[cfg(not(windows))]
fn commit(temporary: NamedTempFile, path: &Path, force: bool) -> Result<(), RuntimeError> {
    if force {
        temporary
            .persist(path)
            .map_err(|error| output_error(error.error))?;
    } else {
        temporary
            .persist_noclobber(path)
            .map_err(|error| match error.error.kind() {
                std::io::ErrorKind::AlreadyExists => RuntimeError::OutputExists,
                _ => output_error(error.error),
            })?;
    }
    sync_parent(path)
}

#[cfg(windows)]
fn commit(temporary: NamedTempFile, path: &Path, force: bool) -> Result<(), RuntimeError> {
    commit_windows_with(temporary, path, force, |file, destination| {
        file.persist(destination).map(|_file| ())
    })
}

#[cfg(windows)]
fn commit_windows_with(
    temporary: NamedTempFile,
    path: &Path,
    force: bool,
    persist: impl FnOnce(NamedTempFile, &Path) -> Result<(), PersistError>,
) -> Result<(), RuntimeError> {
    if !force {
        temporary
            .persist_noclobber(path)
            .map_err(|error| match error.error.kind() {
                std::io::ErrorKind::AlreadyExists => RuntimeError::OutputExists,
                _ => output_error(error.error),
            })?;
        sync_parent(path);
        return Ok(());
    }
    if !path.exists() {
        persist(temporary, path).map_err(|error| output_error(error.error))?;
        sync_parent(path);
        return Ok(());
    }

    let backup = unused_backup_path(path)?;
    fs::rename(path, &backup).map_err(output_error)?;
    if let Err(error) = persist(temporary, path) {
        let restore = fs::rename(&backup, path);
        return match restore {
            Ok(()) => Err(output_error(error.error)),
            Err(restore_error) => Err(RuntimeError::Output(format!(
                "commit failed and the previous output could not be restored: {restore_error}"
            ))),
        };
    }
    fs::remove_file(&backup).map_err(output_error)?;
    sync_parent(path);
    Ok(())
}

#[cfg(windows)]
fn unused_backup_path(path: &Path) -> Result<std::path::PathBuf, RuntimeError> {
    let parent = path
        .parent()
        .ok_or_else(|| RuntimeError::Output("output has no parent directory".into()))?;
    let placeholder = Builder::new()
        .prefix(".speech2md-backup-")
        .tempfile_in(parent)
        .map_err(output_error)?;
    let backup = placeholder.path().to_path_buf();
    placeholder.close().map_err(output_error)?;
    Ok(backup)
}

#[cfg(not(windows))]
fn sync_parent(path: &Path) -> Result<(), RuntimeError> {
    let parent = path
        .parent()
        .ok_or_else(|| RuntimeError::Output("output has no parent directory".into()))?;
    fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(output_error)
}

#[cfg(windows)]
const fn sync_parent(_path: &Path) {}

fn output_error(error: impl std::fmt::Display) -> RuntimeError {
    RuntimeError::Output(error.to_string())
}

#[cfg(all(test, windows))]
mod tests {
    use std::fs;
    use std::io::{self, Write as _};

    use tempfile::{NamedTempFile, PersistError, TempDir};

    use super::commit_windows_with;

    #[test]
    fn failed_commit_restores_the_previous_output_bytes() {
        let root = TempDir::new().expect("output root");
        let output = root.path().join("minutes.md");
        fs::write(&output, b"previous bytes").expect("previous output");
        let mut temporary = NamedTempFile::new_in(root.path()).expect("temporary output");
        temporary.write_all(b"new bytes").expect("staged output");

        let result = commit_windows_with(temporary, &output, true, |file, _path| {
            Err(PersistError {
                error: io::Error::new(io::ErrorKind::PermissionDenied, "injected commit failure"),
                file,
            })
        });

        assert!(result.is_err());
        assert_eq!(
            fs::read(&output).expect("restored output"),
            b"previous bytes"
        );
    }
}
