use std::env;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use fs4::fs_std::FileExt;

use super::{EngineArtifactError, EngineSpec, Platform};

const ENGINE_DIRECTORY_ENV: &str = "SPEECH2MD_ENGINE_DIR";

/// Filesystem location containing installed engine versions.
#[derive(Clone, Debug)]
pub struct EngineStore {
    root: PathBuf,
}

impl EngineStore {
    /// Creates a store at an explicitly selected root.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Returns the storage root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Requires an already installed engine without downloading anything.
    ///
    /// # Errors
    ///
    /// Returns an error if paths are unsafe or the executable is absent.
    pub fn require(&self, spec: &EngineSpec) -> Result<InstalledEngine, EngineArtifactError> {
        let install_dir = self.install_dir(spec)?;
        let executable = install_dir.join(&spec.executable_path);
        if !executable.is_file() {
            return Err(EngineArtifactError::MissingEngine {
                version: spec.version.clone(),
                platform: spec.platform,
            });
        }
        Ok(InstalledEngine {
            root: self.root.clone(),
            spec: spec.clone(),
            executable,
        })
    }

    /// Removes unlocked installed engines other than `keep`.
    ///
    /// # Errors
    ///
    /// Returns an error when the root cannot be inspected safely.
    pub fn prune(&self, keep: &EngineSpec) -> Result<PruneReport, EngineArtifactError> {
        fs::create_dir_all(&self.root).map_err(storage)?;
        let canonical_root = self.root.canonicalize().map_err(storage)?;
        let keep_path = self.install_dir(keep)?;
        let mut report = PruneReport::default();

        for version in fs::read_dir(&self.root).map_err(storage)? {
            let version = version.map_err(storage)?;
            if version.file_name() == ".locks" || !version.file_type().map_err(storage)?.is_dir() {
                continue;
            }
            for platform in fs::read_dir(version.path()).map_err(storage)? {
                let platform = platform.map_err(storage)?;
                if !platform.file_type().map_err(storage)?.is_dir()
                    || platform.path() == keep_path
                    || Platform::from_storage_name(&platform.file_name()).is_none()
                {
                    continue;
                }
                let candidate = platform.path().canonicalize().map_err(storage)?;
                if candidate == canonical_root || !candidate.starts_with(&canonical_root) {
                    return Err(EngineArtifactError::InvalidRoot(
                        "prune candidate escapes engine root".into(),
                    ));
                }
                let version_name = version.file_name().to_string_lossy().into_owned();
                let platform_name = platform.file_name().to_string_lossy().into_owned();
                let lock = self.open_lock(&version_name, &platform_name)?;
                if !FileExt::try_lock_exclusive(&lock)
                    .map_err(|error| EngineArtifactError::Lock(error.to_string()))?
                {
                    report.skipped_locked += 1;
                    continue;
                }
                fs::remove_dir_all(&candidate).map_err(storage)?;
                report.removed += 1;
            }
            if fs::read_dir(version.path())
                .map_err(storage)?
                .next()
                .is_none()
            {
                fs::remove_dir(version.path()).map_err(storage)?;
            }
        }
        Ok(report)
    }

    pub(super) fn install_dir(&self, spec: &EngineSpec) -> Result<PathBuf, EngineArtifactError> {
        if !safe_component(&spec.version) || !safe_relative_path(&spec.executable_path) {
            return Err(EngineArtifactError::InvalidManifest(
                "unsafe engine installation path".into(),
            ));
        }
        Ok(self
            .root
            .join(&spec.version)
            .join(spec.platform.to_string()))
    }

    fn open_lock(&self, version: &str, platform: &str) -> Result<File, EngineArtifactError> {
        let directory = self.root.join(".locks");
        fs::create_dir_all(&directory).map_err(storage)?;
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join(format!("{version}--{platform}.lock")))
            .map_err(storage)
    }
}

/// An installed engine that has not yet been locked for execution.
#[derive(Clone, Debug)]
pub struct InstalledEngine {
    root: PathBuf,
    spec: EngineSpec,
    executable: PathBuf,
}

