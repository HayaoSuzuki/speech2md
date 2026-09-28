use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use reqwest::Client;
use sha2::{Digest, Sha256};
use tokio::runtime::{Builder as RuntimeBuilder, Runtime};

use super::store::remove_entry;
use super::{ModelError, ModelId, ModelManifest, ModelSpec, ModelStore};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const READ_TIMEOUT: Duration = Duration::from_secs(60);
const LOCK_RETRY_DELAY: Duration = Duration::from_millis(25);
const CANCELLATION_POLL_INTERVAL: Duration = Duration::from_millis(25);
const BUFFER_SIZE: usize = 64 * 1_024;

pub struct ModelInstaller {
    manifest: ModelManifest,
    store: ModelStore,
    client: Client,
    runtime: Runtime,
}

impl ModelInstaller {
    /// Creates a blocking model installer with bounded connection and body-read timeouts.
    ///
    /// # Errors
    ///
    /// Returns an error if the async runtime or HTTP client cannot be constructed.
    pub fn new(manifest: ModelManifest, store: ModelStore) -> Result<Self, ModelError> {
        Self::with_timeouts(manifest, store, CONNECT_TIMEOUT, READ_TIMEOUT)
    }

    /// Creates an installer with deterministic timeout bounds for contract tests.
    ///
    /// # Errors
    ///
    /// Returns an error if the async runtime or HTTP client cannot be constructed.
    #[cfg(feature = "test-support")]
    pub fn new_for_test(
        manifest: ModelManifest,
        store: ModelStore,
        connect_timeout: Duration,
        read_timeout: Duration,
    ) -> Result<Self, ModelError> {
        Self::with_timeouts(manifest, store, connect_timeout, read_timeout)
    }

    fn with_timeouts(
        manifest: ModelManifest,
        store: ModelStore,
        connect_timeout: Duration,
        read_timeout: Duration,
    ) -> Result<Self, ModelError> {
        let runtime = RuntimeBuilder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .map_err(|_| ModelError::HttpClientInitialization)?;
        let client = Client::builder()
            .connect_timeout(connect_timeout)
            .read_timeout(read_timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| ModelError::HttpClientInitialization)?;
        Ok(Self {
            manifest,
            store,
            client,
            runtime,
        })
    }

    /// Downloads and verifies only the explicitly requested models.
    ///
    /// # Errors
    ///
    /// Returns an error for an unknown model, network or storage failure, interrupted response,
    /// size mismatch, or hash mismatch. Partial files are removed on failure.
    pub fn install(&self, ids: &[ModelId]) -> Result<(), ModelError> {
        let cancelled = AtomicBool::new(false);
        self.install_with_cancellation(ids, &cancelled)
    }

    /// Installs models while observing a caller-owned cancellation flag.
    ///
    /// # Errors
    ///
    /// Has the same errors as [`Self::install`] and returns [`ModelError::Cancelled`] after
    /// cancellation is observed. Any partial file is removed before the error is returned.
    pub fn install_with_cancellation(
        &self,
        ids: &[ModelId],
        cancelled: &AtomicBool,
    ) -> Result<(), ModelError> {
        for &id in ids {
            self.install_one(self.manifest.spec(id)?, cancelled)?;
        }
        Ok(())
    }

