#![no_main]

use libfuzzer_sys::fuzz_target;
use pulldown_cmark::{Event, HeadingLevel, Parser, Tag};
use yasumaro_core::{SpeakerId, TimeSpan, Timestamp, TranscriptDocument, Utterance};
use yasumaro_formats::render_commonmark;

// CommonMark normalizes NUL and block whitespace. Compare all other visible
// characters so dropped/duplicated prose and double escaping are detected.
fn visible(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| if c == '\0' { '\u{fffd}' } else { c })
        .collect()
}

type Input = (Vec<u8>, Vec<(u64, Option<u32>, Vec<u8>)>);

fuzz_target!(|input: Input| {
    let (title, utterances) = input;
    let document = TranscriptDocument {
        title: String::from_utf8_lossy(&title[..title.len().min(1024)]).into_owned(),
        utterances: utterances
            .into_iter()
            .take(16)
            .map(|(start, speaker, text)| Utterance {
                span: TimeSpan::new(Timestamp::from_millis(start), Timestamp::from_millis(start))
                    .unwrap(),
                speaker: speaker.map(SpeakerId::new),
                text: String::from_utf8_lossy(&text[..text.len().min(2048)]).into_owned(),
            })
            .collect(),
    };
    let rendered = render_commonmark(&document);
    let mut headings = 0;
    let mut labels = 0;
    let mut actual = String::new();
    for event in Parser::new(&rendered) {
        match event {
            Event::Start(Tag::Heading {
                level: HeadingLevel::H1,
                ..
            }) => headings += 1,
            Event::Start(Tag::Strong) => labels += 1,
            Event::Start(Tag::Paragraph) | Event::End(_) | Event::SoftBreak | Event::HardBreak => {}
            Event::Text(text) => actual.push_str(&visible(&text)),
            other => panic!("unexpected Markdown structure: {other:?}"),
        }
    }
    assert_eq!(headings, 1);
    assert_eq!(labels, document.utterances.len());
    let mut expected = visible(&document.title);
    for utterance in &document.utterances {
        let label = utterance.speaker.map_or_else(
            || "Unknown".to_owned(),
            |id| format!("Speaker {}", u64::from(id.as_u32()) + 1),
        );
        let seconds = utterance.span.start().as_millis() / 1000;
        expected.push_str(&visible(&format!(
            "{label}（{:02}:{:02}:{:02}）{}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60,
            utterance.text
        )));
    }
    assert_eq!(actual, expected);
});