impl InstalledEngine {
    /// Returns the verified executable path.
    #[must_use]
    pub fn executable(&self) -> &Path {
        &self.executable
    }

    /// Acquires a shared lock that prevents pruning while the engine runs.
    ///
    /// # Errors
    ///
    /// Returns an error if the lock file cannot be created or locked.
    pub fn acquire(&self) -> Result<EngineLease, EngineArtifactError> {
        let store = EngineStore::new(&self.root);
        let lock = store.open_lock(&self.spec.version, &self.spec.platform.to_string())?;
        FileExt::lock_shared(&lock)
            .map_err(|error| EngineArtifactError::Lock(error.to_string()))?;
        Ok(EngineLease {
            engine: self.clone(),
            _lock: lock,
        })
    }
}

/// Shared-lock guard for an engine being executed.
#[derive(Debug)]
pub struct EngineLease {
    engine: InstalledEngine,
    _lock: File,
}

impl EngineLease {
    /// Returns the executable protected by this lease.
    #[must_use]
    pub fn executable(&self) -> &Path {
        self.engine.executable()
    }
}

/// Counts the outcomes of an engine prune operation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PruneReport {
    /// Successfully removed installations.
    pub removed: usize,
    /// Installations retained because another process holds a shared lock.
    pub skipped_locked: usize,
}

/// Resolves the default engine storage root.
pub struct EngineRootResolver;

impl EngineRootResolver {
    /// Resolves `SPEECH2MD_ENGINE_DIR` or the platform data directory.
    ///
    /// # Errors
    ///
    /// Returns an error for a relative override or unavailable platform directory.
    pub fn resolve() -> Result<PathBuf, EngineArtifactError> {
        let override_path = env::var_os(ENGINE_DIRECTORY_ENV).map(PathBuf::from);
        let platform_path = ProjectDirs::from("", "", "speech2md")
            .map(|directories| directories.data_dir().join("engines"));
        resolve_paths(override_path, platform_path)
    }
}

fn resolve_paths(
    override_path: Option<PathBuf>,
    platform_path: Option<PathBuf>,
) -> Result<PathBuf, EngineArtifactError> {
    if let Some(path) = override_path {
        return path.is_absolute().then_some(path).ok_or_else(|| {
            EngineArtifactError::InvalidRoot(format!("{ENGINE_DIRECTORY_ENV} must be absolute"))
        });
    }
    platform_path.ok_or_else(|| {
        EngineArtifactError::InvalidRoot("platform data directory is unavailable".into())
    })
}

fn safe_component(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value.contains('/')
        && !value.contains('\\')
        && !value.contains(':')
}

fn safe_relative_path(path: &Path) -> bool {
    let normalized = path.to_string_lossy().replace('\\', "/");
    !normalized.starts_with('/')
        && normalized
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != ".." && !part.contains(':'))
}

fn storage(error: impl std::fmt::Display) -> EngineArtifactError {
    EngineArtifactError::Storage(error.to_string())
}

impl Platform {
    fn from_storage_name(name: &std::ffi::OsStr) -> Option<Self> {
        match name.to_str()? {
            "windows-x86_64" => Some(Self::WindowsX86_64),
            "macos-aarch64" => Some(Self::MacosAarch64),
            "macos-x86_64" => Some(Self::MacosX86_64),
            "linux-x86_64" => Some(Self::LinuxX86_64),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{EngineArtifactError, resolve_paths};

    #[test]
    fn absolute_override_has_priority() {
        let absolute = if cfg!(windows) {
            PathBuf::from(r"C:\engines")
        } else {
            PathBuf::from("/engines")
        };
        assert_eq!(
            resolve_paths(Some(absolute.clone()), Some(PathBuf::from("ignored")))
                .expect("absolute override is accepted"),
            absolute
        );
    }

    #[test]
    fn relative_override_and_missing_platform_root_are_rejected() {
        assert!(matches!(
            resolve_paths(Some(PathBuf::from("relative")), None),
            Err(EngineArtifactError::InvalidRoot(_))
        ));
        assert!(matches!(
            resolve_paths(None, None),
            Err(EngineArtifactError::InvalidRoot(_))
        ));
    }
}
