use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use speech2md_core::TranscribedSegment;
use thiserror::Error;

use super::{parse_whisper_json, process, required_whisper_temp_bytes, write_whisper_wav};
use crate::engine_artifact::EngineLease;

const CANCELLATION_GRACE: Duration = Duration::from_secs(5);

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
    /// The child process could not be created, monitored, or reaped.
    #[error("could not run whisper process: {0}")]
    Spawn(String),
    /// `whisper-cli` returned a non-success status. Child output is deliberately omitted.
    #[error(
        "whisper process exit {code:?}; stdout tail {stdout_bytes} bytes (truncated: {stdout_truncated}), stderr tail {stderr_bytes} bytes (truncated: {stderr_truncated})"
    )]
    ProcessExit {
        /// Platform exit code, when available.
        code: Option<i32>,
        /// Retained stdout byte count.
        stdout_bytes: usize,
        /// Whether earlier stdout bytes were discarded.
        stdout_truncated: bool,
        /// Retained stderr byte count.
        stderr_bytes: usize,
        /// Whether earlier stderr bytes were discarded.
        stderr_truncated: bool,
    },
    /// The process succeeded without producing the requested JSON file.
    #[error("whisper process missing output")]
    MissingOutput,
    /// Temporary resources or diagnostic reader threads could not be cleaned up.
    #[error("whisper cleanup failed: {0}")]
    Cleanup(String),
    /// The caller requested cancellation.
    #[error("whisper transcription cancelled")]
    Cancelled,
    /// Engine configuration is invalid before execution starts.
    #[error("invalid whisper configuration: {0}")]
    Configuration(String),
}

/// Values needed for one transcription request.
#[derive(Clone, Debug)]
pub struct TranscriptionRequest {
    /// Optional UTF-8 vocabulary/context hint. It is written only to a private file.
    pub prompt: Option<String>,
    /// CPU worker count passed to `whisper-cli`.
    pub threads: usize,
    /// Cooperative cancellation flag shared with the caller.
    pub cancelled: Arc<AtomicBool>,
}

/// Engine-independent transcription boundary used by the runtime pipeline.
pub trait Transcriber {
    /// Transcribes normalized 16 kHz mono PCM.
    ///
    /// # Errors
    ///
    /// Returns a classified engine, process, JSON, staging, or cancellation error.
    fn transcribe(
        &self,
        samples: &[f32],
        request: &TranscriptionRequest,
    ) -> Result<Vec<TranscribedSegment>, EngineError>;
}

enum SpaceProbe {
    System,
    #[cfg(feature = "test-support")]
    Fixed(u64),
}

impl SpaceProbe {
    fn available(&self, path: &Path) -> Result<u64, EngineError> {
        match self {
            Self::System => fs4::available_space(path)
                .map_err(|error| EngineError::Configuration(error.to_string())),
            #[cfg(feature = "test-support")]
            Self::Fixed(bytes) => Ok(*bytes),
        }
    }
}

/// Runs a managed, locked `whisper-cli` child process.
pub struct WhisperProcessTranscriber {
    engine: EngineLease,
    model_path: PathBuf,
    temp_root: PathBuf,
    cancellation_grace: Duration,
    space_probe: SpaceProbe,
}

impl WhisperProcessTranscriber {
    /// Creates a process transcriber without downloading an engine or model.
    ///
    /// # Errors
    ///
    /// Returns an error unless the model is a file and the temporary root is a directory.
    pub fn new(
        engine: EngineLease,
        model_path: PathBuf,
        temp_root: PathBuf,
    ) -> Result<Self, EngineError> {
        Self::build(
            engine,
            model_path,
            temp_root,
            CANCELLATION_GRACE,
            SpaceProbe::System,
        )
    }

    /// Creates a deterministic process transcriber for contract tests.
    ///
    /// # Errors
    ///
    /// Has the same path validation as [`Self::new`].
    #[cfg(feature = "test-support")]
    pub fn new_for_test(
        engine: EngineLease,
        model_path: PathBuf,
        temp_root: PathBuf,
        available_temp_bytes: u64,
        cancellation_grace: Duration,
    ) -> Result<Self, EngineError> {
        Self::build(
            engine,
            model_path,
            temp_root,
            cancellation_grace,
            SpaceProbe::Fixed(available_temp_bytes),
        )
    }