    fn install_one(&self, spec: &ModelSpec, cancelled: &AtomicBool) -> Result<(), ModelError> {
        let _lock = self.lock_with_cancellation(spec.id, cancelled)?;
        check_cancelled(spec.id, cancelled)?;
        let paths = self.store.paths(spec.id);
        remove_entry(&paths.partial_path)?;

        if Self::existing_final_is_valid(spec, &paths.final_path, cancelled)? {
            check_cancelled(spec.id, cancelled)?;
            tracing::info!(
                target: "yasumaro_runtime::model",
                model_id = %spec.id,
                installed_bytes = spec.size,
                "verified model already installed"
            );
            return Ok(());
        }
        remove_entry(&paths.final_path)?;

        tracing::info!(
            target: "yasumaro_runtime::model",
            model_id = %spec.id,
            expected_bytes = spec.size,
            "model installation started"
        );
        let result = if cancelled.load(Ordering::Acquire) {
            Err(ModelError::Cancelled { id: spec.id })
        } else {
            self.download_and_verify(spec, &paths.partial_path, cancelled)
                .and_then(|()| publish_verified(&paths.partial_path, &paths.final_path))
        };
        if let Err(source) = result {
            return Err(failure_after_cleanup(spec.id, &paths.partial_path, source));
        }

        tracing::info!(
            target: "yasumaro_runtime::model",
            model_id = %spec.id,
            installed_bytes = spec.size,
            "model installation completed"
        );
        Ok(())
    }

    fn lock_with_cancellation(
        &self,
        id: ModelId,
        cancelled: &AtomicBool,
    ) -> Result<File, ModelError> {
        loop {
            check_cancelled(id, cancelled)?;
            if let Some(lock) = self.store.try_lock_exclusive(id)? {
                return Ok(lock);
            }
            thread::sleep(LOCK_RETRY_DELAY);
        }
    }

    fn existing_final_is_valid(
        spec: &ModelSpec,
        path: &Path,
        cancelled: &AtomicBool,
    ) -> Result<bool, ModelError> {
        check_cancelled(spec.id, cancelled)?;
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(storage_error("inspect final model", &error)),
        };
        #[allow(
            clippy::filetype_is_file,
            reason = "installed models must reject symlinks and special files"
        )]
        if !metadata.file_type().is_file() {
            return Ok(false);
        }
        let mut file =
            File::open(path).map_err(|error| storage_error("open final model", &error))?;
        let (size, hash) = read_size_and_hash(&mut file, spec.id, cancelled)?;
        Ok(size == spec.size && hash == spec.sha256)
    }

    fn download_and_verify(
        &self,
        spec: &ModelSpec,
        path: &Path,
        cancelled: &AtomicBool,
    ) -> Result<(), ModelError> {
        self.runtime.block_on(download_and_verify_async(
            &self.client,
            spec,
            path,
            cancelled,
        ))
    }
}

