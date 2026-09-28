mod download;
mod manifest;
mod store;

pub use download::ModelInstaller;
pub use manifest::{ModelId, ModelManifest, ModelSpec};
pub use store::{ModelLease, ModelRootResolver, ModelStore};

use thiserror::Error;

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ModelError {
    #[error("model {id} is missing; run `{install_command}`")]
    MissingModel {
        id: ModelId,
        install_command: String,
    },
    #[error("model manifest is invalid: {0}")]
    InvalidManifest(String),
    #[error("model directory is invalid: {0}")]
    InvalidModelRoot(String),
    #[error("model {id} download failed: {message}")]
    Download { id: ModelId, message: String },
    #[error("model {id} has size {actual}, expected {expected}")]
    SizeMismatch {
        id: ModelId,
        expected: u64,
        actual: u64,
    },
    #[error("model {id} SHA-256 is {actual}, expected {expected}")]
    HashMismatch {
        id: ModelId,
        expected: String,
        actual: String,
    },
    #[error("model {id} lock operation failed: {message}")]
    Lock { id: ModelId, message: String },
    #[error("model {id} is in use")]
    ModelInUse { id: ModelId },
    #[error("model storage operation failed: {0}")]
    Storage(String),
}