    fn build(
        engine: EngineLease,
        model_path: PathBuf,
        temp_root: PathBuf,
        cancellation_grace: Duration,
        space_probe: SpaceProbe,
    ) -> Result<Self, EngineError> {
        if !model_path.is_file() {
            return Err(EngineError::Configuration("model file is missing".into()));
        }
        if !temp_root.is_dir() {
            return Err(EngineError::Configuration(
                "temporary root is missing".into(),
            ));
        }
        Ok(Self {
            engine,
            model_path,
            temp_root,
            cancellation_grace,
            space_probe,
        })
    }

    fn transcribe_inner(
        &self,
        samples: &[f32],
        request: &TranscriptionRequest,
        directory: &Path,
    ) -> Result<Vec<TranscribedSegment>, EngineError> {
        let wav_path = directory.join("input.wav");
        let prompt_path = directory.join("prompt.txt");
        let output_prefix = directory.join("output");
        let output_json = directory.join("output.json");
        write_whisper_wav(samples, &wav_path)?;
        fs::write(&prompt_path, request.prompt.as_deref().unwrap_or_default())
            .map_err(|error| EngineError::Wav(error.to_string()))?;

        let mut command = Command::new(self.engine.executable());
        command
            .arg("--language")
            .arg("ja")
            .arg("--output-json")
            .arg("--threads")
            .arg(request.threads.to_string())
            .arg("--model")
            .arg(&self.model_path)
            .arg("--file")
            .arg(&wav_path)
            .arg("--output-file")
            .arg(&output_prefix)
            .arg("--prompt-file")
            .arg(&prompt_path);
        let outcome = process::run(&mut command, &request.cancelled, self.cancellation_grace)?;
        if !outcome.status.success() {
            return Err(EngineError::ProcessExit {
                code: outcome.status.code(),
                stdout_bytes: outcome.stdout.len(),
                stdout_truncated: outcome.stdout.truncated(),
                stderr_bytes: outcome.stderr.len(),
                stderr_truncated: outcome.stderr.truncated(),
            });
        }
        let bytes = fs::read(&output_json).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                EngineError::MissingOutput
            } else {
                EngineError::Cleanup(error.to_string())
            }
        })?;
        parse_whisper_json(&bytes)
    }
}

impl Transcriber for WhisperProcessTranscriber {
    fn transcribe(
        &self,
        samples: &[f32],
        request: &TranscriptionRequest,
    ) -> Result<Vec<TranscribedSegment>, EngineError> {
        if request.cancelled.load(Ordering::Acquire) {
            return Err(EngineError::Cancelled);
        }
        if request.threads == 0 {
            return Err(EngineError::Configuration(
                "thread count must be positive".into(),
            ));
        }
        let required = required_whisper_temp_bytes(samples.len())?;
        let available = self.space_probe.available(&self.temp_root)?;
        if available < required {
            return Err(EngineError::InsufficientTempSpace {
                required,
                available,
            });
        }
        let directory = tempfile::Builder::new()
            .prefix("speech2md-whisper-")
            .tempdir_in(&self.temp_root)
            .map_err(|error| EngineError::Wav(error.to_string()))?;
        let started = Instant::now();
        tracing::info!(
            target: "speech2md_runtime::whisper",
            sample_count = samples.len(),
            threads = request.threads,
            engine_version = self.engine.version(),
            "whisper transcription started"
        );
        let result = self.transcribe_inner(samples, request, directory.path());
        let cleanup = directory
            .close()
            .map_err(|error| EngineError::Cleanup(error.to_string()));
        cleanup?;
        if let Ok(segments) = &result {
            tracing::info!(
                target: "speech2md_runtime::whisper",
                elapsed_ms = started.elapsed().as_millis(),
                segment_count = segments.len(),
                "whisper transcription completed"
            );
        }
        result
    }
}
