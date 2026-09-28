use std::env;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use fs4::fs_std::FileExt as _;

use super::{ModelError, ModelId};

const MODEL_DIRECTORY_ENV: &str = "YASUMARO_MODEL_DIR";

#[derive(Clone, Debug)]
pub struct ModelStore {
    root: PathBuf,
}

#[derive(Debug)]
pub struct ModelLease {
    path: PathBuf,
    _lock: File,
}

impl ModelLease {
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub(super) struct ModelPaths {
    pub final_path: PathBuf,
    pub partial_path: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RemovalKind {
    Directory,
    FileLike,
}

impl ModelStore {
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns the local path for an installed model without performing network I/O.
    ///
    /// # Errors
    ///
    /// Returns [`ModelError::MissingModel`] when the expected local file does not exist.
    pub fn require(&self, id: ModelId) -> Result<PathBuf, ModelError> {
        let path = self.path(id);
        is_regular_file(&path)
            .then_some(path)
            .ok_or_else(|| missing_model(id))
    }

    /// Acquires a shared lease for an installed regular model file.
    ///
    /// # Errors
    ///
    /// Returns an error when locking fails or the model is not a regular file after the lock is
    /// acquired.
    pub fn acquire(&self, id: ModelId) -> Result<ModelLease, ModelError> {
        let lock = self.open_lock(id)?;
        lock.lock_shared()
            .map_err(|error| lock_error(id, "acquire shared lock", &error))?;
        let path = self.path(id);
        if !is_regular_file(&path) {
            return Err(missing_model(id));
        }
        Ok(ModelLease { path, _lock: lock })
    }

    /// Removes a model and any stale partial file without waiting for active users.
    ///
    /// # Errors
    ///
    /// Returns [`ModelError::ModelInUse`] when the model lock is busy. Lock setup and filesystem
    /// failures retain their distinct error classifications.
    pub fn remove(&self, id: ModelId) -> Result<(), ModelError> {
        let lock = self.open_lock(id)?;
        if lock
            .try_lock_exclusive()
            .map_err(|error| lock_error(id, "try exclusive lock", &error))?
        {
            let paths = self.paths(id);
            remove_entry(&paths.partial_path)?;
            remove_entry(&paths.final_path)
        } else {
            Err(ModelError::ModelInUse { id })
        }
    }

    pub(super) fn lock_exclusive(&self, id: ModelId) -> Result<File, ModelError> {
        let lock = self.open_lock(id)?;
        lock.lock_exclusive()
            .map_err(|error| lock_error(id, "acquire exclusive lock", &error))?;
        Ok(lock)
    }

    pub(super) fn paths(&self, id: ModelId) -> ModelPaths {
        let final_path = self.path(id);
        let mut partial_name = OsString::from(final_path.as_os_str());
        partial_name.push(".part");
        ModelPaths {
            final_path,
            partial_path: PathBuf::from(partial_name),
        }
    }

    pub(super) fn path(&self, id: ModelId) -> PathBuf {
        self.root.join(id.file_name())
    }

    fn open_lock(&self, id: ModelId) -> Result<File, ModelError> {
        let lock_directory = self.root.join(".locks");
        fs::create_dir_all(&lock_directory)
            .map_err(|error| lock_error(id, "create lock directory", &error))?;
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_directory.join(format!("{id}.lock")))
            .map_err(|error| lock_error(id, "open lock file", &error))
    }
}

pub(super) fn remove_entry(path: &Path) -> Result<(), ModelError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(storage_error("inspect model entry", &error)),
    };
    let file_type = metadata.file_type();
    let result = match removal_kind(file_type.is_dir(), is_directory_symlink(file_type)) {
        RemovalKind::Directory => fs::remove_dir(path),
        RemovalKind::FileLike => fs::remove_file(path),
    };
    result.map_err(|error| storage_error("remove model entry", &error))
}

const fn removal_kind(is_directory: bool, is_directory_symlink: bool) -> RemovalKind {
    if is_directory || is_directory_symlink {
        RemovalKind::Directory
    } else {
        RemovalKind::FileLike
    }
}

