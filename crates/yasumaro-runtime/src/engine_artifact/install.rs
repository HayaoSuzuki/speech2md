use std::fs;
use std::io::{Read, Write};
use std::time::Duration;

use fs4::fs_std::FileExt;
use reqwest::blocking::Client;
use sha2::{Digest, Sha256};

use super::store::write_integrity_receipt;
use super::{EngineArtifactError, EngineSpec, EngineStore, InstalledEngine, archive};

/// Supplies archive bytes to the installer. Only implementations of this trait perform download I/O.
pub trait EngineArchiveSource {
    /// Writes the complete archive into `destination`.
    ///
    /// # Errors
    ///
    /// Returns a download or storage error when the transfer cannot finish.
    fn download(
        &self,
        spec: &EngineSpec,
        destination: &mut dyn Write,
    ) -> Result<(), EngineArtifactError>;
}

/// HTTPS archive source used by the `engine install` command.
pub struct HttpEngineArchiveSource {
    client: Client,
}

impl HttpEngineArchiveSource {
    /// Builds a client with bounded connect and request timeouts.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be initialized.
    pub fn new() -> Result<Self, EngineArtifactError> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(30))
            .timeout(Duration::from_mins(30))
            .build()
            .map_err(|error| EngineArtifactError::Download(error.to_string()))?;
        Ok(Self { client })
    }
}

impl EngineArchiveSource for HttpEngineArchiveSource {
    fn download(
        &self,
        spec: &EngineSpec,
        destination: &mut dyn Write,
    ) -> Result<(), EngineArtifactError> {
        let mut response = self
            .client
            .get(spec.url.clone())
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map_err(|error| EngineArtifactError::Download(error.to_string()))?;
        let mut buffer = vec![0_u8; 64 * 1_024].into_boxed_slice();
        loop {
            let count = response
                .read(&mut buffer)
                .map_err(|error| EngineArtifactError::Download(error.to_string()))?;
            if count == 0 {
                return Ok(());
            }
            destination
                .write_all(&buffer[..count])
                .map_err(|error| EngineArtifactError::Download(error.to_string()))?;
        }
    }
}

/// Installs verified engine archives into an [`EngineStore`].
pub struct EngineInstaller<S> {
    source: S,
    store: EngineStore,
}

impl<S: EngineArchiveSource> EngineInstaller<S> {
    /// Creates an installer whose source is invoked only by [`Self::install`].
    #[must_use]
    pub const fn new(source: S, store: EngineStore) -> Self {
        Self { source, store }
    }

    /// Downloads, verifies, safely extracts, and atomically publishes one engine.
    ///
    /// # Errors
    ///
    /// Returns an error on transfer, verification, extraction, or storage failure.
    pub fn install(&self, spec: &EngineSpec) -> Result<InstalledEngine, EngineArtifactError> {
        let final_path = self.store.install_dir(spec)?;
        fs::create_dir_all(self.store.root()).map_err(storage)?;
        let install_lock = self
            .store
            .open_lock(&spec.version, &spec.platform.to_string())?;
        FileExt::lock_exclusive(&install_lock)
            .map_err(|error| EngineArtifactError::Lock(error.to_string()))?;
        if let Ok(installed) = self.store.require(spec) {
            return Ok(installed);
        }

        let mut archive_file = tempfile::Builder::new()
            .prefix("engine-")
            .suffix(".part")
            .tempfile_in(self.store.root())
            .map_err(storage)?;
        let mut verified = VerifyingWriter::new(archive_file.as_file_mut(), spec.size);
        self.source.download(spec, &mut verified)?;
        let (actual_size, actual_hash) = verified.finish();
        archive_file.as_file().sync_all().map_err(storage)?;
        if actual_size != spec.size {
            return Err(EngineArtifactError::SizeMismatch {
                expected: spec.size,
                actual: actual_size,
            });
        }
        if actual_hash != spec.sha256 {
            return Err(EngineArtifactError::HashMismatch {
                expected: spec.sha256.clone(),
                actual: actual_hash,
            });
        }

        let staging = tempfile::Builder::new()
            .prefix("engine-extract-")
            .tempdir_in(self.store.root())
            .map_err(storage)?;
        archive::extract(archive_file.path(), spec, staging.path())?;
        write_integrity_receipt(staging.path(), spec)?;
        if let Some(parent) = final_path.parent() {
            fs::create_dir_all(parent).map_err(storage)?;
        }
        let staging_path = staging.keep();
        let backup_path = if final_path.exists() {
            let reservation = tempfile::Builder::new()
                .prefix("engine-backup-")
                .tempdir_in(self.store.root())
                .map_err(storage)?;
            let path = reservation.path().to_path_buf();
            reservation.close().map_err(storage)?;
            fs::rename(&final_path, &path).map_err(storage)?;
            Some(path)
        } else {
            None
        };
        if let Err(error) = fs::rename(&staging_path, &final_path) {
            let _ignored = fs::remove_dir_all(&staging_path);
            if let Some(backup) = &backup_path {
                let _ignored = fs::rename(backup, &final_path);
            }
            return Err(storage(error));
        }
        if let Some(backup) = backup_path {
            fs::remove_dir_all(backup).map_err(storage)?;
        }
        self.store.require(spec)
    }
}

struct VerifyingWriter<W> {
    output: W,
    expected: u64,
    written: u64,
    hasher: Sha256,
}

impl<W> VerifyingWriter<W> {
    fn new(output: W, expected: u64) -> Self {
        Self {
            output,
            expected,
            written: 0,
            hasher: Sha256::new(),
        }
    }

    fn finish(self) -> (u64, String) {
        (self.written, format!("{:x}", self.hasher.finalize()))
    }
}

impl<W: Write> Write for VerifyingWriter<W> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let count = u64::try_from(buffer.len())
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        let next = self
            .written
            .checked_add(count)
            .ok_or_else(|| std::io::Error::other("archive byte count overflow"))?;
        if next > self.expected {
            return Err(std::io::Error::other("archive exceeds manifest size"));
        }
        self.output.write_all(buffer)?;
        self.hasher.update(buffer);
        self.written = next;
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.output.flush()
    }
}

fn storage(error: impl std::fmt::Display) -> EngineArtifactError {
    EngineArtifactError::Storage(error.to_string())
}
