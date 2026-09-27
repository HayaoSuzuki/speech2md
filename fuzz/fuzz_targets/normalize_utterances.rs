#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use yasumaro_core::{NormalizationConfig, SpeakerId, Utterance, normalize_utterances};
use yasumaro_fuzz::SpanInput;

#[derive(Arbitrary, Debug)]
struct Input {
    gap: u16,
    chars: u16,
    values: Vec<(SpanInput, Option<u8>)>,
}

fn labeled_text(items: &[Utterance]) -> Vec<(char, Option<SpeakerId>)> {
    items
        .iter()
        .flat_map(|item| item.text.chars().map(|c| (c, item.speaker)))
        .collect()
}

fuzz_target!(|input: Input| {
    let config = NormalizationConfig {
        max_gap_ms: u64::from(input.gap),
        max_chars: usize::from(input.chars),
    };
    // Unique, nonempty markers isolate merging from intentional deduplication.
    let values = input
        .values
        .into_iter()
        .take(64)
        .enumerate()
        .map(|(index, (span, speaker))| Utterance {
            span: span.span(),
            speaker: speaker.map(|id| SpeakerId::new(u32::from(id % 4))),
            text: format!("<{index}:日本語>"),
        })
        .collect::<Vec<_>>();
    let mut sorted = values.clone();
    sorted.sort_by_key(|item| (item.span.start(), item.span.end()));
    let result = normalize_utterances(values, &config);
    assert_eq!(labeled_text(&result), labeled_text(&sorted));
    assert!(
        result
            .windows(2)
            .all(|pair| pair[0].span.start() <= pair[1].span.start())
    );
    assert_eq!(normalize_utterances(result.clone(), &config), result);

    // Pairwise contracts cover arbitrary overlaps, unknown speakers, character
    // limits and intentional duplicates without assuming all text is retained.
    for pair in sorted.windows(2) {
        let same_known = pair[0].speaker.is_some() && pair[0].speaker == pair[1].speaker;
        let gap = pair[1]
            .span
            .start()
            .as_millis()
            .saturating_sub(pair[0].span.end().as_millis());
        let chars = pair[0].text.chars().count() + pair[1].text.chars().count();
        let merged = same_known && gap <= config.max_gap_ms && chars <= config.max_chars;
        let normalized = normalize_utterances(pair.to_vec(), &config);
        if merged {
            assert_eq!(normalized.len(), 1);
            assert_eq!(normalized[0].span.start(), pair[0].span.start());
            assert_eq!(
                normalized[0].span.end(),
                pair[0].span.end().max(pair[1].span.end())
            );
            assert_eq!(
                normalized[0].text,
                format!("{}{}", pair[0].text, pair[1].text)
            );
        } else {
            assert_eq!(normalized, pair);
        }

        let mut duplicate = pair.to_vec();
        duplicate[1].text = duplicate[0].text.clone();
        let deduplicated = same_known && pair[1].span.start() < pair[0].span.end();
        let actual = normalize_utterances(
            duplicate.clone(),
            &NormalizationConfig {
                max_gap_ms: 0,
                max_chars: 0,
            },
        );
        if deduplicated {
            assert_eq!(actual.len(), 1);
            assert_eq!(actual[0].text, duplicate[0].text);
            assert_eq!(actual[0].span.start(), pair[0].span.start());
            assert_eq!(
                actual[0].span.end(),
                pair[0].span.end().max(pair[1].span.end())
            );
        } else {
            assert_eq!(actual, duplicate);
        }
    }
});
