#![no_main]

use libfuzzer_sys::fuzz_target;
use yasumaro_core::{
    AssignmentConfig, SpeakerId, SpeakerTurn, TimeSpan, TimedToken, Timestamp, TranscribedSegment,
    assign_speakers,
};

fuzz_target!(|data: &[u8]| {
    let midpoint = data.len() / 2;
    let tokens = data[..midpoint]
        .chunks_exact(5)
        .take(64)
        .enumerate()
        .map(|(index, chunk)| {
            let start = u64::from(u16::from_le_bytes([chunk[0], chunk[1]]));
            let duration = u64::from(u16::from_le_bytes([chunk[2], chunk[3]])) + 1;
            TimedToken {
                span: span(start, start + duration),
                text: format!("<{index}:{}>", chunk[4]),
            }
        })
        .collect::<Vec<_>>();
    let turns = data[midpoint..]
        .chunks_exact(5)
        .take(64)
        .map(|chunk| {
            let start = u64::from(u16::from_le_bytes([chunk[0], chunk[1]]));
            let duration = u64::from(u16::from_le_bytes([chunk[2], chunk[3]])) + 1;
            SpeakerTurn {
                span: span(start, start + duration),
                speaker: SpeakerId::new(u32::from(chunk[4])),
                confidence: None,
            }
        })
        .collect::<Vec<_>>();
    let mut sorted_tokens = tokens.clone();
    sorted_tokens.sort_by_key(|token| (token.span.start(), token.span.end()));
    let expected_text = sorted_tokens
        .iter()
        .map(|token| token.text.as_str())
        .collect::<String>();
    let transcript = [TranscribedSegment {
        span: span(0, u64::from(u16::MAX) * 2),
        text: expected_text.clone(),
        confidence: None,
        tokens,
    }];

    let assigned = assign_speakers(&transcript, &turns, &AssignmentConfig::default());

    assert_eq!(
        assigned
            .iter()
            .map(|utterance| utterance.text.as_str())
            .collect::<String>(),
        expected_text
    );
    assert!(
        assigned
            .windows(2)
            .all(|pair| pair[0].span.start() <= pair[1].span.start())
    );
});

fn span(start_ms: u64, end_ms: u64) -> TimeSpan {
    TimeSpan::new(
        Timestamp::from_millis(start_ms),
        Timestamp::from_millis(end_ms),
    )
    .expect("fuzz input constructs ordered spans")
}
