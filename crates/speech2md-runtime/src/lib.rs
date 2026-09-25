//! I/O adapters and application orchestration.

mod audio;
mod error;

pub use audio::{DecodedPcm, decode_to_pcm};
pub use error::RuntimeError;
