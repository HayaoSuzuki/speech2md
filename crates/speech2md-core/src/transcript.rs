use crate::{Confidence, TimeSpan};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SpeakerId(u32);

impl SpeakerId {
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimedToken {
    pub span: TimeSpan,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TranscribedSegment {
    pub span: TimeSpan,
    pub text: String,
    pub confidence: Option<Confidence>,
    pub tokens: Vec<TimedToken>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SpeakerTurn {
    pub span: TimeSpan,
    pub speaker: SpeakerId,
    pub confidence: Option<Confidence>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Utterance {
    pub span: TimeSpan,
    pub speaker: Option<SpeakerId>,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TranscriptDocument {
    pub title: String,
    pub utterances: Vec<Utterance>,
}

#[cfg(test)]
mod tests {
    use super::{
        SpeakerId, SpeakerTurn, TimedToken, TranscribedSegment, TranscriptDocument, Utterance,
    };
    use crate::{Confidence, TimeSpan, Timestamp};

    fn span(start: u64, end: u64) -> TimeSpan {
        TimeSpan::new(Timestamp::from_millis(start), Timestamp::from_millis(end))
            .expect("test span is ordered")
    }

    #[test]
    fn transcript_values_preserve_engine_independent_data() {
        let token = TimedToken {
            span: span(0, 100),
            text: "Rust".into(),
        };
        let segment = TranscribedSegment {
            span: span(0, 100),
            text: "Rust".into(),
            confidence: Some(Confidence::new(0.75).expect("valid probability")),
            tokens: vec![token.clone()],
        };

        assert_eq!(segment.tokens, vec![token]);
        assert_eq!(segment.text, "Rust");
    }

    #[test]
    fn speaker_confidence_can_remain_unknown() {
        let turn = SpeakerTurn {
            span: span(0, 100),
            speaker: SpeakerId::new(2),
            confidence: None,
        };

        assert_eq!(turn.speaker.as_u32(), 2);
        assert_eq!(turn.confidence, None);
    }

    #[test]
    fn document_owns_renderable_utterances() {
        let utterance = Utterance {
            span: span(0, 100),
            speaker: None,
            text: "確認します。".into(),
        };
        let document = TranscriptDocument {
            title: "meeting".into(),
            utterances: vec![utterance.clone()],
        };

        assert_eq!(document.title, "meeting");
        assert_eq!(document.utterances, vec![utterance]);
    }
}
