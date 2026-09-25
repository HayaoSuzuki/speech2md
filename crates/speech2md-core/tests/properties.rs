use proptest::prelude::*;
use speech2md_core::{
    NormalizationConfig, OverlapRatio, SpeakerId, TimeSpan, Timestamp, Utterance,
    normalize_utterances, overlap_ms,
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
    fn valid_spans_preserve_bounds_and_duration(start in any::<u64>(), duration in any::<u32>()) {
        let end = start.saturating_add(u64::from(duration));
        let span = TimeSpan::new(Timestamp::from_millis(start), Timestamp::from_millis(end))
            .expect("saturating addition cannot put the end before the start");

        prop_assert_eq!(span.start().as_millis(), start);
        prop_assert_eq!(span.end().as_millis(), end);
        prop_assert_eq!(span.duration_ms(), end - start);
    }

    #[test]
    fn overlap_is_symmetric_and_bounded(
        left_start in 0_u64..1_000_000,
        left_length in 0_u32..100_000,
        right_start in 0_u64..1_000_000,
        right_length in 0_u32..100_000,
    ) {
        let left = TimeSpan::new(
            Timestamp::from_millis(left_start),
            Timestamp::from_millis(left_start + u64::from(left_length)),
        ).expect("generated left span is ordered");
        let right = TimeSpan::new(
            Timestamp::from_millis(right_start),
            Timestamp::from_millis(right_start + u64::from(right_length)),
        ).expect("generated right span is ordered");

        let overlap = overlap_ms(left, right);

        prop_assert_eq!(overlap, overlap_ms(right, left));
        prop_assert!(overlap <= left.duration_ms());
        prop_assert!(overlap <= right.duration_ms());
    }

    #[test]
    fn exact_overlap_ratio_matches_integer_cross_multiplication(
        numerator in 0_u16..=10_000,
        overlap in any::<u32>(),
        duration in any::<u32>(),
    ) {
        let ratio = OverlapRatio::from_basis_points(numerator)
            .expect("generated basis points are within the valid range");
        let expected = numerator == 0
            || (duration > 0
                && u128::from(duration) * u128::from(numerator)
                    <= u128::from(overlap) * 10_000_u128);

        prop_assert_eq!(ratio.is_met_by(u64::from(overlap), u64::from(duration)), expected);
    }

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
