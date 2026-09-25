use speech2md_core::{
    NormalizationConfig, SpeakerId, TimeSpan, Timestamp, Utterance, normalize_utterances,
};

fn utterance(start: u64, end: u64, speaker: u32, text: &str) -> Utterance {
    Utterance {
        span: TimeSpan::new(Timestamp::from_millis(start), Timestamp::from_millis(end))
            .expect("fixture span is ordered"),
        speaker: Some(SpeakerId::new(speaker)),
        text: text.into(),
    }
}

fn config() -> NormalizationConfig {
    NormalizationConfig {
        max_gap_ms: 200,
        max_chars: 100,
    }
}

#[test]
fn merges_nearby_utterances_from_the_same_speaker() {
    let input = vec![
        utterance(0, 500, 0, "確認します。"),
        utterance(600, 900, 0, "次です。"),
    ];

    let result = normalize_utterances(input, &config());

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].text, "確認します。次です。");
    assert_eq!(result[0].span.end().as_millis(), 900);
}

#[test]
fn removes_only_an_adjacent_exact_window_duplicate() {
    let input = vec![
        utterance(0, 500, 0, "境界の文"),
        utterance(450, 900, 0, "境界の文"),
    ];

    let result = normalize_utterances(input, &config());

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].text, "境界の文");
    assert_eq!(result[0].span.end().as_millis(), 900);
}

#[test]
fn preserves_repetition_inside_one_utterance() {
    let result = normalize_utterances(vec![utterance(0, 500, 0, "はい、はい")], &config());

    assert_eq!(result[0].text, "はい、はい");
}

#[test]
fn does_not_merge_across_speaker_changes() {
    let input = vec![
        utterance(0, 500, 0, "一人目"),
        utterance(600, 900, 1, "二人目"),
    ];

    let result = normalize_utterances(input, &config());

    assert_eq!(result.len(), 2);
}

#[test]
fn does_not_treat_two_unknown_segments_as_the_same_speaker() {
    let mut first = utterance(0, 500, 0, "不明一");
    first.speaker = None;
    let mut second = utterance(600, 900, 0, "不明二");
    second.speaker = None;

    let result = normalize_utterances(vec![first, second], &config());

    assert_eq!(result.len(), 2);
}

#[test]
fn does_not_merge_when_combined_text_exceeds_the_limit() {
    let input = vec![utterance(0, 10, 0, "123"), utterance(10, 20, 0, "456")];
    let config = NormalizationConfig {
        max_gap_ms: 200,
        max_chars: 5,
    };

    let result = normalize_utterances(input, &config);

    assert_eq!(result.len(), 2);
}
