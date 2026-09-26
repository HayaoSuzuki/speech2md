use std::process::Command;

fn yasumaro() -> Command {
    Command::new(env!("CARGO_BIN_EXE_yasumaro"))
}

#[test]
fn accepts_a_valid_rust_log_filter() {
    let root = tempfile::tempdir().expect("temporary engine root");
    let output = yasumaro()
        .args(["engine", "list"])
        .env("YASUMARO_ENGINE_DIR", root.path())
        .env("RUST_LOG", "yasumaro_cli=debug")
        .output()
        .expect("run yasumaro");

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("windows-x86_64"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("logging initialized"));
}

#[test]
fn rejects_an_invalid_rust_log_filter() {
    let output = yasumaro()
        .args(["engine", "list"])
        .env("RUST_LOG", "yasumaro=[invalid")
        .output()
        .expect("run yasumaro");

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
}
