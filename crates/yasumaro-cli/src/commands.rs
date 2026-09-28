use std::fmt::Write as _;
use std::fs;
use std::io::{self, Write as _};
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use thiserror::Error;
use yasumaro_runtime::engine::SherpaDiarizer;
use yasumaro_runtime::engine_artifact::{
    EngineArtifactError, EngineInstaller, EngineManifest, EngineRootResolver, EngineStore,
    HttpEngineArchiveSource, Platform,
};
use yasumaro_runtime::{
    ModelError, ModelId, ModelInstaller, ModelManifest, ModelRootResolver, ModelStore,
    RuntimeError, RuntimeServices, TranscribeOptions, run_transcription,
};

use crate::args::{
    Cli, Command, EngineCommand, ModelChoice, ModelCommand, TranscribeArgs, WhisperChoice,
};

#[derive(Debug, Error)]
enum AppError {
    #[error(transparent)]
    Engine(#[from] EngineArtifactError),
    #[error(transparent)]
    Model(#[from] ModelError),
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
    #[error("configuration is invalid: {0}")]
    Configuration(String),
}

pub fn execute(cli: &Cli) -> ExitCode {
    match execute_inner(cli) {
        Ok(message) => {
            if !message.is_empty() {
                let _ignored = writeln!(io::stdout().lock(), "{message}");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            let _ignored = writeln!(io::stderr().lock(), "error: {error}");
            if let Some(help) = help(&error) {
                let _ignored = writeln!(io::stderr().lock(), "help: {help}");
            }
            ExitCode::from(classify(&error))
        }
    }
}

fn execute_inner(cli: &Cli) -> Result<String, AppError> {
    match &cli.command {
        Command::Engine { command } => execute_engine(*command),
        Command::Model { command } => execute_model(command),
        Command::Transcribe(arguments) => execute_transcribe(arguments),
        Command::Doctor => doctor(),
    }
}

fn execute_engine(command: EngineCommand) -> Result<String, AppError> {
    let manifest = EngineManifest::embedded()?;
    let platform = Platform::current()?;
    let store = EngineStore::new(EngineRootResolver::resolve()?);
    match command {
        EngineCommand::Install => {
            let spec = manifest.select(platform)?;
            EngineInstaller::new(HttpEngineArchiveSource::new()?, store).install(spec)?;
            Ok(format!(
                "Installed whisper engine {} for {}.",
                spec.version, spec.platform
            ))
        }
        EngineCommand::List => {
            let mut output = String::new();
            for spec in manifest.specs() {
                let status = if store.require(spec).is_ok() {
                    "installed"
                } else {
                    "not installed"
                };
                writeln!(output, "{} {}: {status}", spec.platform, spec.version)
                    .map_err(|error| AppError::Configuration(error.to_string()))?;
            }
            Ok(output.trim_end().into())
        }
        EngineCommand::Verify => {
            let spec = manifest.select(platform)?;
            store.require(spec)?;
            Ok(format!(
                "Verified whisper engine {} for {}.",
                spec.version, spec.platform
            ))
        }
        EngineCommand::Prune => {
            let spec = manifest.select(platform)?;
            let report = store.prune(spec)?;
            Ok(format!(
                "Pruned {} engine installation(s); kept {} locked installation(s).",
                report.removed, report.skipped_locked
            ))
        }
    }
}

fn execute_model(command: &ModelCommand) -> Result<String, AppError> {
    let manifest = ModelManifest::embedded()?;
    let store = ModelStore::new(ModelRootResolver::resolve()?);
    match command {
        ModelCommand::List => {
            let mut output = String::new();
            for spec in manifest.specs() {
                let status = if store.require(spec.id).is_ok() {
                    "installed"
                } else {
                    "not installed"
                };
                writeln!(output, "{}: {status} ({} bytes)", spec.id, spec.size)
                    .map_err(|error| AppError::Configuration(error.to_string()))?;
            }
            Ok(output.trim_end().into())
        }
        ModelCommand::Install { models } => {
            let ids = if models.is_empty() {
                vec![
                    ModelId::WhisperBase,
                    ModelId::SpeakerSegmentation,
                    ModelId::SpeakerEmbedding,
                ]
            } else {
                models.iter().copied().map(ModelId::from).collect()
            };
            let cancelled = cancellation_flag()?;
            ModelInstaller::new(manifest, store)?
                .install_with_cancellation(&ids, cancelled.as_ref())?;
            Ok(format!("Installed {} model(s).", ids.len()))
        }
        ModelCommand::Remove { models } => {
            let mut ids = models
                .iter()
                .copied()
                .map(ModelId::from)
                .collect::<Vec<_>>();
            ids.sort_unstable();
            ids.dedup();
            for &id in &ids {
                store.remove(id)?;
            }
            Ok(format!("Removed {} model(s).", ids.len()))
        }
    }
}

fn execute_transcribe(arguments: &TranscribeArgs) -> Result<String, AppError> {
    let output = arguments
        .output
        .clone()
        .unwrap_or_else(|| arguments.input.with_extension("md"));
    if output.exists() && !arguments.force {
        return Err(RuntimeError::OutputExists.into());
    }
    let engine_manifest = EngineManifest::embedded()?;
    let platform = Platform::current()?;
    let engine_spec = engine_manifest.select(platform)?;
    let engine_store = EngineStore::new(EngineRootResolver::resolve()?);
    let model_manifest = ModelManifest::embedded()?;
    let model_store = ModelStore::new(ModelRootResolver::resolve()?);
    let threads = std::thread::available_parallelism().map_or(1, usize::from);
    let segmentation = model_store.acquire(model_manifest.spec(ModelId::SpeakerSegmentation)?)?;
    let embedding = model_store.acquire(model_manifest.spec(ModelId::SpeakerEmbedding)?)?;
    let diarizer = SherpaDiarizer::new(segmentation, embedding, threads)
        .map_err(|error| RuntimeError::Diarization(error.to_string()))?;
    let cancelled = cancellation_flag()?;
    let title = arguments
        .input
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("transcript")
        .to_owned();
    let prompt = arguments.prompt_file.as_ref().map_or_else(
        || Ok(arguments.prompt.clone()),
        |path| {
            fs::read_to_string(path)
                .map(Some)
                .map_err(|error| AppError::Configuration(format!("prompt file: {error}")))
        },
    )?;
    let temp_root = std::env::temp_dir();
    let services = RuntimeServices::new(
        &engine_store,
        engine_spec,
        &model_manifest,
        &model_store,
        &diarizer,
        &temp_root,
    );
    let report = run_transcription(
        &services,
        &TranscribeOptions {
            input: arguments.input.clone(),
            output: output.clone(),
            title,
            whisper_model: ModelId::from(arguments.whisper),
            speakers: arguments.speakers.map(std::num::NonZeroU32::get),
            prompt,
            threads,
            force: arguments.force,
            cancelled,
        },
    )?;
    Ok(format!(
        "Wrote {} utterance(s) from {} segment(s) to {}.",
        report.utterance_count,
        report.segment_count,
        display_name(&output)
    ))
}

fn cancellation_flag() -> Result<Arc<AtomicBool>, AppError> {
    let cancelled = Arc::new(AtomicBool::new(false));
    let trigger = Arc::clone(&cancelled);
    ctrlc::set_handler(move || trigger.store(true, Ordering::Release))
        .map_err(|error| AppError::Configuration(error.to_string()))?;
    Ok(cancelled)
}

fn doctor() -> Result<String, AppError> {
    let platform = Platform::current()?;
    let engines = EngineManifest::embedded()?;
    let engine_store = EngineStore::new(EngineRootResolver::resolve()?);
    let engine = engines
        .select(platform)
        .is_ok_and(|spec| engine_store.require(spec).is_ok());
    let models = ModelManifest::embedded()?;
    let model_store = ModelStore::new(ModelRootResolver::resolve()?);
    let installed = models
        .specs()
        .iter()
        .filter(|spec| model_store.require(spec.id).is_ok())
        .count();
    tempfile::Builder::new()
        .prefix("yasumaro-doctor-")
        .tempdir_in(std::env::temp_dir())
        .and_then(tempfile::TempDir::close)
        .map_err(|error| AppError::Configuration(format!("temporary directory: {error}")))?;
    Ok(format!(
        "platform: {platform}\nlogical CPUs: {}\nengine: {}\nmodels: {installed}/{} installed\ntemporary directory: available",
        std::thread::available_parallelism().map_or(1, usize::from),
        if engine { "installed" } else { "not installed" },
        models.specs().len()
    ))
}

impl From<ModelChoice> for ModelId {
    fn from(value: ModelChoice) -> Self {
        match value {
            ModelChoice::WhisperBase => Self::WhisperBase,
            ModelChoice::WhisperSmall => Self::WhisperSmall,
            ModelChoice::WhisperMedium => Self::WhisperMedium,
            ModelChoice::WhisperLargeV3 => Self::WhisperLargeV3,
            ModelChoice::WhisperLargeV3Turbo => Self::WhisperLargeV3Turbo,
            ModelChoice::SpeakerSegmentation => Self::SpeakerSegmentation,
            ModelChoice::SpeakerEmbedding => Self::SpeakerEmbedding,
        }
    }
}
impl From<WhisperChoice> for ModelId {
    fn from(value: WhisperChoice) -> Self {
        match value {
            WhisperChoice::Base => Self::WhisperBase,
            WhisperChoice::Small => Self::WhisperSmall,
            WhisperChoice::Medium => Self::WhisperMedium,
            WhisperChoice::LargeV3 => Self::WhisperLargeV3,
            WhisperChoice::LargeV3Turbo => Self::WhisperLargeV3Turbo,
        }
    }
}

const fn classify(error: &AppError) -> u8 {
    match error {
        AppError::Model(ModelError::Cancelled { .. })
        | AppError::Runtime(RuntimeError::Cancelled) => 130,
        AppError::Engine(_) | AppError::Model(_) => 4,
        AppError::Runtime(RuntimeError::OutputExists | RuntimeError::Output(_)) => 6,
        AppError::Runtime(
            RuntimeError::Transcription(_)
            | RuntimeError::Diarization(_)
            | RuntimeError::EmptyTranscript,
        ) => 5,
        AppError::Configuration(_) | AppError::Runtime(_) => 3,
    }
}
fn help(error: &AppError) -> Option<String> {
    match error {
        AppError::Engine(
            EngineArtifactError::MissingEngine { .. } | EngineArtifactError::CorruptEngine { .. },
        ) => Some("run `yasumaro engine install`".into()),
        AppError::Engine(EngineArtifactError::Download(_)) => {
            Some("check the network connection and retry `yasumaro engine install`".into())
        }
        AppError::Model(ModelError::MissingModel { .. }) => {
            Some("run the model install command shown above".into())
        }
        AppError::Model(ModelError::ModelInUse { id }) => Some(format!(
            "retry `yasumaro model remove {id}` after transcription or installation completes"
        )),
        AppError::Model(ModelError::CleanupFailed { id, .. }) => Some(format!(
            "run `yasumaro model remove {id}` to clean the incomplete model"
        )),
        AppError::Model(ModelError::Cancelled { id }) => {
            Some(format!("retry `yasumaro model install {id}`"))
        }
        AppError::Runtime(RuntimeError::OutputExists) => {
            Some("pass --force only when replacement is intended".into())
        }
        _ => None,
    }
}
fn display_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{AppError, Cli, Command, ModelError, ModelId, classify, help};

    #[test]
    fn transcription_choices_resolve_to_the_selected_model() {
        for (choice, expected) in [
            ("base", "whisper-base"),
            ("small", "whisper-small"),
            ("medium", "whisper-medium"),
            ("large-v3", "whisper-large-v3"),
            ("large-v3-turbo", "whisper-large-v3-turbo"),
        ] {
            let cli =
                Cli::try_parse_from(["yasumaro", "transcribe", "meeting.wav", "--whisper", choice])
                    .expect("supported transcription model");
            let Command::Transcribe(arguments) = cli.command else {
                panic!("expected transcription command");
            };
            assert_eq!(ModelId::from(arguments.whisper).to_string(), expected);
        }
    }

    #[test]
    fn transcription_defaults_to_base() {
        let cli = Cli::try_parse_from(["yasumaro", "transcribe", "meeting.wav"])
            .expect("default transcription arguments");
        let Command::Transcribe(arguments) = cli.command else {
            panic!("expected transcription command");
        };
        assert_eq!(ModelId::from(arguments.whisper), ModelId::WhisperBase);
    }

    #[test]
    fn model_cancellation_uses_exit_code_130() {
        let error = AppError::Model(ModelError::Cancelled {
            id: ModelId::WhisperSmall,
        });

        assert_eq!(classify(&error), 130);
        assert!(
            help(&error)
                .is_some_and(|text| { text.contains("yasumaro model install whisper-small") })
        );
    }

    #[test]
    fn cleanup_failure_reports_model_remove_help() {
        let error = AppError::Model(ModelError::CleanupFailed {
            id: ModelId::WhisperMedium,
            source: Box::new(ModelError::Cancelled {
                id: ModelId::WhisperMedium,
            }),
            cleanup: "remove partial model: permission denied".into(),
        });

        assert_eq!(classify(&error), 4);
        assert!(
            help(&error)
                .is_some_and(|text| { text.contains("yasumaro model remove whisper-medium") })
        );
    }
}
