use serde::Deserialize;
use yasumaro_core::{
    AssignmentConfig, Confidence, OverlapRatio, SpeakerId, SpeakerTurn, TimeSpan, TimedToken,
    Timestamp, TranscribedSegment, assign_speakers,
};

fn span(start: u64, end: u64) -> TimeSpan {
    TimeSpan::new(Timestamp::from_millis(start), Timestamp::from_millis(end))
        .expect("fixture span is ordered")
}

fn segment(start: u64, end: u64, text: &str) -> TranscribedSegment {
    TranscribedSegment {
        span: span(start, end),
        text: text.into(),
        confidence: Some(Confidence::new(0.8).expect("valid confidence")),
        tokens: Vec::new(),
    }
}

fn turn(start: u64, end: u64, speaker: u32) -> SpeakerTurn {
    SpeakerTurn {
        span: span(start, end),
        speaker: SpeakerId::new(speaker),
        confidence: None,
    }
}

#[test]
fn assigns_the_speaker_with_the_largest_overlap() {
    let transcript = vec![segment(0, 1_000, "確認します")];
    let turns = vec![turn(0, 400, 0), turn(400, 1_000, 1)];

    let result = assign_speakers(&transcript, &turns, &AssignmentConfig::default());

    assert_eq!(result[0].speaker, Some(SpeakerId::new(1)));
}

#[test]
fn equal_overlap_selects_the_smaller_speaker_id() {
    let transcript = vec![segment(0, 1_000, "同率")];
    let turns = vec![turn(0, 500, 9), turn(500, 1_000, 2)];

    let result = assign_speakers(&transcript, &turns, &AssignmentConfig::default());

    assert_eq!(result[0].speaker, Some(SpeakerId::new(2)));
}

#[test]
fn overlap_below_the_default_ratio_is_unknown() {
    let transcript = vec![segment(0, 1_000, "短い重複")];
    let turns = vec![turn(0, 100, 0)];

    let result = assign_speakers(&transcript, &turns, &AssignmentConfig::default());

    assert_eq!(result[0].speaker, None);
}

#[test]
fn timed_tokens_split_an_utterance_at_a_speaker_boundary() {
    let transcript = vec![TranscribedSegment {
        span: span(0, 1_000),
        text: "前半後半".into(),
        confidence: None,
        tokens: vec![
            TimedToken {
                span: span(0, 500),
                text: "前半".into(),
            },
            TimedToken {
                span: span(500, 1_000),
                text: "後半".into(),
            },
        ],
    }];
    let turns = vec![turn(0, 500, 0), turn(500, 1_000, 1)];

    let result = assign_speakers(&transcript, &turns, &AssignmentConfig::default());

    assert_eq!(result.len(), 2);
    assert_eq!(result[0].text, "前半");
    assert_eq!(result[0].speaker, Some(SpeakerId::new(0)));
    assert_eq!(result[1].text, "後半");
    assert_eq!(result[1].speaker, Some(SpeakerId::new(1)));
}

#[test]
fn adjacent_tokens_from_the_same_speaker_stay_in_one_utterance() {
    let transcript = vec![TranscribedSegment {
        span: span(0, 1_000),
        text: "前半後半".into(),
        confidence: None,
        tokens: vec![
            TimedToken {
                span: span(0, 500),
                text: "前半".into(),
            },
            TimedToken {
                span: span(500, 1_000),
                text: "後半".into(),
            },
        ],
    }];
    let turns = vec![turn(0, 1_000, 0)];

    let result = assign_speakers(&transcript, &turns, &AssignmentConfig::default());

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].text, "前半後半");
    assert_eq!(result[0].span, span(0, 1_000));
}

#[test]
fn token_input_order_does_not_change_chronological_output() {
    let transcript = vec![TranscribedSegment {
        span: span(0, 1_000),
        text: "前半後半".into(),
        confidence: None,
        tokens: vec![
            TimedToken {
                span: span(500, 1_000),
                text: "後半".into(),
            },
            TimedToken {
                span: span(0, 500),
                text: "前半".into(),
            },
        ],
    }];
    let turns = vec![turn(0, 1_000, 0)];

    let result = assign_speakers(&transcript, &turns, &AssignmentConfig::default());

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].text, "前半後半");
    assert_eq!(result[0].span, span(0, 1_000));
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LeanSpan {
    start_ms: u64,
    end_ms: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LeanSegment {
    speaker: u32,
    start_ms: u64,
    end_ms: u64,
}

#[derive(Deserialize)]
struct LeanThreshold {
    numerator: u32,
    denominator: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LeanVector {
    name: String,
    utterance: LeanSpan,
    segments: Vec<LeanSegment>,
    threshold: LeanThreshold,
    expected_speaker: Option<u32>,
}

#[test]
fn rust_assignment_matches_lean_generated_vectors() {
    let vectors: Vec<LeanVector> =
        serde_json::from_str(include_str!("fixtures/lean-speaker-assignment.json"))
            .expect("Lean vectors are valid JSON");

    for vector in vectors {
        let transcript = vec![segment(
            vector.utterance.start_ms,
            vector.utterance.end_ms,
            &vector.name,
        )];
        let turns = vector
            .segments
            .into_iter()
            .map(|item| turn(item.start_ms, item.end_ms, item.speaker))
            .collect::<Vec<_>>();
        let ratio =
            OverlapRatio::from_fraction(vector.threshold.numerator, vector.threshold.denominator)
                .expect("Lean threshold is valid");
        let config = AssignmentConfig {
            min_overlap_ratio: ratio,
        };

        let result = assign_speakers(&transcript, &turns, &config);
        assert_eq!(
            result[0].speaker.map(SpeakerId::as_u32),
            vector.expected_speaker,
            "Lean vector {}",
            vector.name
        );
    }
}
