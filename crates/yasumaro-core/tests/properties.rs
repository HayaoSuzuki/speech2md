use proptest::prelude::*;
use yasumaro_core::{
    AssignmentConfig, NormalizationConfig, OverlapRatio, SpeakerId, SpeakerTurn, TimeSpan,
    TimedToken, Timestamp, TranscribedSegment, Utterance, assign_speakers, normalize_utterances,
    overlap_ms,
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
            speaker: (speaker % 5 != 4).then(|| SpeakerId::new(u32::from(speaker % 4))),
            text: format!("<{index}>"),
        })
        .collect()
}

fn boundary_millis() -> impl Strategy<Value = u64> {
    prop_oneof![
        Just(0),
        Just(1),
        Just(u64::MAX),
        Just(u64::MAX - 1),
        any::<u64>()
    ]
}

fn span(start: u64, end: u64) -> TimeSpan {
    TimeSpan::new(Timestamp::from_millis(start), Timestamp::from_millis(end))
        .expect("generated ordered span")
}

fn unicode_text() -> impl Strategy<Value = String> {
    prop::collection::vec(any::<char>(), 0..16)
        .prop_map(|characters| characters.into_iter().collect())
}

proptest! {
    #[test]
    fn valid_spans_preserve_bounds_and_duration(start in boundary_millis(), duration in boundary_millis()) {
        let end = start.saturating_add(duration);
        let span = TimeSpan::new(Timestamp::from_millis(start), Timestamp::from_millis(end))
            .expect("saturating addition cannot put the end before the start");

        prop_assert_eq!(span.start().as_millis(), start);
        prop_assert_eq!(span.end().as_millis(), end);
        prop_assert_eq!(span.duration_ms(), end - start);
    }

    #[test]
    fn overlap_is_symmetric_and_bounded(
        left_start in boundary_millis(),
        left_length in boundary_millis(),
        right_start in boundary_millis(),
        right_length in boundary_millis(),
    ) {
        let left = TimeSpan::new(
            Timestamp::from_millis(left_start),
            Timestamp::from_millis(left_start.saturating_add(left_length)),
        ).expect("generated left span is ordered");
        let right = TimeSpan::new(
            Timestamp::from_millis(right_start),
            Timestamp::from_millis(right_start.saturating_add(right_length)),
        ).expect("generated right span is ordered");

        let overlap = overlap_ms(left, right);

        prop_assert_eq!(overlap, overlap_ms(right, left));
        prop_assert!(overlap <= left.duration_ms());
        prop_assert!(overlap <= right.duration_ms());
    }

    #[test]
    fn exact_overlap_ratio_matches_integer_cross_multiplication(
        numerator in 0_u16..=10_000,
        overlap in boundary_millis(),
        duration in boundary_millis(),
    ) {
        let ratio = OverlapRatio::from_basis_points(numerator)
            .expect("generated basis points are within the valid range");
        let expected = numerator == 0
            || (duration > 0
                && u128::from(duration) * u128::from(numerator)
                    <= u128::from(overlap) * 10_000_u128);

        prop_assert_eq!(ratio.is_met_by(overlap, duration), expected);
    }

    #[test]
    fn normalization_preserves_order_validity_and_text(
        values in prop::collection::vec((0_u16..10_000, 0_u16..1_000, any::<u8>()), 0..100),
        max_gap_ms in 0_u64..1_000,
        max_chars in 0_usize..256,
    ) {
        let input = make_utterances(&values);
        let mut sorted = input.clone();
        sorted.sort_by_key(|item| (item.span.start(), item.span.end()));
        let expected_text = sorted.iter().map(|item| item.text.as_str()).collect::<String>();
        let config = NormalizationConfig { max_gap_ms, max_chars };

        let result = normalize_utterances(input, &config);

        prop_assert!(result.windows(2).all(|pair| pair[0].span.start() <= pair[1].span.start()));
        prop_assert!(result.iter().all(|item| item.span.start() <= item.span.end()));
        prop_assert_eq!(result.iter().map(|item| item.text.as_str()).collect::<String>(), expected_text);
        prop_assert_eq!(normalize_utterances(result.clone(), &config), result);
    }

    #[test]
    fn assignment_threshold_is_inclusive_and_ties_choose_the_smallest_id(
        scale in 1_u64..=u64::MAX / 10_000,
        threshold in 0_u16..=10_000,
        first_id in 0_u32..u32::MAX,
        offset in boundary_millis(),
    ) {
        let duration = scale * 10_000;
        let start = offset.min(u64::MAX - duration);
        let boundary = scale * u64::from(threshold);
        let transcript = [TranscribedSegment {
            span: span(start, start + duration), text: "本文".into(),
            confidence: None, tokens: vec![],
        }];
        let config = AssignmentConfig {
            min_overlap_ratio: OverlapRatio::from_basis_points(threshold).expect("valid threshold"),
        };
        for overlap in [boundary.saturating_sub(1), boundary, boundary.saturating_add(1).min(duration)] {
            let mut turns = [first_id + 1, first_id].map(|id| SpeakerTurn {
                span: span(start, start + overlap), speaker: SpeakerId::new(id), confidence: None,
            });
            let expected = (overlap >= boundary).then(|| SpeakerId::new(first_id));
            for _ in 0..2 {
                let result = assign_speakers(&transcript, &turns, &config);
                prop_assert_eq!(&result, &[Utterance {
                    span: transcript[0].span, text: "本文".into(), speaker: expected,
                }]);
                turns.reverse();
            }
        }
    }

    #[test]
    fn zero_duration_assignment_requires_zero_threshold_and_a_candidate(
        time in boundary_millis(),
        speaker in any::<u32>(),
        positive_threshold in 1_u16..=10_000,
    ) {
        let transcript = [TranscribedSegment {
            span: span(time, time), text: "zero".into(), confidence: None, tokens: vec![],
        }];
        let turns = [SpeakerTurn {
            span: span(0, u64::MAX), speaker: SpeakerId::new(speaker), confidence: None,
        }];
        for threshold in [0, positive_threshold] {
            let config = AssignmentConfig {
                min_overlap_ratio: OverlapRatio::from_basis_points(threshold).expect("valid threshold"),
            };
            let expected = (threshold == 0).then(|| SpeakerId::new(speaker));
            prop_assert_eq!(assign_speakers(&transcript, &turns, &config)[0].speaker, expected);
            prop_assert_eq!(assign_speakers(&transcript, &[], &config)[0].speaker, None);
        }
    }

    #[test]
    fn multiple_segments_preserve_text_spans_and_speaker_boundaries(
        values in prop::collection::vec((any::<u32>(), any::<bool>(), unicode_text(), unicode_text()), 0..16),
        offset in boundary_millis(),
        threshold in 0_u16..=10_000,
    ) {
        let base = offset.min(u64::MAX - 16_000);
        let mut transcript = Vec::new();
        let mut turns = Vec::new();
        let mut expected = Vec::new();
        for (index, (id, tokenized, left_text, right_text)) in values.into_iter().enumerate() {
            let start = base + u64::try_from(index).expect("bounded index") * 1_000;
            let left = span(start, start + 100);
            let right = span(start + 100, start + 200);
            let ids = [SpeakerId::new(id), SpeakerId::new(id.wrapping_add(1))];
            turns.extend([left, right].into_iter().zip(ids).map(|(span, speaker)| SpeakerTurn {
                span, speaker, confidence: None,
            }));
            let text = format!("{left_text}{right_text}");
            let tokens = if tokenized {
                expected.extend([
                    Utterance { span: left, speaker: Some(ids[0]), text: left_text.clone() },
                    Utterance { span: right, speaker: Some(ids[1]), text: right_text.clone() },
                ]);
                // Deliberately reverse token order while keeping segment order.
                vec![TimedToken { span: right, text: right_text }, TimedToken { span: left, text: left_text }]
            } else {
                expected.push(Utterance {
                    span: span(start, start + 200), text: text.clone(),
                    speaker: (threshold <= 5_000).then_some(ids[0].min(ids[1])),
                });
                vec![]
            };
            transcript.push(TranscribedSegment {
                span: span(start, start + 200), text, tokens, confidence: None,
            });
        }
        turns.reverse();
        let config = AssignmentConfig {
            min_overlap_ratio: OverlapRatio::from_basis_points(threshold).expect("valid threshold"),
        };
        prop_assert_eq!(assign_speakers(&transcript, &turns, &config), expected);
    }

    #[test]
    fn normalization_respects_unicode_character_and_gap_boundaries(
        left_text in unicode_text(), right_text in unicode_text(),
        gap in 1_u64..1_000, offset in boundary_millis(),
    ) {
        let start = offset.min(u64::MAX - 1_002);
        let texts = [format!("左{left_text}"), format!("右{right_text}")];
        let chars = texts.iter().map(|text| text.chars().count()).sum::<usize>();
        for speakers in [[Some(SpeakerId::new(0)); 2], [None; 2], [Some(SpeakerId::new(0)), Some(SpeakerId::new(1))]] {
            let pair = [
                Utterance { span: span(start, start + 1), speaker: speakers[0], text: texts[0].clone() },
                Utterance { span: span(start + 1 + gap, start + 2 + gap), speaker: speakers[1], text: texts[1].clone() },
            ];
            for max_gap_ms in [gap - 1, gap, gap + 1] {
                for max_chars in [chars - 1, chars, chars + 1] {
                    let config = NormalizationConfig { max_gap_ms, max_chars };
                    let actual = normalize_utterances(pair.to_vec(), &config);
                    let should_merge = speakers[0].is_some() && speakers[0] == speakers[1]
                        && max_gap_ms >= gap && max_chars >= chars;
                    if should_merge {
                        prop_assert_eq!(actual, vec![Utterance {
                            span: span(start, start + 2 + gap), speaker: speakers[0],
                            text: format!("{}{}", texts[0], texts[1]),
                        }]);
                    } else {
                        prop_assert_eq!(actual, pair.to_vec());
                    }
                }
            }
        }
    }

    #[test]
    fn normalization_deduplicates_only_overlapping_known_speaker_text(
        text in unicode_text(), duration in 1_u64..1_000, offset in boundary_millis(),
    ) {
        let start = offset.min(u64::MAX - 2_001);
        let text = format!("本文{text}");
        let config = NormalizationConfig { max_gap_ms: 0, max_chars: 0 };
        for distance in [duration - 1, duration, duration + 1] {
            for speakers in [
                [None; 2], [Some(SpeakerId::new(1)); 2],
                [Some(SpeakerId::new(1)), Some(SpeakerId::new(2))],
                [Some(SpeakerId::new(1)), None], [None, Some(SpeakerId::new(1))],
            ] {
                let pair = vec![
                    Utterance { span: span(start, start + duration), speaker: speakers[0], text: text.clone() },
                    Utterance { span: span(start + distance, start + distance + duration), speaker: speakers[1], text: text.clone() },
                ];
                let result = normalize_utterances(pair.clone(), &config);
                if speakers[0].is_some() && speakers[0] == speakers[1] && distance < duration {
                    prop_assert_eq!(result, vec![Utterance {
                        span: span(start, start + distance + duration), speaker: speakers[0], text: text.clone(),
                    }]);
                } else {
                    prop_assert_eq!(result, pair);
                }
            }
        }
    }

    #[test]
    fn speaker_assignment_preserves_all_token_text_and_ignores_turn_order(
        token_values in prop::collection::vec((0_u16..10_000, 1_u16..1_000, any::<u8>()), 0..100),
        turn_values in prop::collection::vec((0_u16..10_000, 1_u16..2_000, 0_u8..8), 0..100),
    ) {
        let tokens = token_values
            .iter()
            .enumerate()
            .map(|(index, &(start, duration, _))| TimedToken {
                span: TimeSpan::new(
                    Timestamp::from_millis(u64::from(start)),
                    Timestamp::from_millis(u64::from(start) + u64::from(duration)),
                ).expect("generated token span is ordered"),
                text: format!("<{index}>"),
            })
            .collect::<Vec<_>>();
        let mut sorted_tokens = tokens.clone();
        sorted_tokens.sort_by_key(|token| (token.span.start(), token.span.end()));
        let expected_text = sorted_tokens.iter().map(|token| token.text.as_str()).collect::<String>();
        let transcript = vec![TranscribedSegment {
            span: TimeSpan::new(Timestamp::from_millis(0), Timestamp::from_millis(11_000))
                .expect("literal segment span is ordered"),
            text: expected_text.clone(),
            confidence: None,
            tokens,
        }];
        let turns = turn_values.iter().map(|&(start, duration, speaker)| SpeakerTurn {
            span: TimeSpan::new(
                Timestamp::from_millis(u64::from(start)),
                Timestamp::from_millis(u64::from(start) + u64::from(duration)),
            ).expect("generated speaker span is ordered"),
            speaker: SpeakerId::new(u32::from(speaker)),
            confidence: None,
        }).collect::<Vec<_>>();
        let mut reversed_turns = turns.clone();
        reversed_turns.reverse();

        let assigned = assign_speakers(&transcript, &turns, &AssignmentConfig::default());
        let reversed = assign_speakers(&transcript, &reversed_turns, &AssignmentConfig::default());

        prop_assert_eq!(assigned.iter().map(|item| item.text.as_str()).collect::<String>(), expected_text);
        prop_assert_eq!(assigned, reversed);
    }
}
