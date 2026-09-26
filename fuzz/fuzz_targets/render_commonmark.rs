#![no_main]

use libfuzzer_sys::fuzz_target;
use pulldown_cmark::{Event, HeadingLevel, Parser, Tag};
use speech2md_core::{SpeakerId, TimeSpan, Timestamp, TranscriptDocument, Utterance};
use speech2md_formats::render_commonmark;

fuzz_target!(|data: &[u8]| {
    let midpoint = data.len() / 2;
    let title = String::from_utf8_lossy(&data[..midpoint]).into_owned();
    let text = String::from_utf8_lossy(&data[midpoint..]).into_owned();
    let speaker = data.first().map(|byte| SpeakerId::new(u32::from(*byte)));
    let document = TranscriptDocument {
        title,
        utterances: vec![Utterance {
            span: TimeSpan::new(Timestamp::from_millis(0), Timestamp::from_millis(1))
                .expect("literal ordered span"),
            speaker,
            text,
        }],
    };

    let rendered = render_commonmark(&document);
    let events = Parser::new(&rendered).collect::<Vec<_>>();
    let headings = events
        .iter()
        .filter(|event| {
            matches!(
                event,
                Event::Start(Tag::Heading {
                    level: HeadingLevel::H1,
                    ..
                })
            )
        })
        .count();
    let code_blocks = events
        .iter()
        .filter(|event| matches!(event, Event::Start(Tag::CodeBlock(_))))
        .count();
    let strong_labels = events
        .iter()
        .filter(|event| matches!(event, Event::Start(Tag::Strong)))
        .count();

    assert_eq!(headings, 1);
    assert_eq!(code_blocks, 0);
    assert_eq!(strong_labels, 1);
});
