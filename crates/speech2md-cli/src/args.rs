use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "speech2md",
    version,
    about = "Local Japanese meeting transcription"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Install, inspect, verify, or prune the managed whisper.cpp engine.
    Engine {
        #[command(subcommand)]
        command: EngineCommand,
    },
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
