use serde::Deserialize;
use serde_json::Number;
use yasumaro_core::{TimeSpan, Timestamp, TranscribedSegment};

use super::EngineError;

#[derive(Deserialize)]
struct WhisperOutput {
    transcription: Vec<WhisperSegment>,
}

#[derive(Deserialize)]
struct WhisperSegment {
    offsets: WhisperOffsets,
    text: String,
}

#[derive(Deserialize)]
struct WhisperOffsets {
    from: Number,
    to: Number,
}

/// Converts `whisper-cli` JSON output into engine-independent segments.
///
/// Unknown JSON fields are intentionally ignored for forward compatibility.
///
/// # Errors
///
/// Returns an error for malformed JSON, missing fields, negative/out-of-range
/// timestamps, or a segment whose end precedes its start.
pub fn parse_whisper_json(bytes: &[u8]) -> Result<Vec<TranscribedSegment>, EngineError> {
    let output: WhisperOutput = serde_json::from_slice(bytes)
        .map_err(|error| EngineError::InvalidJson(error.to_string()))?;
    output
        .transcription
        .into_iter()
        .enumerate()
        .map(|(index, segment)| {
            let start = segment.offsets.from.as_u64().ok_or_else(|| {
                EngineError::InvalidSegment(format!("segment {index} has an invalid start"))
            })?;
            let end = segment.offsets.to.as_u64().ok_or_else(|| {
                EngineError::InvalidSegment(format!("segment {index} has an invalid end"))
            })?;
            let span = TimeSpan::new(Timestamp::from_millis(start), Timestamp::from_millis(end))
                .map_err(|error| {
                    EngineError::InvalidSegment(format!("segment {index}: {error}"))
                })?;
            Ok(TranscribedSegment {
                span,
                text: segment.text,
                confidence: None,
                tokens: Vec::new(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::parse_whisper_json;

    #[test]
    fn parses_fixed_whisper_cli_fixture_into_half_open_milliseconds() {
        let segments =
            parse_whisper_json(include_bytes!("../../tests/fixtures/whisper-output.json"))
                .expect("fixture is valid");

        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].span.start().as_millis(), 120);
        assert_eq!(segments[0].span.end().as_millis(), 1_530);
        assert_eq!(segments[0].text, " Rustの設計を確認します。");
        assert_eq!(segments[1].span.start().as_millis(), 1_530);
        assert_eq!(segments[1].span.end().as_millis(), 2_000);
        assert_eq!(segments[1].text, " はい。");
    }

    #[test]
    fn rejects_negative_reversed_overflow_and_missing_offsets() {
        for invalid in [
            br#"{"transcription":[{"offsets":{"from":-1,"to":2},"text":"x"}]}"#.as_slice(),
            br#"{"transcription":[{"offsets":{"from":3,"to":2},"text":"x"}]}"#,
            br#"{"transcription":[{"offsets":{"from":18446744073709551616,"to":2},"text":"x"}]}"#,
            br#"{"transcription":[{"text":"x"}]}"#,
        ] {
            assert!(parse_whisper_json(invalid).is_err());
        }
    }

    #[test]
    fn accepts_nonempty_unknown_fields_and_an_empty_transcription() {
        let with_unknown = br#"{
            "systeminfo":"fixture",
            "transcription":[{
                "offsets":{"from":0,"to":1,"future":42},
                "text":"x",
                "tokens":[{"text":"x"}]
            }]
        }"#;
        assert_eq!(
            parse_whisper_json(with_unknown)
                .expect("unknown fields are forward compatible")
                .len(),
            1
        );
        assert!(
            parse_whisper_json(br#"{"transcription":[]}"#)
                .expect("an empty result is valid")
                .is_empty()
        );
    }

    proptest! {
        #[test]
        fn valid_segments_preserve_order_and_text(
            starts_and_lengths in proptest::collection::vec((0_u32..1_000_000, 0_u16..10_000), 0..32),
            texts in proptest::collection::vec(".*", 0..32),
        ) {
            let count = starts_and_lengths.len().min(texts.len());
            let values = starts_and_lengths.into_iter().zip(texts).take(count)
                .map(|((start, length), text)| serde_json::json!({
                    "offsets": {"from": start, "to": u64::from(start) + u64::from(length)},
                    "text": text,
                }))
                .collect::<Vec<_>>();
            let expected = values.iter().map(|value| {
                (
                    value["offsets"]["from"].as_u64().expect("generated integer"),
                    value["offsets"]["to"].as_u64().expect("generated integer"),
                    value["text"].as_str().expect("generated string").to_owned(),
                )
            }).collect::<Vec<_>>();
            let bytes = serde_json::to_vec(&serde_json::json!({"transcription": values}))
                .expect("generated JSON serializes");

            let parsed = parse_whisper_json(&bytes).expect("generated segments are valid");
            let actual = parsed.into_iter().map(|segment| (
                segment.span.start().as_millis(),
                segment.span.end().as_millis(),
                segment.text,
            )).collect::<Vec<_>>();
            prop_assert_eq!(actual, expected);
        }

        #[test]
        fn arbitrary_bytes_never_panic(bytes: Vec<u8>) {
            let _result = parse_whisper_json(&bytes);
        }
    }
}
