mod archive;
mod install;
mod manifest;
mod platform;
mod store;

use thiserror::Error;

pub use install::{EngineArchiveSource, EngineInstaller, HttpEngineArchiveSource};
pub use manifest::{EngineManifest, EngineSpec};
pub use platform::Platform;
pub use store::{EngineLease, EngineRootResolver, EngineStore, InstalledEngine, PruneReport};

/// Failures while resolving, validating, or managing a Whisper engine artifact.
#[derive(Debug, Error)]
pub enum EngineArtifactError {
    /// The manifest cannot be parsed or violates an invariant.
    #[error("invalid engine manifest: {0}")]
    InvalidManifest(String),
    /// No engine build is supported for the requested target.
    #[error("unsupported engine platform: {os}-{arch}")]
    UnsupportedPlatform {
        /// Operating-system identifier reported by Rust.
        os: String,
        /// Architecture identifier reported by Rust.
        arch: String,
    },
    /// The manifest does not yet publish an artifact for a supported platform.
    #[error("no engine artifact is published for {0}")]
    MissingArtifact(Platform),
    /// The configured engine storage root is invalid.
    #[error("invalid engine directory: {0}")]
    InvalidRoot(String),
    /// A network source could not provide the requested archive.
    #[error("engine download failed: {0}")]
    Download(String),
    /// A local storage operation failed.
    #[error("engine storage operation failed: {0}")]
    Storage(String),
    /// The downloaded byte count differs from the signed manifest.
    #[error("engine archive has size {actual}, expected {expected}")]
    SizeMismatch {
        /// Manifest size.
        expected: u64,
        /// Downloaded size.
        actual: u64,
    },
    /// The downloaded digest differs from the signed manifest.
    #[error("engine archive SHA-256 is {actual}, expected {expected}")]
    HashMismatch {
        /// Manifest digest.
        expected: String,
        /// Downloaded digest.
        actual: String,
    },
    /// The archive format or one of its entries is unsafe.
    #[error("invalid engine archive: {0}")]
    InvalidArchive(String),
    /// The expected installed executable is absent.
    #[error("engine {version} for {platform} is not installed; run `speech2md engine install`")]
    MissingEngine {
        /// Engine version.
        version: String,
        /// Distribution platform.
        platform: Platform,
    },
    /// A version lock could not be acquired or inspected.
    #[error("engine lock operation failed: {0}")]
    Lock(String),
}
