use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;

use super::{EngineArtifactError, EngineSpec};

const MIN_EXTRACTION_BUDGET: u64 = 64 * 1_024 * 1_024;
const MAX_EXPANSION_FACTOR: u64 = 20;

pub(super) fn extract(
    archive_path: &Path,
    spec: &EngineSpec,
    destination: &Path,
) -> Result<(), EngineArtifactError> {
    let mut budget = ExtractionBudget::new(
        spec.size
            .saturating_mul(MAX_EXPANSION_FACTOR)
            .max(MIN_EXTRACTION_BUDGET),
    );
    let archive_name = Path::new(&spec.archive_name);
    let extension = archive_name.extension().and_then(std::ffi::OsStr::to_str);
    let is_tar_gz = extension.is_some_and(|value| value.eq_ignore_ascii_case("gz"))
        && archive_name
            .file_stem()
            .map(Path::new)
            .and_then(Path::extension)
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|value| value.eq_ignore_ascii_case("tar"));
    if extension.is_some_and(|value| value.eq_ignore_ascii_case("zip")) {
        extract_zip(archive_path, destination, &mut budget)?;
    } else if is_tar_gz {
        extract_tar_gz(archive_path, destination, &mut budget)?;
    } else {
        return Err(EngineArtifactError::InvalidArchive(format!(
            "unsupported archive name {}",
            spec.archive_name
        )));
    }

    let executable = destination.join(&spec.executable_path);
    if !executable.is_file() {
        return Err(EngineArtifactError::InvalidArchive(
            "manifest executable is absent".into(),
        ));
    }
    #[cfg(unix)]
    set_executable(&executable)?;
    Ok(())
}

fn extract_zip(
    archive_path: &Path,
    destination: &Path,
    budget: &mut ExtractionBudget,
) -> Result<(), EngineArtifactError> {
    let input = File::open(archive_path).map_err(storage)?;
    let mut archive = zip::ZipArchive::new(input).map_err(invalid)?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(invalid)?;
        let relative = safe_relative(entry.name())?;
        budget.consume(entry.size())?;
        let output = destination.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&output).map_err(storage)?;
            continue;
        }
        if !entry.is_file()
            || entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170_000 != 0o100_000)
        {
            return Err(EngineArtifactError::InvalidArchive(
                "ZIP contains a non-regular entry".into(),
            ));
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(storage)?;
        }
        let mut file = File::create(&output).map_err(storage)?;
        io::copy(&mut entry, &mut file).map_err(storage)?;
        file.sync_all().map_err(storage)?;
    }
    Ok(())
}

fn extract_tar_gz(
    archive_path: &Path,
    destination: &Path,
    budget: &mut ExtractionBudget,
) -> Result<(), EngineArtifactError> {
    let input = File::open(archive_path).map_err(storage)?;
    let mut archive = tar::Archive::new(GzDecoder::new(input));
    for entry in archive.entries().map_err(invalid)? {
        let mut entry = entry.map_err(invalid)?;
        let kind = entry.header().entry_type();
        if !kind.is_file() && !kind.is_dir() {
            return Err(EngineArtifactError::InvalidArchive(
                "tar contains a non-regular entry".into(),
            ));
        }
        let path = entry.path().map_err(invalid)?;
        budget.consume(entry.size())?;
        let relative = safe_relative(&path.to_string_lossy())?;
        let output = destination.join(relative);
        if kind.is_dir() {
            fs::create_dir_all(&output).map_err(storage)?;
        } else {
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent).map_err(storage)?;
            }
            let mut file = File::create(&output).map_err(storage)?;
            io::copy(&mut entry, &mut file).map_err(storage)?;
            file.sync_all().map_err(storage)?;
        }
    }
    Ok(())
}

struct ExtractionBudget {
    remaining: u64,
}

impl ExtractionBudget {
    const fn new(limit: u64) -> Self {
        Self { remaining: limit }
    }

    fn consume(&mut self, bytes: u64) -> Result<(), EngineArtifactError> {
        self.remaining = self.remaining.checked_sub(bytes).ok_or_else(|| {
            EngineArtifactError::InvalidArchive("expanded archive is too large".into())
        })?;
        Ok(())
    }
}

fn safe_relative(raw: &str) -> Result<PathBuf, EngineArtifactError> {
    let normalized = raw.replace('\\', "/");
    if normalized.starts_with('/') {
        return Err(EngineArtifactError::InvalidArchive(
            "absolute archive path".into(),
        ));
    }
    let mut result = PathBuf::new();
    for part in normalized.split('/').filter(|part| !part.is_empty()) {
        if part == "." || part == ".." || part.contains(':') {
            return Err(EngineArtifactError::InvalidArchive(
                "unsafe archive path".into(),
            ));
        }
        result.push(part);
    }
    if result.as_os_str().is_empty() {
        return Err(EngineArtifactError::InvalidArchive(
            "empty archive path".into(),
        ));
    }
    Ok(result)
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<(), EngineArtifactError> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path).map_err(storage)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).map_err(storage)
}

fn invalid(error: impl std::fmt::Display) -> EngineArtifactError {
    EngineArtifactError::InvalidArchive(error.to_string())
}

fn storage(error: impl std::fmt::Display) -> EngineArtifactError {
    EngineArtifactError::Storage(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{ExtractionBudget, safe_relative};

    #[test]
    fn rejects_drive_relative_windows_paths_on_every_host() {
        assert!(safe_relative("C:escape.exe").is_err());
    }

    #[test]
    fn rejects_windows_alternate_data_streams_on_every_host() {
        assert!(safe_relative("bin/whisper-cli.exe:stream").is_err());
    }

    #[test]
    fn rejects_entries_that_exceed_the_total_extraction_budget() {
        let mut budget = ExtractionBudget::new(10);
        budget.consume(6).expect("first entry fits");
        assert!(budget.consume(5).is_err());
    }
}
