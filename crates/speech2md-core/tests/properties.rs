use proptest::prelude::*;
use speech2md_core::{
    NormalizationConfig, SpeakerId, TimeSpan, Timestamp, Utterance, normalize_utterances,
};

fn make_utterances(values: &[(u16, u16, u8)]) -> Vec<Utterance> {
    values
        .iter()
        .enumerate()
        .map(|(index, &(start, duration, speaker))| Utterance {
            span: TimeSpan::new(
                Timestamp::from_millis(u64::from(start)),
                Timestamp::from_millis(u64::from(start) + u64::from(duration)),
            )
            .expect("generated end is not before start"),
            speaker: Some(SpeakerId::new(u32::from(speaker % 4))),
            text: format!("<{index}>"),
        })
        .collect()
}

proptest! {
    #[test]
    fn normalization_preserves_order_validity_and_text(
        values in prop::collection::vec((0_u16..10_000, 0_u16..1_000, any::<u8>()), 0..100)
    ) {
        let input = make_utterances(&values);
        let mut sorted = input.clone();
        sorted.sort_by_key(|item| (item.span.start(), item.span.end()));
        let expected_text = sorted.iter().map(|item| item.text.as_str()).collect::<String>();
        let config = NormalizationConfig { max_gap_ms: 200, max_chars: usize::MAX };

        let result = normalize_utterances(input, &config);

        prop_assert!(result.windows(2).all(|pair| pair[0].span.start() <= pair[1].span.start()));
        prop_assert!(result.iter().all(|item| item.span.start() <= item.span.end()));
        prop_assert_eq!(result.iter().map(|item| item.text.as_str()).collect::<String>(), expected_text);
        prop_assert_eq!(normalize_utterances(result.clone(), &config), result);
    }
}
