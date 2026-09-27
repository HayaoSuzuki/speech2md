use proptest::prelude::*;
use pulldown_cmark::{Event, HeadingLevel, Parser, Tag};
use yasumaro_core::{SpeakerId, TimeSpan, Timestamp, TranscriptDocument, Utterance};
use yasumaro_formats::render_commonmark;

fn markdown_sensitive_text() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop_oneof![
            Just('#'),
            Just('`'),
            Just('*'),
            Just('_'),
            Just('>'),
            Just('-'),
            Just('+'),
            Just(' '),
            Just('\t'),
            Just('\n'),
            Just('\r'),
            any::<char>(),
        ],
        0..80,
    )
    .prop_map(|characters| characters.into_iter().collect())
}

fn markdown_content() -> impl Strategy<Value = String> {
    prop_oneof![
        markdown_sensitive_text(),
        prop::sample::select(vec![
            "[link](https://example.com)",
            "![image](x)",
            "<script>x</script>",
            "```\ncode\n```",
            "\r\t#",
            "&amp; &#32; \\ * _",
            "a\r\nb\rc\n\0",
            "---\n===\n***",
        ])
        .prop_map(str::to_owned),
    ]
}

fn visible(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_whitespace())
        .map(|character| {
            if character == '\0' {
                '\u{fffd}'
            } else {
                character
            }
        })
        .collect()
}

proptest! {
    #[test]
    fn arbitrary_content_cannot_create_extra_blocks(
        title in markdown_sensitive_text(),
        body in markdown_sensitive_text(),
        speaker in prop::option::of(0_u32..16),
    ) {
        let document = TranscriptDocument {
            title,
            utterances: vec![Utterance {
                span: TimeSpan::new(Timestamp::from_millis(0), Timestamp::from_millis(1))
                    .expect("literal ordered span"),
                speaker: speaker.map(SpeakerId::new),
                text: body,
            }],
        };

        let rendered = render_commonmark(&document);
        let events = Parser::new(&rendered).collect::<Vec<_>>();
        let headings = events.iter().filter(|event| {
            matches!(event, Event::Start(Tag::Heading { level: HeadingLevel::H1, .. }))
        }).count();
        let code_blocks = events.iter().filter(|event| {
            matches!(event, Event::Start(Tag::CodeBlock(_)))
        }).count();
        let strong_labels = events.iter().filter(|event| {
            matches!(event, Event::Start(Tag::Strong))
        }).count();

        prop_assert_eq!(headings, 1);
        prop_assert_eq!(code_blocks, 0);
        prop_assert_eq!(strong_labels, 1);
    }

    #[test]
    fn multiple_utterances_preserve_visible_text_and_only_allowed_markdown_structure(
        title in markdown_content(),
        values in prop::collection::vec((
            prop_oneof![Just(0_u64), Just(3_599_999), Just(3_600_000), Just(u64::MAX), any::<u64>()],
            prop::option::of(prop_oneof![Just(u32::MAX), any::<u32>()]),
            markdown_content(),
        ), 0..12),
    ) {
        let document = TranscriptDocument {
            title,
            utterances: values.into_iter().map(|(start, speaker, text)| Utterance {
                span: TimeSpan::new(Timestamp::from_millis(start), Timestamp::from_millis(start))
                    .expect("zero-length span is ordered"),
                speaker: speaker.map(SpeakerId::new), text,
            }).collect(),
        };
        let rendered = render_commonmark(&document);
        let mut headings = 0;
        let mut labels = 0;
        let mut actual = String::new();
        for event in Parser::new(&rendered) {
            match event {
                Event::Start(Tag::Heading { level: HeadingLevel::H1, .. }) => headings += 1,
                Event::Start(Tag::Strong) => labels += 1,
                Event::Start(Tag::Paragraph) | Event::End(_) | Event::SoftBreak | Event::HardBreak => {},
                Event::Text(text) => actual.push_str(&visible(&text)),
                other => prop_assert!(false, "unexpected Markdown structure: {other:?}"),
            }
        }
        prop_assert_eq!(headings, 1);
        prop_assert_eq!(labels, document.utterances.len());
        // Compare visible content after CommonMark's NUL/whitespace handling;
        // this detects lost prose, double escaping and wrong speaker labels.
        let mut expected = visible(&document.title);
        for utterance in &document.utterances {
            let speaker = utterance.speaker.map_or_else(|| "Unknown".to_owned(), |id| {
                format!("Speaker {}", u64::from(id.as_u32()) + 1)
            });
            let seconds = utterance.span.start().as_millis() / 1_000;
            expected.push_str(&visible(&format!("{speaker}（{:02}:{:02}:{:02}）{}",
                seconds / 3_600, seconds / 60 % 60, seconds % 60, utterance.text)));
        }
        prop_assert_eq!(actual, expected);
    }
}
