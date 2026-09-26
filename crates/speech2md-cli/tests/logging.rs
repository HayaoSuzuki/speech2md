use std::process::Command;

fn speech2md() -> Command {
    Command::new(env!("CARGO_BIN_EXE_speech2md"))
}

#[test]
fn accepts_a_valid_rust_log_filter() {
    let output = speech2md()
        .args(["engine", "list"])
        .env("RUST_LOG", "speech2md_cli=debug")
        .output()
        .expect("run speech2md");

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("No engine artifacts"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("logging initialized"));
}

#[test]
fn rejects_an_invalid_rust_log_filter() {
    let output = speech2md()
        .args(["engine", "list"])
        .env("RUST_LOG", "speech2md=[invalid")
        .output()
        .expect("run speech2md");

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
}
