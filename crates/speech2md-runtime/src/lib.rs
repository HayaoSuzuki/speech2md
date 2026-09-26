//! I/O adapters and application orchestration.

mod audio;
pub mod engine;
pub mod engine_artifact;
mod error;
mod model;
mod output;
mod pipeline;

pub use audio::{DecodedPcm, decode_to_pcm};
pub use error::RuntimeError;
pub use model::{
    ModelError, ModelId, ModelInstaller, ModelManifest, ModelRootResolver, ModelSpec, ModelStore,
};
pub use pipeline::{RunReport, RuntimeServices, TranscribeOptions, run_transcription};
