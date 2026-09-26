mod args;
mod commands;
mod logging;

use std::io::{self, Write as _};
use std::process::ExitCode;

use clap::Parser as _;

use args::Cli;

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let code = error.exit_code();
            let _ignored = error.print();
            return ExitCode::from(u8::try_from(code).unwrap_or(2));
        }
    };
    if let Err(error) = logging::init() {
        let _ignored = writeln!(
            io::stderr().lock(),
            "error: invalid logging configuration: {error}"
        );
        let _ignored = writeln!(io::stderr().lock(), "help: correct or remove RUST_LOG");
        return ExitCode::from(3);
    }
    tracing::debug!(target: "speech2md_cli", "logging initialized");
    commands::execute(&cli)
}