async fn download_and_verify_async(
    client: &Client,
    spec: &ModelSpec,
    path: &Path,
    cancelled: &AtomicBool,
) -> Result<(), ModelError> {
    check_cancelled(spec.id, cancelled)?;
    let request = client.get(spec.url.clone()).send();
    let mut response = tokio::select! {
        result = request => result.map_err(|error| download_error(spec.id, &error))?,
        () = wait_for_cancellation(cancelled) => {
            return Err(ModelError::Cancelled { id: spec.id });
        }
    };
    check_cancelled(spec.id, cancelled)?;
    if !response.status().is_success() {
        return Err(ModelError::Download {
            id: spec.id,
            message: format!("HTTP status {}", response.status()),
        });
    }
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| storage_error("create partial model", &error))?;
    let mut hasher = Sha256::new();
    let mut actual_size = 0_u64;
    loop {
        check_cancelled(spec.id, cancelled)?;
        let chunk = tokio::select! {
            result = response.chunk() => {
                result.map_err(|error| download_error(spec.id, &error))?
            }
            () = wait_for_cancellation(cancelled) => {
                return Err(ModelError::Cancelled { id: spec.id });
            }
        };
        check_cancelled(spec.id, cancelled)?;
        let Some(chunk) = chunk else {
            break;
        };
        let count = chunk.len();
        actual_size = actual_size
            .checked_add(u64::try_from(count).map_err(|_| {
                ModelError::Storage("count downloaded bytes: numeric overflow".into())
            })?)
            .ok_or_else(|| ModelError::Storage("count downloaded bytes: overflow".into()))?;
        if actual_size > spec.size {
            return Err(ModelError::SizeMismatch {
                id: spec.id,
                expected: spec.size,
                actual: actual_size,
            });
        }
        output
            .write_all(&chunk)
            .map_err(|error| storage_error("write partial model", &error))?;
        check_cancelled(spec.id, cancelled)?;
        hasher.update(&chunk);
    }
    check_cancelled(spec.id, cancelled)?;
    output
        .sync_all()
        .map_err(|error| storage_error("sync partial model", &error))?;
    check_cancelled(spec.id, cancelled)?;
    let file_size = output
        .metadata()
        .map_err(|error| storage_error("inspect partial model", &error))?
        .len();
    check_cancelled(spec.id, cancelled)?;
    if file_size != spec.size {
        return Err(ModelError::SizeMismatch {
            id: spec.id,
            expected: spec.size,
            actual: file_size,
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
    check_cancelled(spec.id, cancelled)?;
    Ok(())
}

async fn wait_for_cancellation(cancelled: &AtomicBool) {
    loop {
        if cancelled.load(Ordering::Acquire) {
            return;
        }
        tokio::time::sleep(CANCELLATION_POLL_INTERVAL).await;
    }
}

fn read_size_and_hash(
    reader: &mut impl Read,
    id: ModelId,
    cancelled: &AtomicBool,
) -> Result<(u64, String), ModelError> {
    let mut size = 0_u64;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; BUFFER_SIZE].into_boxed_slice();
    loop {
        check_cancelled(id, cancelled)?;
        let count = reader
            .read(&mut buffer)
            .map_err(|error| storage_error("read final model", &error))?;
        if count == 0 {
            break;
        }
        size = size
            .checked_add(
                u64::try_from(count).map_err(|_| {
                    ModelError::Storage("count final bytes: numeric overflow".into())
                })?,
            )
            .ok_or_else(|| ModelError::Storage("count final bytes: overflow".into()))?;
        hasher.update(&buffer[..count]);
    }
    check_cancelled(id, cancelled)?;
    Ok((size, format!("{:x}", hasher.finalize())))
}

fn check_cancelled(id: ModelId, cancelled: &AtomicBool) -> Result<(), ModelError> {
    if cancelled.load(Ordering::Acquire) {
        Err(ModelError::Cancelled { id })
    } else {
        Ok(())
    }
}

fn publish_verified(partial_path: &Path, final_path: &Path) -> Result<(), ModelError> {
    fs::rename(partial_path, final_path).map_err(|error| storage_error("publish model", &error))
}

fn failure_after_cleanup(id: ModelId, partial_path: &Path, source: ModelError) -> ModelError {
    match remove_entry(partial_path) {
        Ok(()) => source,
        Err(cleanup) => ModelError::CleanupFailed {
            id,
            source: Box::new(source),
            cleanup: cleanup.to_string(),
        },
    }
}

fn download_error(id: ModelId, error: &reqwest::Error) -> ModelError {
    let message = if error.is_timeout() {
        "request timed out".into()
    } else if error.is_connect() {
        "connection failed".into()
    } else if let Some(status) = error.status() {
        format!("HTTP status {status}")
    } else if error.is_body() {
        "response body failed".into()
    } else {
        "request failed".into()
    };
    ModelError::Download { id, message }
}

fn storage_error(operation: &str, error: &io::Error) -> ModelError {
    ModelError::Storage(format!("{operation}: {}", error.kind()))
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::{ModelError, ModelId, failure_after_cleanup};

    #[test]
    fn publish_and_cleanup_failure_preserves_both_errors() {
        let root = TempDir::new().expect("temporary model root");
        let partial = root.path().join("ggml-base.bin.part");
        std::fs::create_dir(&partial).expect("create partial directory");
        std::fs::write(partial.join("child"), b"blocks cleanup").expect("write child");
        let source = ModelError::Storage("publish model: other error".into());

        let error = failure_after_cleanup(ModelId::WhisperBase, &partial, source.clone());

        assert!(matches!(
            error,
            ModelError::CleanupFailed {
                id: ModelId::WhisperBase,
                source: actual,
                cleanup,
            } if *actual == source && cleanup.contains("remove model entry")
        ));
    }
}
