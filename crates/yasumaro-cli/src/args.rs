use std::num::NonZeroU32;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    name = "yasumaro",
    version,
    about = "Local Japanese meeting transcription"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Transcribe a local audio file to `CommonMark`.
    Transcribe(TranscribeArgs),
    /// Install, inspect, verify, or prune the managed whisper.cpp engine.
    Engine {
        #[command(subcommand)]
        command: EngineCommand,
    },
    /// Install or inspect local inference models.
    Model {
        #[command(subcommand)]
        command: ModelCommand,
    },
    /// Inspect the local platform, engine, models, and temporary directory.
    Doctor,
}

#[derive(Debug, Args)]
pub struct TranscribeArgs {
    pub input: PathBuf,
    #[arg(long)]
    pub output: Option<PathBuf>,
    #[arg(long, value_enum, default_value_t = WhisperChoice::Base)]
    pub whisper: WhisperChoice,
    #[arg(long)]
    pub speakers: Option<NonZeroU32>,
    #[arg(long, conflicts_with = "prompt_file")]
    pub prompt: Option<String>,
    #[arg(long, conflicts_with = "prompt")]
    pub prompt_file: Option<PathBuf>,
    #[arg(long)]
    pub force: bool,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum WhisperChoice {
    Base,
    Small,
    Medium,
    LargeV3,
    LargeV3Turbo,
}

#[derive(Clone, Copy, Debug, Subcommand)]
pub enum EngineCommand {
    /// Download and verify the engine published for this platform.
    Install,
    /// List published engine artifacts and local installation state.
    List,
    /// Verify that this platform's engine is installed locally.
    Verify,
    /// Remove unlocked engine versions other than the published version.
    Prune,
}

#[derive(Clone, Debug, Subcommand)]
pub enum ModelCommand {
    /// Download and verify the default model set, or selected models.
    Install {
        #[arg(value_enum)]
        models: Vec<ModelChoice>,
    },
    /// List every known model and its local installation state.
    List,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum ModelChoice {
    WhisperBase,
    WhisperSmall,
    WhisperMedium,
    WhisperLargeV3,
    WhisperLargeV3Turbo,
    SpeakerSegmentation,
    SpeakerEmbedding,
}
