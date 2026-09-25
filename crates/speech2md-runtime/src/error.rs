use thiserror::Error;

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("audio input or PCM storage failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("the input does not contain a supported audio stream")]
    NoAudioStream,
    #[error("audio probing failed: {0}")]
    Probe(String),
    #[error("audio decoding failed: {0}")]
    Decode(String),
    #[error("audio resampling failed: {0}")]
    Resample(String),
    #[error("decoded PCM storage is invalid")]
    InvalidPcm,
}
