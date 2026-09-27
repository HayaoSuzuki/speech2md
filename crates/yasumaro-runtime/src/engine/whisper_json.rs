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

    fn timestamp() -> impl Strategy<Value = u64> {
        prop_oneof![
            Just(0),
            Just(1),
            Just(u64::MAX),
            Just(u64::MAX - 1),
            any::<u64>()
        ]
    }

    fn segment_values() -> impl Strategy<Value = Vec<(u64, u64, String)>> {
        proptest::collection::vec(
            (
                timestamp(),
                timestamp(),
                proptest::collection::vec(any::<char>(), 0..64),
            ),
            0..16,
        )
        .prop_map(|values| {
            values
                .into_iter()
                .map(|(start, duration, text)| {
                    (
                        start,
                        start.saturating_add(duration),
                        text.into_iter().collect(),
                    )
                })
                .collect()
        })
    }

    fn document(values: &[(u64, u64, String)]) -> serde_json::Value {
        serde_json::json!({"transcription": values.iter().map(|(start, end, text)| {
            serde_json::json!({"offsets": {"from": start, "to": end}, "text": text})
        }).collect::<Vec<_>>()})
    }

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
            expected in segment_values(),
        ) {
            let bytes = serde_json::to_vec(&document(&expected))
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
        fn unknown_fields_at_every_level_do_not_change_segments(
            values in segment_values(),
            extra in proptest::collection::vec(any::<char>(), 0..64),
        ) {
            let mut value = document(&values);
            let baseline = parse_whisper_json(&serde_json::to_vec(&value).expect("JSON serializes"))
                .expect("generated document is valid");
            let extra = serde_json::json!({"nested": [extra.into_iter().collect::<String>(), null, 42]});
            value["future"] = extra.clone();
            for segment in value["transcription"].as_array_mut().expect("array") {
                segment["future"] = extra.clone();
                segment["offsets"]["future"] = extra.clone();
            }
            let actual = parse_whisper_json(&serde_json::to_vec(&value).expect("JSON serializes"))
                .expect("unknown fields are accepted");
            prop_assert_eq!(actual, baseline);
        }

        #[test]
        fn one_invalid_segment_rejects_the_entire_document(
            values in segment_values(),
            position in any::<usize>(),
            magnitude in 1_u64..=u64::MAX,
        ) {
            let negative = format!("-{magnitude}");
            let overflow = (u128::from(u64::MAX) + u128::from(magnitude)).to_string();
            let reversed = format!(r#"{{"offsets":{{"from":{magnitude},"to":0}},"text":"x"}}"#);
            // Test every invalid numeric/type/missing-field class on every case,
            // at a generated position among otherwise-valid segments.
            let invalid = [
                format!(r#"{{"offsets":{{"from":{negative},"to":1}},"text":"x"}}"#),
                format!(r#"{{"offsets":{{"from":0,"to":{overflow}}},"text":"x"}}"#),
                reversed,
                r#"{"offsets":{"from":0.5,"to":1},"text":"x"}"#.into(),
                r#"{"offsets":{"from":0,"to":1.5},"text":"x"}"#.into(),
                r#"{"offsets":{"from":"0","to":1},"text":"x"}"#.into(),
                r#"{"offsets":{"from":null,"to":1},"text":"x"}"#.into(),
                r#"{"offsets":{"to":1},"text":"x"}"#.into(),
                r#"{"offsets":{"from":0,"to":1}}"#.into(),
            ];
            for bad_segment in invalid {
                let mut value = document(&values);
                value["transcription"].as_array_mut().expect("array").insert(
                    position % (values.len() + 1),
                    serde_json::from_str(&bad_segment).expect("syntactically valid JSON"),
                );
                prop_assert!(parse_whisper_json(&serde_json::to_vec(&value).expect("JSON serializes")).is_err());
            }
        }

        #[test]
        fn arbitrary_bytes_never_panic(bytes: Vec<u8>) {
            let _result = parse_whisper_json(&bytes);
        }
    }
}
