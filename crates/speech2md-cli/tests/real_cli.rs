use std::collections::BTreeSet;
use std::path::PathBuf;

use assert_cmd::Command;
use pulldown_cmark::{Event, Parser, Tag};

#[test]
#[ignore = "requires locally installed engine, models, and a self-authored four-speaker fixture"]
fn transcribes_a_local_four_speaker_fixture_to_commonmark() {
    let engine_root = required_path("SPEECH2MD_ENGINE_DIR");
    let model_root = required_path("SPEECH2MD_MODEL_DIR");
    let fixture = required_path("SPEECH2MD_DIARIZATION_FIXTURE");
    assert!(
        fixture.is_file(),
        "fixture environment variable must name a file"
    );
    let root = tempfile::tempdir().expect("output root");
    let output = root.path().join("minutes.md");

    Command::new(env!("CARGO_BIN_EXE_speech2md"))
        .arg("transcribe")
        .arg(&fixture)
        .args(["--speakers", "4", "--output"])
        .arg(&output)
        .env("SPEECH2MD_ENGINE_DIR", engine_root)
        .env("SPEECH2MD_MODEL_DIR", model_root)
        .assert()
        .success();

    let commonmark = std::fs::read_to_string(output).expect("read generated CommonMark");
    let speaker_lines = commonmark
        .lines()
        .filter(|line| line.starts_with("**Speaker "))
        .collect::<Vec<_>>();
    let speakers = speaker_lines
        .iter()
        .filter_map(|line| line.split("**").nth(1))
        .collect::<BTreeSet<_>>();
    assert_eq!(speakers.len(), 4);
    let timestamps = speaker_lines
        .iter()
        .map(|line| {
            line.split_once('（')
                .and_then(|(_, rest)| rest.strip_suffix('）'))
                .expect("speaker line timestamp")
        })
        .collect::<Vec<_>>();
    assert!(timestamps.windows(2).all(|pair| pair[0] <= pair[1]));
    let events = Parser::new(&commonmark).collect::<Vec<_>>();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Start(Tag::Heading { .. })))
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Text(text) if !text.trim().is_empty()))
    );
}

fn required_path(name: &str) -> PathBuf {
    std::env::var_os(name).map_or_else(
        || panic!("{name} must be set for this ignored test"),
        PathBuf::from,
    )
}
