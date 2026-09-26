use assert_cmd::Command;
use predicates::prelude::*;

fn speech2md() -> Command {
    Command::new(env!("CARGO_BIN_EXE_speech2md"))
}

#[test]
fn top_level_help_lists_user_commands() {
    speech2md()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("transcribe"))
        .stdout(predicate::str::contains("model"))
        .stdout(predicate::str::contains("doctor"));
}

#[test]
fn transcribe_requires_input_and_positive_speaker_count() {
    speech2md()
        .arg("transcribe")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("Usage:"));
    speech2md()
        .args(["transcribe", "meeting.wav", "--speakers", "0"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("speakers"));
}

#[test]
fn prompt_text_and_file_conflict_at_argument_parsing() {
    speech2md()
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
    speech2md()
        .args(["model", "list"])
        .env("SPEECH2MD_MODEL_DIR", root.path().join("models"))
        .env("HTTPS_PROXY", "http://127.0.0.1:9")
        .assert()
        .success()
        .stdout(predicate::str::contains("whisper-base: not installed"))
        .stdout(predicate::str::contains(
            "speaker-segmentation: not installed",
        ));
    speech2md()
        .arg("doctor")
        .env("SPEECH2MD_MODEL_DIR", root.path().join("models"))
        .env("SPEECH2MD_ENGINE_DIR", root.path().join("engines"))
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
    speech2md()
        .arg("transcribe")
        .arg(&input)
        .arg("--output")
        .arg(&output)
        .env("SPEECH2MD_MODEL_DIR", root.path().join("models"))
        .env("SPEECH2MD_ENGINE_DIR", root.path().join("engines"))
        .assert()
        .code(6)
        .stderr(predicate::str::contains("output already exists"));
    assert_eq!(
        std::fs::read(output).expect("preserved output"),
        b"existing bytes"
    );
}
