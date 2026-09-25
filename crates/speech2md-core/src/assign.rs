use thiserror::Error;

use crate::{SpeakerId, SpeakerTurn, TimeSpan, TranscribedSegment, Utterance};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OverlapRatio {
    numerator: u32,
    denominator: u32,
}

impl OverlapRatio {
    pub fn from_basis_points(value: u16) -> Result<Self, InvalidOverlapRatio> {
        Self::from_fraction(u32::from(value), 10_000_u32)
    }

    pub fn from_fraction(numerator: u32, denominator: u32) -> Result<Self, InvalidOverlapRatio> {
        (denominator > 0 && numerator <= denominator)
            .then_some(Self {
                numerator,
                denominator,
            })
            .ok_or(InvalidOverlapRatio)
    }

    #[must_use]
    pub fn is_met_by(self, overlap_ms: u64, duration_ms: u64) -> bool {
        self.numerator == 0
            || (duration_ms > 0
                && u128::from(duration_ms) * u128::from(self.numerator)
                    <= u128::from(overlap_ms) * u128::from(self.denominator))
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
#[error("overlap ratio must be between zero and one and have a nonzero denominator")]
pub struct InvalidOverlapRatio;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AssignmentConfig {
    pub min_overlap_ratio: OverlapRatio,
}

impl Default for AssignmentConfig {
    fn default() -> Self {
        Self {
            min_overlap_ratio: OverlapRatio::from_basis_points(2_000)
                .expect("the default overlap ratio is valid"),
        }
    }
}

#[must_use]
pub fn overlap_ms(left: TimeSpan, right: TimeSpan) -> u64 {
    left.end()
        .as_millis()
        .min(right.end().as_millis())
        .saturating_sub(left.start().as_millis().max(right.start().as_millis()))
}

fn assigned_speaker(
    span: TimeSpan,
    turns: &[SpeakerTurn],
    config: &AssignmentConfig,
) -> Option<SpeakerId> {
    let (speaker, overlap) = turns
        .iter()
        .map(|turn| (turn.speaker, overlap_ms(span, turn.span)))
        .max_by(
            |(left_speaker, left_overlap), (right_speaker, right_overlap)| {
                left_overlap
                    .cmp(right_overlap)
                    .then_with(|| right_speaker.cmp(left_speaker))
            },
        )?;

    config
        .min_overlap_ratio
        .is_met_by(overlap, span.duration_ms())
        .then_some(speaker)
}

fn utterance(
    span: TimeSpan,
    text: String,
    turns: &[SpeakerTurn],
    config: &AssignmentConfig,
) -> Utterance {
    Utterance {
        span,
        speaker: assigned_speaker(span, turns, config),
        text,
    }
}

fn assign_one(
    segment: &TranscribedSegment,
    turns: &[SpeakerTurn],
    config: &AssignmentConfig,
) -> Vec<Utterance> {
    if segment.tokens.is_empty() {
        return vec![utterance(segment.span, segment.text.clone(), turns, config)];
    }

    let mut tokens = segment.tokens.iter().collect::<Vec<_>>();
    tokens.sort_by_key(|token| (token.span.start(), token.span.end()));

    let mut assigned = Vec::<Utterance>::new();
    for token in tokens {
        let next = utterance(token.span, token.text.clone(), turns, config);
        if let Some(previous) = assigned.last_mut() {
            if previous.speaker == next.speaker && previous.span.end() <= next.span.end() {
                previous.span = TimeSpan::new(previous.span.start(), next.span.end())
                    .expect("merged token bounds remain ordered");
                previous.text.push_str(&next.text);
                continue;
            }
        }
        assigned.push(next);
    }
    assigned
}

#[must_use]
pub fn assign_speakers(
    transcript: &[TranscribedSegment],
    turns: &[SpeakerTurn],
    config: &AssignmentConfig,
) -> Vec<Utterance> {
    transcript
        .iter()
        .flat_map(|segment| assign_one(segment, turns, config))
        .collect()
}
