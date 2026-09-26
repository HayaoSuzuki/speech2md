use assert_cmd::Command;
use predicates::prelude::*;

fn speech2md() -> Command {
    Command::new(env!("CARGO_BIN_EXE_speech2md"))
}

#[test]
fn help_lists_engine_management_commands() {
    speech2md()
        .args(["engine", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("install"))
        .stdout(predicate::str::contains("list"))
        .stdout(predicate::str::contains("verify"))
        .stdout(predicate::str::contains("prune"));
}

#[test]
fn usage_errors_exit_with_code_two() {
    speech2md()
        .assert()
        .code(2)
        .stderr(predicate::str::contains("Usage:"));
}

#[test]
fn install_reports_an_unpublished_artifact_without_network_access() {
    speech2md()
        .args(["engine", "install"])
        .env("HTTPS_PROXY", "http://127.0.0.1:9")
        .assert()
        .code(4)
        .stderr(predicate::str::contains("error:"))
        .stderr(predicate::str::contains("help:"));
}

#[test]
fn list_is_local_and_verify_reports_an_actionable_missing_artifact() {
    let root = tempfile::tempdir().expect("temporary engine root");
    speech2md()
        .args(["engine", "list"])
        .env("SPEECH2MD_ENGINE_DIR", root.path())
        .env("HTTPS_PROXY", "http://127.0.0.1:9")
        .assert()
        .success()
        .stdout(predicate::str::contains("No engine artifacts"));

    speech2md()
        .args(["engine", "verify"])
        .env("SPEECH2MD_ENGINE_DIR", root.path())
        .env("HTTPS_PROXY", "http://127.0.0.1:9")
        .assert()
        .code(4)
        .stderr(predicate::str::contains("error:"))
        .stderr(predicate::str::contains("help:"));
}
