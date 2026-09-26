use crate::{TimeSpan, Utterance};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NormalizationConfig {
    pub max_gap_ms: u64,
    pub max_chars: usize,
}

fn merged_span(left: TimeSpan, right: TimeSpan) -> TimeSpan {
    let end = left.end().max(right.end());
    TimeSpan::new(left.start(), end).expect("sorted utterance bounds remain ordered")
}

fn has_same_known_speaker(left: &Utterance, right: &Utterance) -> bool {
    left.speaker.is_some() && left.speaker == right.speaker
}

fn is_overlapping_duplicate(left: &Utterance, right: &Utterance) -> bool {
    has_same_known_speaker(left, right)
        && left.text == right.text
        && right.span.start() < left.span.end()
}

fn can_merge(left: &Utterance, right: &Utterance, config: &NormalizationConfig) -> bool {
    let gap = right
        .span
        .start()
        .as_millis()
        .saturating_sub(left.span.end().as_millis());
    has_same_known_speaker(left, right)
        && gap <= config.max_gap_ms
        && left.text.chars().count() + right.text.chars().count() <= config.max_chars
}

#[must_use]
pub fn normalize_utterances(
    mut utterances: Vec<Utterance>,
    config: &NormalizationConfig,
) -> Vec<Utterance> {
    utterances.sort_by_key(|utterance| (utterance.span.start(), utterance.span.end()));
    let mut normalized = Vec::<Utterance>::with_capacity(utterances.len());

    for next in utterances {
        if let Some(previous) = normalized.last_mut() {
            if is_overlapping_duplicate(previous, &next) {
                previous.span = merged_span(previous.span, next.span);
                continue;
            }
            if can_merge(previous, &next, config) {
                previous.span = merged_span(previous.span, next.span);
                previous.text.push_str(&next.text);
                continue;
            }
        }
        normalized.push(next);
    }

    normalized
}
