use assert_cmd::Command;
use predicates::prelude::*;
use yasumaro_runtime::{ModelId, ModelStore};

fn yasumaro() -> Command {
    Command::new(env!("CARGO_BIN_EXE_yasumaro"))
}

#[test]
fn selected_model_install_reports_failure_without_partial_files() {
    for model in [
        "whisper-small",
        "whisper-medium",
        "whisper-large-v3",
        "whisper-large-v3-turbo",
    ] {
        let root = tempfile::tempdir().expect("temporary model root");

        yasumaro()
            .args(["model", "install", model])
            .env("YASUMARO_MODEL_DIR", root.path())
            .env("HTTPS_PROXY", "http://127.0.0.1:9")
            .assert()
            .code(4)
            .stderr(predicate::str::contains(format!(
                "model {model} download failed"
            )));

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
}

#[test]
fn model_help_lists_every_installable_choice() {
    yasumaro()
        .args(["model", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("remove"));
    yasumaro()
        .args(["model", "install", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("whisper-base"))
        .stdout(predicate::str::contains("whisper-small"))
        .stdout(predicate::str::contains("whisper-medium"))
        .stdout(predicate::str::contains("whisper-large-v3"))
        .stdout(predicate::str::contains("whisper-large-v3-turbo"))
        .stdout(predicate::str::contains("speaker-segmentation"))
        .stdout(predicate::str::contains("speaker-embedding"));
}

#[test]
fn remove_requires_at_least_one_model() {
    yasumaro()
        .args(["model", "remove"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("required"));
}

#[test]
fn remove_deletes_final_and_partial_files() {
    let root = tempfile::tempdir().expect("temporary model root");
    let final_path = root.path().join("ggml-base.bin");
    let partial_path = root.path().join("ggml-base.bin.part");
    std::fs::write(&final_path, b"final").expect("write final model");
    std::fs::write(&partial_path, b"partial").expect("write partial model");

    yasumaro()
        .args(["model", "remove", "whisper-base"])
        .env("YASUMARO_MODEL_DIR", root.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Removed 1 model(s)."));

    assert!(!final_path.exists());
    assert!(!partial_path.exists());
}

#[test]
fn remove_deduplicates_models() {
    let root = tempfile::tempdir().expect("temporary model root");
    std::fs::write(root.path().join("ggml-base.bin"), b"base").expect("write base model");
    std::fs::write(root.path().join("ggml-small.bin"), b"small").expect("write small model");

    yasumaro()
        .args([
            "model",
            "remove",
            "whisper-small",
            "whisper-base",
            "whisper-small",
        ])
        .env("YASUMARO_MODEL_DIR", root.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Removed 2 model(s)."));
}

#[test]
fn remove_stops_after_first_failure_without_rollback() {
    let root = tempfile::tempdir().expect("temporary model root");
    let base = root.path().join("ggml-base.bin");
    let small = root.path().join("ggml-small.bin");
    let medium = root.path().join("ggml-medium.bin");
    std::fs::write(&base, b"base").expect("write base model");
    std::fs::write(&small, b"small").expect("write small model");
    std::fs::write(&medium, b"medium").expect("write medium model");
    let store = ModelStore::new(root.path());
    let _lease = store
        .acquire(ModelId::WhisperSmall)
        .expect("lease middle model");

    yasumaro()
        .args([
            "model",
            "remove",
            "whisper-medium",
            "whisper-small",
            "whisper-base",
        ])
        .env("YASUMARO_MODEL_DIR", root.path())
        .assert()
        .code(4)
        .stdout(predicate::str::is_empty());

    assert!(!base.exists(), "successful prefix is not rolled back");
    assert!(small.exists(), "busy model is unchanged");
    assert!(
        medium.exists(),
        "models after the failure are not processed"
    );
}

#[test]
fn model_in_use_reports_retry_help() {
    let root = tempfile::tempdir().expect("temporary model root");
    std::fs::write(root.path().join("ggml-base.bin"), b"base").expect("write base model");
    let store = ModelStore::new(root.path());
    let _lease = store
        .acquire(ModelId::WhisperBase)
        .expect("lease base model");

    yasumaro()
        .args(["model", "remove", "whisper-base"])
        .env("YASUMARO_MODEL_DIR", root.path())
        .assert()
        .code(4)
        .stderr(predicate::str::contains(
            "retry `yasumaro model remove whisper-base` after transcription or installation completes",
        ));
}

#[test]
fn readme_explains_model_removal_and_cleanup() {
    let readme = include_str!("../../../README.md");

    for required in [
        "yasumaro model remove whisper-small",
        "一つ以上",
        "`.part`",
        "通常の失敗",
        "強制終了",
        "次回の`model install`または`model remove`",
        "model whisper-small is in use",
        "途中から再開せず",
    ] {
        assert!(
            readme.contains(required),
            "README is missing the model lifecycle contract: {required}"
        );
    }
}
