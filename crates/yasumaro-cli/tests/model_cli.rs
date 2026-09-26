use assert_cmd::Command;
use predicates::prelude::*;

fn yasumaro() -> Command {
    Command::new(env!("CARGO_BIN_EXE_yasumaro"))
}

#[test]
fn selected_model_install_reports_failure_without_partial_files() {
    let root = tempfile::tempdir().expect("temporary model root");

    yasumaro()
        .args(["model", "install", "whisper-small"])
        .env("YASUMARO_MODEL_DIR", root.path())
        .env("HTTPS_PROXY", "http://127.0.0.1:9")
        .assert()
        .code(4)
        .stderr(predicate::str::contains(
            "model whisper-small download failed",
        ));

    let files = std::fs::read_dir(root.path())
        .expect("read model root")
        .collect::<Result<Vec<_>, _>>()
        .expect("read model entries");
    assert!(files.iter().all(|entry| {
        entry
            .path()
            .extension()
            .is_none_or(|extension| extension != "part")
    }));
}

#[test]
fn model_help_lists_every_installable_choice() {
    yasumaro()
        .args(["model", "install", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("whisper-base"))
        .stdout(predicate::str::contains("whisper-small"))
        .stdout(predicate::str::contains("speaker-segmentation"))
        .stdout(predicate::str::contains("speaker-embedding"));
}
