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

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

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
}
