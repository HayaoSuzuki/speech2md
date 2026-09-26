use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    /// `whisper-cli` did not emit its documented JSON shape.
    #[error("invalid whisper JSON: {0}")]
    InvalidJson(String),
    /// A transcription segment has invalid or unordered timestamps.
    #[error("invalid whisper segment: {0}")]
    InvalidSegment(String),
    /// A PCM sample is NaN or infinite.
    #[error("invalid PCM sample at index {index}")]
    InvalidSample {
        /// Zero-based sample index.
        index: usize,
    },
    /// Temporary-space arithmetic overflowed or the probed space is insufficient.
    #[error("temporary space is insufficient: need {required} bytes, have {available} bytes")]
    InsufficientTempSpace {
        /// Calculated additional bytes required.
        required: u64,
        /// Available bytes, or zero when the calculation itself overflowed.
        available: u64,
    },
    /// WAV creation or finalization failed.
    #[error("could not stage whisper WAV: {0}")]
    Wav(String),
}
