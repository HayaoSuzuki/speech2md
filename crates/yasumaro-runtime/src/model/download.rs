use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::Path;
#[cfg(feature = "test-support")]
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use reqwest::Client;
use reqwest::redirect::Policy;
use sha2::{Digest, Sha256};
use tokio::runtime::{Builder as RuntimeBuilder, Runtime};

use super::store::remove_entry;
use super::{ModelError, ModelId, ModelManifest, ModelSpec, ModelStore};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const READ_TIMEOUT: Duration = Duration::from_secs(60);
const LOCK_RETRY_DELAY: Duration = Duration::from_millis(25);
const CANCELLATION_POLL_INTERVAL: Duration = Duration::from_millis(25);
const RUNTIME_SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(100);
const MAX_REDIRECTS: usize = 10;
const BUFFER_SIZE: usize = 64 * 1_024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublishCheckpoint {
    BeforeAuthorization,
    AfterAuthorization,
}

pub struct ModelInstaller {
    manifest: ModelManifest,
    store: ModelStore,
    client: Client,
    runtime: Option<Runtime>,
    #[cfg(feature = "test-support")]
    publish_observer: Option<Arc<dyn Fn(PublishCheckpoint) + Send + Sync>>,
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

    /// Creates a test installer that observes the two sides of publication authorization.
    ///
    /// # Errors
    ///
    /// Returns an error if the async runtime or HTTP client cannot be constructed.
    #[cfg(feature = "test-support")]
    pub fn new_for_test_with_publish_observer(
        manifest: ModelManifest,
        store: ModelStore,
        connect_timeout: Duration,
        read_timeout: Duration,
        observer: Arc<dyn Fn(PublishCheckpoint) + Send + Sync>,
    ) -> Result<Self, ModelError> {
        let mut installer = Self::with_timeouts(manifest, store, connect_timeout, read_timeout)?;
        installer.publish_observer = Some(observer);
        Ok(installer)
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
            .redirect(model_redirect_policy())
            .build()
            .map_err(|_| ModelError::HttpClientInitialization)?;
        Ok(Self {
            manifest,
            store,
            client,
            runtime: Some(runtime),
            #[cfg(feature = "test-support")]
            publish_observer: None,
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
                .and_then(|()| {
                    self.publish_authorized(
                        spec.id,
                        &paths.partial_path,
                        &paths.final_path,
                        cancelled,
                    )
                })
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
        let Some(runtime) = &self.runtime else {
            return Err(ModelError::HttpClientInitialization);
        };
        runtime.block_on(download_and_verify_async(
            &self.client,
            spec,
            path,
            cancelled,
        ))
    }

    fn publish_authorized(
        &self,
        id: ModelId,
        partial_path: &Path,
        final_path: &Path,
        cancelled: &AtomicBool,
    ) -> Result<(), ModelError> {
        self.observe_publish_checkpoint(PublishCheckpoint::BeforeAuthorization);
        check_cancelled(id, cancelled)?;
        self.observe_publish_checkpoint(PublishCheckpoint::AfterAuthorization);
        publish_verified(partial_path, final_path)
    }

    fn observe_publish_checkpoint(&self, checkpoint: PublishCheckpoint) {
        #[cfg(feature = "test-support")]
        if let Some(observer) = &self.publish_observer {
            observer(checkpoint);
        }
        #[cfg(not(feature = "test-support"))]
        let _ = checkpoint;
    }
}

impl Drop for ModelInstaller {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_timeout(RUNTIME_SHUTDOWN_TIMEOUT);
        }
    }
}

fn model_redirect_policy() -> Policy {
    Policy::custom(|attempt| {
        if redirect_is_allowed(attempt.previous(), attempt.url()) {
            attempt.follow()
        } else {
            attempt.error("model redirect rejected")
        }
    })
}

