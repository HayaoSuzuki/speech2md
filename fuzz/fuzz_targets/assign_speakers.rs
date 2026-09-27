#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use yasumaro_core::{
    AssignmentConfig, OverlapRatio, SpeakerId, SpeakerTurn, TimeSpan, TimedToken,
    TranscribedSegment, Utterance, assign_speakers,
};
use yasumaro_fuzz::SpanInput;

#[derive(Arbitrary, Debug)]
struct SegmentInput {
    span: SpanInput,
    tokens: Vec<SpanInput>,
}

#[derive(Arbitrary, Debug)]
struct Input {
    threshold: u16,
    segments: Vec<SegmentInput>,
    turns: Vec<(SpanInput, u32)>,
}

// Independent of overlap_ms, is_met_by and the production max_by comparator.
fn expected_speaker(span: TimeSpan, turns: &[SpeakerTurn], threshold: u16) -> Option<SpeakerId> {
    let mut candidates = turns
        .iter()
        .map(|turn| {
            let start = span.start().max(turn.span.start()).as_millis();
            let end = span.end().min(turn.span.end()).as_millis();
            (end.saturating_sub(start), turn.speaker)
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|&(overlap, speaker)| (std::cmp::Reverse(overlap), speaker));
    let &(overlap, speaker) = candidates.first()?;
    let duration = span.end().as_millis() - span.start().as_millis();
    (threshold == 0
        || (duration != 0
            && u128::from(overlap) * 10_000 >= u128::from(duration) * u128::from(threshold)))
    .then_some(speaker)
}

fn labeled_text(items: &[Utterance]) -> Vec<(char, Option<SpeakerId>)> {
    items
        .iter()
        .flat_map(|item| item.text.chars().map(|c| (c, item.speaker)))
        .collect()
}

fuzz_target!(|input: Input| {
    let threshold = input.threshold % 10_001;
    let config = AssignmentConfig {
        min_overlap_ratio: OverlapRatio::from_basis_points(threshold).unwrap(),
    };
    let turns = input
        .turns
        .into_iter()
        .take(32)
        .map(|(span, speaker)| SpeakerTurn {
            span: span.span(),
            speaker: SpeakerId::new(speaker),
            confidence: None,
        })
        .collect::<Vec<_>>();
    let transcript = input
        .segments
        .into_iter()
        .take(8)
        .enumerate()
        .map(|(s, segment)| TranscribedSegment {
            span: segment.span.span(),
            text: format!("<segment:{s}:本文>"),
            confidence: None,
            tokens: segment
                .tokens
                .into_iter()
                .take(32)
                .enumerate()
                .map(|(t, span)| TimedToken {
                    span: span.span(),
                    text: format!("<{s}:{t}:本文>"),
                })
                .collect(),
        })
        .collect::<Vec<_>>();
    let mut expected = Vec::new();
    for segment in &transcript {
        let mut atoms = if segment.tokens.is_empty() {
            vec![TimedToken {
                span: segment.span,
                text: segment.text.clone(),
            }]
        } else {
            segment.tokens.clone()
        };
        atoms.sort_by_key(|atom| (atom.span.start(), atom.span.end()));
        expected.extend(atoms.into_iter().map(|atom| Utterance {
            speaker: expected_speaker(atom.span, &turns, threshold),
            span: atom.span,
            text: atom.text,
        }));
    }
    let assigned = assign_speakers(&transcript, &turns, &config);
    assert_eq!(labeled_text(&assigned), labeled_text(&expected));

    // Every output interval must be the hull of the tokens it contains.
    let mut atoms = expected.iter();
    for item in &assigned {
        let first = atoms.next().expect("output has an input atom");
        let mut text = first.text.clone();
        let mut end = first.span.end();
        while text.len() < item.text.len() {
            let next = atoms.next().expect("merged text has an input atom");
            text.push_str(&next.text);
            end = end.max(next.span.end());
        }
        assert_eq!(text, item.text);
        assert_eq!(item.span.start(), first.span.start());
        assert_eq!(item.span.end(), end);
    }
    assert!(atoms.next().is_none());
    let mut reversed = turns.clone();
    reversed.reverse();
    assert_eq!(assigned, assign_speakers(&transcript, &reversed, &config));
});
