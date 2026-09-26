use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use speech2md_core::{SpeakerId, TimeSpan, Timestamp, TranscriptDocument, Utterance};
use speech2md_formats::render_commonmark;

fn utterance(speaker: Option<u32>, start_ms: u64, text: &str) -> Utterance {
    Utterance {
        span: TimeSpan::new(
            Timestamp::from_millis(start_ms),
            Timestamp::from_millis(start_ms + 1_000),
        )
        .expect("fixture span is ordered"),
        speaker: speaker.map(SpeakerId::new),
        text: text.into(),
    }
}

fn document(title: &str, utterances: Vec<Utterance>) -> TranscriptDocument {
    TranscriptDocument {
        title: title.into(),
        utterances,
    }
}

#[test]
fn renders_speaker_and_timestamp() {
    let document = document(
        "meeting",
        vec![utterance(Some(1), 12_000, "API側の変更は完了しています。")],
    );

    assert_eq!(
        render_commonmark(&document),
        "# meeting\n\n**Speaker 2**（00:00:12）\n\nAPI側の変更は完了しています。\n"
    );
}

#[test]
fn renders_unknown_hours_and_escaped_control_characters() {
    let document = document(
        "long #1",
        vec![utterance(None, 10_862_000, "# status\n- item")],
    );

    let output = render_commonmark(&document);

    assert!(output.starts_with("# long \\#1\n"));
    assert!(output.contains("**Unknown**（03:01:02）"));
    assert!(output.contains("\\# status\n\\- item"));
}

#[test]
fn renders_an_empty_document_as_one_heading() {
    assert_eq!(render_commonmark(&document("empty", vec![])), "# empty\n");
}

#[test]
fn title_newlines_cannot_create_additional_blocks() {
    let output = render_commonmark(&document("first\n# injected", vec![]));
    let heading_count = Parser::new(&output)
        .filter(|event| matches!(event, Event::Start(Tag::Heading { .. })))
        .count();

    assert_eq!(output, "# first \\# injected\n");
    assert_eq!(heading_count, 1);
}

#[test]
fn parser_sees_only_the_document_heading_and_strong_speaker_labels() {
    let output = render_commonmark(&document(
        "meeting",
        vec![
            utterance(Some(0), 0, "# not a heading"),
            utterance(None, 1_000, "- not a list"),
        ],
    ));
    let events = Parser::new(&output).collect::<Vec<_>>();

    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::Start(Tag::Heading { .. })))
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::Start(Tag::Strong)))
            .count(),
        2
    );
    assert!(!events.iter().any(|event| matches!(
        event,
        Event::Start(Tag::List(_)) | Event::End(TagEnd::List(_))
    )));
}

#[test]
fn leading_indentation_does_not_create_a_code_block() {
    let output = render_commonmark(&document(
        "meeting",
        vec![utterance(Some(0), 0, "    spaces\n\ttab")],
    ));
    let events = Parser::new(&output).collect::<Vec<_>>();

    assert!(
        !events
            .iter()
            .any(|event| matches!(event, Event::Start(Tag::CodeBlock(_))))
    );
    let mut rendered_text = String::new();
    for event in &events {
        match event {
            Event::Text(text) => rendered_text.push_str(text),
            Event::SoftBreak | Event::HardBreak => rendered_text.push('\n'),
            _ => {}
        }
    }
    assert!(rendered_text.contains("    spaces\n\ttab"));
}
