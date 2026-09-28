use assert_cmd::Command;
use predicates::prelude::*;

fn yasumaro() -> Command {
    Command::new(env!("CARGO_BIN_EXE_yasumaro"))
}

#[test]
fn top_level_help_lists_user_commands() {
    yasumaro()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage: yasumaro"))
        .stdout(predicate::str::contains("transcribe"))
        .stdout(predicate::str::contains("model"))
        .stdout(predicate::str::contains("doctor"));
}

#[test]
fn transcribe_requires_input_and_positive_speaker_count() {
    yasumaro()
        .arg("transcribe")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("Usage:"));
    yasumaro()
        .args(["transcribe", "meeting.wav", "--speakers", "0"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("speakers"));
}

#[test]
fn prompt_text_and_file_conflict_at_argument_parsing() {
    yasumaro()
        .args([
            "transcribe",
            "meeting.wav",
            "--prompt",
            "Rust",
            "--prompt-file",
            "prompt.txt",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("cannot be used with"));
}

#[test]
fn model_list_and_doctor_are_local() {
    let root = tempfile::tempdir().expect("local roots");
    yasumaro()
        .args(["model", "list"])
        .env("YASUMARO_MODEL_DIR", root.path().join("models"))
        .env("HTTPS_PROXY", "http://127.0.0.1:9")
        .assert()
        .success()
        .stdout(predicate::str::contains("whisper-base: not installed"))
        .stdout(predicate::str::contains(
            "speaker-segmentation: not installed",
        ));
    yasumaro()
        .arg("doctor")
        .env("YASUMARO_MODEL_DIR", root.path().join("models"))
        .env("YASUMARO_ENGINE_DIR", root.path().join("engines"))
        .env("HTTPS_PROXY", "http://127.0.0.1:9")
        .assert()
        .success()
        .stdout(predicate::str::contains("platform:"))
        .stdout(predicate::str::contains("engine: not installed"));
}

#[test]
fn existing_output_fails_before_models_or_inference_and_preserves_bytes() {
    let root = tempfile::tempdir().expect("local roots");
    let input = root.path().join("meeting.wav");
    let output = root.path().join("meeting.md");
    std::fs::write(&input, b"not needed").expect("input placeholder");
    std::fs::write(&output, b"existing bytes").expect("existing output");
    yasumaro()
        .arg("transcribe")
        .arg(&input)
        .arg("--output")
        .arg(&output)
        .env("YASUMARO_MODEL_DIR", root.path().join("models"))
        .env("YASUMARO_ENGINE_DIR", root.path().join("engines"))
        .assert()
        .code(6)
        .stderr(predicate::str::contains("output already exists"));
    assert_eq!(
        std::fs::read(output).expect("preserved output"),
        b"existing bytes"
    );
}

#[test]
fn readme_command_examples_are_accepted_by_the_argument_parser() {
    let examples = [
        ["model", "install"].as_slice(),
        ["model", "remove", "whisper-small"].as_slice(),
        ["engine", "install"].as_slice(),
        ["transcribe", "meeting.m4a"].as_slice(),
        [
            "transcribe",
            "meeting.wav",
            "--speakers",
            "3",
            "--prompt",
            "Rust, Kubernetes, PostgreSQL",
        ]
        .as_slice(),
        [
            "transcribe",
            "meeting.mp3",
            "--whisper",
            "small",
            "--output",
            "minutes.md",
        ]
        .as_slice(),
        ["model", "list"].as_slice(),
        ["doctor"].as_slice(),
    ];

    for arguments in examples {
        yasumaro().args(arguments).arg("--help").assert().success();
    }
}
