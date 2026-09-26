mod manifest;
mod platform;

use thiserror::Error;

pub use manifest::{EngineManifest, EngineSpec};
pub use platform::Platform;

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
}
