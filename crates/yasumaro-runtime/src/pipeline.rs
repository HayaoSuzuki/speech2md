use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use yasumaro_core::{
    AssignmentConfig, NormalizationConfig, TranscriptDocument, assign_speakers,
    normalize_utterances,
};
use yasumaro_formats::render_commonmark;

use crate::engine::{
    DiarizationRequest, Diarizer, EngineError, Transcriber, TranscriptionRequest,
    WhisperProcessTranscriber,
};
use crate::engine_artifact::{EngineSpec, EngineStore};
use crate::output::{atomic_write, validate_output_target};
use crate::{ModelId, ModelStore, RuntimeError, decode_to_pcm};

const JOB_PREFIX: &str = "yasumaro-job-";
const OWNER_MARKER: &str = ".yasumaro-owner";
const OWNER_MARKER_CONTENT: &[u8] = b"yasumaro\n";
const STALE_AGE: Duration = Duration::from_hours(24);

/// Local-only services used by one transcription run.
pub struct RuntimeServices<'a> {
    engine_store: &'a EngineStore,
    engine_spec: &'a EngineSpec,
    model_store: &'a ModelStore,
    diarizer: &'a dyn Diarizer,
    temp_root: &'a Path,
}

impl<'a> RuntimeServices<'a> {
    #[must_use]
    pub const fn new(
        engine_store: &'a EngineStore,
        engine_spec: &'a EngineSpec,
        model_store: &'a ModelStore,
        diarizer: &'a dyn Diarizer,
        temp_root: &'a Path,
    ) -> Self {
        Self {
            engine_store,
            engine_spec,
            model_store,
            diarizer,
            temp_root,
        }
    }
}

/// User-selected values for a single offline transcription.
pub struct TranscribeOptions {
    pub input: PathBuf,
    pub output: PathBuf,
    pub title: String,
    pub whisper_model: ModelId,
    pub speakers: Option<u32>,
    pub prompt: Option<String>,
    pub threads: usize,
    pub force: bool,
    pub cancelled: Arc<AtomicBool>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RunReport {
    pub segment_count: usize,
    pub utterance_count: usize,
}

/// Runs the complete local pipeline and commits output only after every stage succeeds.
///
/// # Errors
///
/// Returns a classified input, installation, inference, cancellation, or output error.
pub fn run_transcription(
    services: &RuntimeServices<'_>,
    options: &TranscribeOptions,
) -> Result<RunReport, RuntimeError> {
    check_cancelled(options)?;
    let output_exists = validate_output_target(&options.output)?;
    if output_exists && !options.force {
        return Err(RuntimeError::OutputExists);
    }
    cleanup_stale_jobs(services.temp_root, SystemTime::now())?;
    let job = tempfile::Builder::new()
        .prefix(JOB_PREFIX)
        .tempdir_in(services.temp_root)?;
    fs::write(job.path().join(OWNER_MARKER), OWNER_MARKER_CONTENT)?;
    let pcm = decode_to_pcm(&options.input, job.path())?;
    check_cancelled(options)?;

    let installed = services.engine_store.require(services.engine_spec)?;
    let lease = installed.acquire()?;
    let whisper_model = services.model_store.require(options.whisper_model)?;
    services.model_store.require(ModelId::SpeakerSegmentation)?;
    services.model_store.require(ModelId::SpeakerEmbedding)?;
    let transcriber =
        WhisperProcessTranscriber::new(lease, whisper_model, job.path().to_path_buf())
            .map_err(|error| map_transcription_error(&error))?;
    let segments = transcriber
        .transcribe(
            pcm.samples(),
            &TranscriptionRequest {
                prompt: options.prompt.clone(),
                threads: options.threads,
                cancelled: Arc::clone(&options.cancelled),
            },
        )
        .map_err(|error| map_transcription_error(&error))?;
    if segments.is_empty() {
        return Err(RuntimeError::EmptyTranscript);
    }
    check_cancelled(options)?;

    let turns = services
        .diarizer
        .diarize(
            pcm.samples(),
            &DiarizationRequest {
                num_speakers: options.speakers,
            },
        )
        .map_err(|error| map_diarization_error(&error))?;
    check_cancelled(options)?;
    let assigned = assign_speakers(&segments, &turns, &AssignmentConfig::default());
    let utterances = normalize_utterances(
        assigned,
        &NormalizationConfig {
            max_gap_ms: 200,
            max_chars: 1_000,
        },
    );
    let document = TranscriptDocument {
        title: options.title.clone(),
        utterances,
    };
    let rendered = render_commonmark(&document);
    check_cancelled(options)?;
    atomic_write(&options.output, rendered.as_bytes(), options.force)?;

    Ok(RunReport {
        segment_count: segments.len(),
        utterance_count: document.utterances.len(),
    })
}

fn cleanup_stale_jobs(root: &Path, now: SystemTime) -> Result<(), RuntimeError> {
    let canonical_root = root.canonicalize()?;
    let cutoff = now.checked_sub(STALE_AGE).unwrap_or(SystemTime::UNIX_EPOCH);
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !name.starts_with(JOB_PREFIX) {
            continue;
        }
        let metadata = fs::symlink_metadata(entry.path())?;
        if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.modified()? > cutoff {
            continue;
        }
        let marker = entry.path().join(OWNER_MARKER);
        if !matches!(fs::read(&marker), Ok(content) if content == OWNER_MARKER_CONTENT) {
            continue;
        }
        let candidate = entry.path().canonicalize()?;
        if candidate == canonical_root || !candidate.starts_with(&canonical_root) {
            continue;
        }
        fs::remove_dir_all(candidate)?;
    }
    Ok(())
}

fn check_cancelled(options: &TranscribeOptions) -> Result<(), RuntimeError> {
    if options.cancelled.load(Ordering::Acquire) {
        Err(RuntimeError::Cancelled)
    } else {
        Ok(())
    }
}

fn map_transcription_error(error: &EngineError) -> RuntimeError {
    if matches!(error, EngineError::Cancelled) {
        RuntimeError::Cancelled
    } else {
        RuntimeError::Transcription(error.to_string())
    }
}

fn map_diarization_error(error: &EngineError) -> RuntimeError {
    if matches!(error, EngineError::Cancelled) {
        RuntimeError::Cancelled
    } else {
        RuntimeError::Diarization(error.to_string())
    }
}