fn redirect_is_allowed(previous: &[url::Url], next: &url::Url) -> bool {
    let within_limit = previous.len() <= MAX_REDIRECTS;
    let downgrades_https = previous
        .first()
        .is_some_and(|initial| initial.scheme() == "https" && next.scheme() != "https");
    within_limit && !downgrades_https
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
mod runtime_tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, mpsc};
    use std::thread;
    use std::time::Duration;

    use reqwest::dns::{Addrs, Name, Resolve, Resolving};
    use sha2::{Digest, Sha256};
    use tempfile::TempDir;
    use url::Url;

    use super::{MAX_REDIRECTS, ModelInstaller, RuntimeBuilder, redirect_is_allowed};
    use crate::{ModelError, ModelId, ModelManifest, ModelSpec, ModelStore};

    #[derive(Debug)]
    struct BlockingResolver {
        started: mpsc::Sender<()>,
        release: Arc<Mutex<mpsc::Receiver<()>>>,
    }

    #[test]
    fn redirect_policy_is_bounded_and_rejects_https_downgrade() {
        let secure = Url::parse("https://models.example/first").expect("secure URL");
        let next_secure = Url::parse("https://cdn.example/model").expect("secure CDN URL");
        let next_insecure = Url::parse("http://cdn.example/model").expect("insecure CDN URL");

        assert!(redirect_is_allowed(
            std::slice::from_ref(&secure),
            &next_secure
        ));
        assert!(!redirect_is_allowed(
            std::slice::from_ref(&secure),
            &next_insecure
        ));
        assert!(!redirect_is_allowed(
            &vec![secure; MAX_REDIRECTS + 1],
            &next_secure
        ));
    }

    impl Resolve for BlockingResolver {
        fn resolve(&self, _name: Name) -> Resolving {
            let started = self.started.clone();
            let release = Arc::clone(&self.release);
            Box::pin(async move {
                let result = tokio::task::spawn_blocking(move || {
                    let _ignored = started.send(());
                    let _ignored = release
                        .lock()
                        .ok()
                        .and_then(|receiver| receiver.recv().ok());
                    Box::new(std::iter::empty()) as Addrs
                })
                .await;
                match result {
                    Ok(addresses) => Ok(addresses),
                    Err(error) => Err(Box::new(error) as Box<dyn std::error::Error + Send + Sync>),
                }
            })
        }
    }

    #[test]
    fn cancellation_is_not_delayed_by_runtime_drop_after_blocking_dns() {
        let root = TempDir::new().expect("temporary model root");
        let bytes = b"model";
        let manifest = ModelManifest::new(vec![ModelSpec {
            id: ModelId::WhisperBase,
            engine_version: "test".into(),
            url: Url::parse("http://dns-stall.invalid/model").expect("fixture URL"),
            size: u64::try_from(bytes.len()).expect("fixture size"),
            sha256: format!("{:x}", Sha256::digest(bytes)),
            license: "CC0-1.0".into(),
            file_name: "ggml-base.bin".into(),
        }])
        .expect("valid manifest");
        let (dns_started_tx, dns_started_rx) = mpsc::channel();
        let (dns_release_tx, dns_release_rx) = mpsc::channel();
        let resolver = Arc::new(BlockingResolver {
            started: dns_started_tx,
            release: Arc::new(Mutex::new(dns_release_rx)),
        });
        let runtime = RuntimeBuilder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .expect("runtime");
        let client = reqwest::Client::builder()
            .dns_resolver(resolver)
            .build()
            .expect("client");
        let installer = ModelInstaller {
            manifest,
            store: ModelStore::new(root.path()),
            client,
            runtime: Some(runtime),
            #[cfg(feature = "test-support")]
            publish_observer: None,
        };
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let (result_tx, result_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            let result = installer
                .install_with_cancellation(&[ModelId::WhisperBase], worker_cancelled.as_ref());
            drop(installer);
            let _ignored = result_tx.send(result);
        });
        dns_started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("DNS resolution starts");

        cancelled.store(true, Ordering::Release);
        let prompt_result = result_rx.recv_timeout(Duration::from_millis(250));
        let _ignored = dns_release_tx.send(());
        worker.join().expect("installer worker exits");

        assert!(matches!(
            prompt_result.expect("cancelled installer returns before DNS unblocks"),
            Err(ModelError::Cancelled {
                id: ModelId::WhisperBase
            })
        ));
    }
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
