use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct Control {
    mode: String,
    capture: PathBuf,
}

#[derive(Serialize)]
struct Capture {
    args: Vec<String>,
    environment: BTreeMap<String, String>,
    prompt: String,
}

fn value_after(args: &[String], flag: &str) -> PathBuf {
    let index = args
        .iter()
        .position(|argument| argument == flag)
        .expect("fake whisper required flag");
    PathBuf::from(args.get(index + 1).expect("fake whisper flag value"))
}

fn write_capture(control: &Control, args: &[String]) {
    let prompt_path = value_after(args, "--prompt-file");
    let capture = Capture {
        args: args.to_vec(),
        environment: env::vars().collect(),
        prompt: fs::read_to_string(prompt_path).expect("read prompt file"),
    };
    fs::write(
        &control.capture,
        serde_json::to_vec(&capture).expect("serialize capture"),
    )
    .expect("write capture");
}

fn write_success(output_prefix: &Path) {
    let path = PathBuf::from(format!("{}.json", output_prefix.display()));
    fs::write(
        path,
        r#"{"transcription":[{"offsets":{"from":0,"to":250},"text":" テスト"}]}"#.as_bytes(),
    )
    .expect("write fake JSON");
}

fn write_empty(output_prefix: &Path) {
    let path = PathBuf::from(format!("{}.json", output_prefix.display()));
    fs::write(path, br#"{"transcription":[]}"#).expect("write empty fake JSON");
}

fn main() -> ExitCode {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let model_path = value_after(&args, "--model");
    let output_prefix = value_after(&args, "--output-file");
    let control: Control = serde_json::from_slice(&fs::read(model_path).expect("read control"))
        .expect("parse control");
    write_capture(&control, &args);

    match control.mode.as_str() {
        "success" => write_success(&output_prefix),
        "empty" => write_empty(&output_prefix),
        "missing" => {}
        "invalid" => {
            let path = PathBuf::from(format!("{}.json", output_prefix.display()));
            fs::write(path, b"not json").expect("write invalid JSON");
        }
        "nonzero" => {
            let mut stdout = io::stdout().lock();
            let mut stderr = io::stderr().lock();
            stdout.write_all(&vec![b'o'; 70_000]).expect("stdout");
            stderr.write_all(&vec![b'e'; 70_000]).expect("stderr");
            return ExitCode::from(9);
        }
        "hang" => loop {
            std::thread::sleep(Duration::from_secs(1));
        },
        "ignore-terminate" => {
            ctrlc::set_handler(|| {}).expect("install no-op signal handler");
            loop {
                std::thread::sleep(Duration::from_secs(1));
            }
        }
        mode => panic!("unknown fake mode {mode}"),
    }
    ExitCode::SUCCESS
}