#[cfg(windows)]
fn is_directory_symlink(file_type: fs::FileType) -> bool {
    use std::os::windows::fs::FileTypeExt as _;

    file_type.is_symlink_dir()
}

#[cfg(not(windows))]
const fn is_directory_symlink(_file_type: fs::FileType) -> bool {
    false
}

fn is_regular_file(path: &Path) -> bool {
    // This store deliberately rejects symlinks and special files, so regular-file semantics are
    // narrower than Clippy's suggested "not a directory" check.
    #[allow(
        clippy::filetype_is_file,
        reason = "model paths must reject symlinks and special files"
    )]
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_file())
}

fn missing_model(id: ModelId) -> ModelError {
    ModelError::MissingModel {
        id,
        install_command: format!("yasumaro model install {id}"),
    }
}

fn lock_error(id: ModelId, operation: &str, error: &io::Error) -> ModelError {
    ModelError::Lock {
        id,
        message: format!("{operation}: {}", error.kind()),
    }
}

fn storage_error(operation: &str, error: &io::Error) -> ModelError {
    ModelError::Storage(format!("{operation}: {}", error.kind()))
}

pub struct ModelRootResolver;

impl ModelRootResolver {
    /// Resolves the platform model directory, honoring `YASUMARO_MODEL_DIR`.
    ///
    /// # Errors
    ///
    /// Returns an error when the override is relative or a platform data directory
    /// cannot be determined.
    pub fn resolve() -> Result<PathBuf, ModelError> {
        let override_path = env::var_os(MODEL_DIRECTORY_ENV).map(PathBuf::from);
        resolve_paths(override_path, default_model_root())
    }
}

fn default_model_root() -> Option<PathBuf> {
    ProjectDirs::from("", "", "yasumaro")
        .map(|directories| directories.data_local_dir().join("models"))
}

fn resolve_paths(
    override_path: Option<PathBuf>,
    platform_path: Option<PathBuf>,
) -> Result<PathBuf, ModelError> {
    if let Some(path) = override_path {
        return path.is_absolute().then_some(path).ok_or_else(|| {
            ModelError::InvalidModelRoot(format!("{MODEL_DIRECTORY_ENV} must be absolute"))
        });
    }
    platform_path.ok_or_else(|| {
        ModelError::InvalidModelRoot("platform data directory is unavailable".into())
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use directories::ProjectDirs;

    use super::{
        ModelError, ModelRootResolver, RemovalKind, default_model_root, removal_kind, resolve_paths,
    };

    #[test]
    fn windows_directory_symlink_uses_directory_removal() {
        assert_eq!(removal_kind(false, true), RemovalKind::Directory);
    }

    #[test]
    fn absolute_override_has_priority() {
        let override_path = if cfg!(windows) {
            PathBuf::from(r"C:\models")
        } else {
            PathBuf::from("/models")
        };

        let resolved = resolve_paths(Some(override_path.clone()), Some(PathBuf::from("ignored")))
            .expect("absolute override is valid");

        assert_eq!(resolved, override_path);
    }

    #[test]
    fn relative_override_and_missing_platform_directory_are_rejected() {
        assert!(matches!(
            resolve_paths(Some(PathBuf::from("relative")), None),
            Err(ModelError::InvalidModelRoot(_))
        ));
        assert!(matches!(
            resolve_paths(None, None),
            Err(ModelError::InvalidModelRoot(_))
        ));
    }

    #[test]
    fn platform_resolution_returns_an_absolute_directory() {
        let path = ModelRootResolver::resolve().expect("this platform provides a data directory");

        assert!(path.is_absolute());
        assert!(path.ends_with("models"));
    }

    #[test]
    fn platform_resolution_uses_the_local_data_directory() {
        let expected = ProjectDirs::from("", "", "yasumaro")
            .expect("platform data directory")
            .data_local_dir()
            .join("models");

        assert_eq!(
            default_model_root().expect("resolve default model root"),
            expected
        );
    }
}
