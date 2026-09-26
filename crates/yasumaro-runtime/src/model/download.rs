use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;

use reqwest::blocking::Client;
use sha2::{Digest, Sha256};

use super::{ModelError, ModelId, ModelManifest, ModelSpec, ModelStore};

pub struct ModelInstaller {
    manifest: ModelManifest,
    store: ModelStore,
    client: Client,
}

impl ModelInstaller {
    /// Creates a blocking model installer with bounded connection and request timeouts.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be constructed.
    pub fn new(manifest: ModelManifest, store: ModelStore) -> Result<Self, ModelError> {
        let client = Client::builder()
            .connect_timeout(std::time::Duration::from_secs(30))
            .timeout(std::time::Duration::from_mins(30))
            .build()
            .map_err(|error| ModelError::Download {
                id: ModelId::WhisperBase,
                message: format!("HTTP client initialization: {error}"),
            })?;
        Ok(Self {
            manifest,
            store,
            client,
        })
    }

    /// Downloads and verifies only the explicitly requested models.
    ///
    /// # Errors
    ///
    /// Returns an error for an unknown model, network or storage failure, interrupted
    /// response, size mismatch, or hash mismatch. Partial files are removed on failure.
    pub fn install(&self, ids: &[ModelId]) -> Result<(), ModelError> {
        fs::create_dir_all(self.store.root())
            .map_err(|error| ModelError::Storage(error.to_string()))?;
        for &id in ids {
            self.install_one(self.manifest.spec(id)?)?;
        }
        Ok(())
    }

    fn install_one(&self, spec: &ModelSpec) -> Result<(), ModelError> {
        tracing::info!(
            target: "yasumaro_runtime::model",
            model_id = %spec.id,
            expected_bytes = spec.size,
            "model installation started"
        );
        let final_path = self.store.path(spec.id);
        let partial_path = final_path.with_extension(format!(
            "{}.part",
            final_path
                .extension()
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or_default()
        ));
        let result = self
            .download_and_verify(spec, &partial_path)
            .and_then(|()| {
                fs::rename(&partial_path, &final_path)
                    .map_err(|error| ModelError::Storage(error.to_string()))
            });
        if result.is_err() {
            remove_partial(&partial_path)?;
        }
        if result.is_ok() {
            tracing::info!(
                target: "yasumaro_runtime::model",
                model_id = %spec.id,
                installed_bytes = spec.size,
                "model installation completed"
            );
        }
        result
    }

    fn download_and_verify(&self, spec: &ModelSpec, path: &Path) -> Result<(), ModelError> {
        let mut response = self
            .client
            .get(spec.url.clone())
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map_err(|error| ModelError::Download {
                id: spec.id,
                message: error.to_string(),
            })?;
        let mut output =
            File::create(path).map_err(|error| ModelError::Storage(error.to_string()))?;
        let mut hasher = Sha256::new();
        let mut actual_size = 0_u64;
        let mut buffer = vec![0_u8; 64 * 1_024].into_boxed_slice();
        loop {
            let count = response
                .read(&mut buffer)
                .map_err(|error| ModelError::Download {
                    id: spec.id,
                    message: error.to_string(),
                })?;
            if count == 0 {
                break;
            }
            actual_size = actual_size.saturating_add(
                u64::try_from(count).map_err(|error| ModelError::Storage(error.to_string()))?,
            );
            if actual_size > spec.size {
                return Err(ModelError::SizeMismatch {
                    id: spec.id,
                    expected: spec.size,
                    actual: actual_size,
                });
            }
            output
                .write_all(&buffer[..count])
                .map_err(|error| ModelError::Storage(error.to_string()))?;
            hasher.update(&buffer[..count]);
        }
        output
            .sync_all()
            .map_err(|error| ModelError::Storage(error.to_string()))?;
        if actual_size != spec.size {
            return Err(ModelError::SizeMismatch {
                id: spec.id,
                expected: spec.size,
                actual: actual_size,
            });
        }
        let actual_hash = format!("{:x}", hasher.finalize());
        if actual_hash != spec.sha256 {
            return Err(ModelError::HashMismatch {
                id: spec.id,
                expected: spec.sha256.clone(),
                actual: actual_hash,
            });
        }
        Ok(())
    }
}

fn remove_partial(path: &Path) -> Result<(), ModelError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(ModelError::Storage(error.to_string())),
    }
}
