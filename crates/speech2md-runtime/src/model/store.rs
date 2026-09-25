use std::env;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;

use super::{ModelError, ModelId};

const MODEL_DIRECTORY_ENV: &str = "SPEECH2MD_MODEL_DIR";

#[derive(Clone, Debug)]
pub struct ModelStore {
    root: PathBuf,
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
        path.is_file()
            .then_some(path)
            .ok_or_else(|| ModelError::MissingModel {
                id,
                install_command: format!("speech2md model install {id}"),
            })
    }

    pub(super) fn path(&self, id: ModelId) -> PathBuf {
        self.root.join(id.file_name())
    }
}

pub struct ModelRootResolver;

impl ModelRootResolver {
    /// Resolves the platform model directory, honoring `SPEECH2MD_MODEL_DIR`.
    ///
    /// # Errors
    ///
    /// Returns an error when the override is relative or a platform data directory
    /// cannot be determined.
    pub fn resolve() -> Result<PathBuf, ModelError> {
        let override_path = env::var_os(MODEL_DIRECTORY_ENV).map(PathBuf::from);
        let platform_path = ProjectDirs::from("", "", "speech2md")
            .map(|directories| directories.data_dir().join("models"));
        resolve_paths(override_path, platform_path)
    }
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

    use super::{ModelError, ModelRootResolver, resolve_paths};

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
}
