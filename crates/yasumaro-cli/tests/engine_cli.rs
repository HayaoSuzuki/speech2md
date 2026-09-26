use assert_cmd::Command;
use predicates::prelude::*;

fn yasumaro() -> Command {
    Command::new(env!("CARGO_BIN_EXE_yasumaro"))
}

#[test]
fn help_lists_engine_management_commands() {
    yasumaro()
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
    yasumaro()
        .assert()
        .code(2)
        .stderr(predicate::str::contains("Usage:"));
}

#[test]
#[cfg(target_os = "windows")]
fn install_reports_a_network_failure_without_leaving_the_test_root() {
    let root = tempfile::tempdir().expect("temporary engine root");
    yasumaro()
        .args(["engine", "install"])
        .env("YASUMARO_ENGINE_DIR", root.path())
        .env("HTTPS_PROXY", "http://127.0.0.1:9")
        .assert()
        .code(4)
        .stderr(predicate::str::contains("engine download failed"))
        .stderr(predicate::str::contains("check the network connection"));
}

#[test]
fn list_is_local() {
    let root = tempfile::tempdir().expect("temporary engine root");
    yasumaro()
        .args(["engine", "list"])
        .env("YASUMARO_ENGINE_DIR", root.path())
        .env("HTTPS_PROXY", "http://127.0.0.1:9")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "windows-x86_64 whispercpp-v1.9.4-speech2md.1: not installed",
        ));
}

#[test]
#[cfg(target_os = "windows")]
fn verify_reports_an_actionable_missing_engine() {
    let root = tempfile::tempdir().expect("temporary engine root");

    yasumaro()
        .args(["engine", "verify"])
        .env("YASUMARO_ENGINE_DIR", root.path())
        .env("HTTPS_PROXY", "http://127.0.0.1:9")
        .assert()
        .code(4)
        .stderr(predicate::str::contains("is not installed"))
        .stderr(predicate::str::contains("yasumaro engine install"));
}

#[test]
#[cfg(target_os = "windows")]
fn prune_is_idempotent_when_no_obsolete_engine_exists() {
    let root = tempfile::tempdir().expect("temporary engine root");

    yasumaro()
        .args(["engine", "prune"])
        .env("YASUMARO_ENGINE_DIR", root.path())
        .env("HTTPS_PROXY", "http://127.0.0.1:9")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Pruned 0 engine installation(s); kept 0 locked installation(s).",
        ));
}

#[test]
#[cfg(not(target_os = "windows"))]
fn unpublished_platform_rejects_engine_operations_without_network_access() {
    let root = tempfile::tempdir().expect("temporary engine root");

    for command in ["install", "verify", "prune"] {
        yasumaro()
            .args(["engine", command])
            .env("YASUMARO_ENGINE_DIR", root.path())
            .env("HTTPS_PROXY", "http://127.0.0.1:9")
            .assert()
            .code(4)
            .stderr(predicate::str::contains(
                "no engine artifact is published for",
            ));
    }
}
