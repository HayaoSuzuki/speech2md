#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use serde_json::json;
use yasumaro_fuzz::SpanInput;
use yasumaro_runtime::engine::parse_whisper_json;

#[derive(Arbitrary, Debug)]
struct Input {
    invalid_kind: u8,
    segments: Vec<(SpanInput, Vec<u8>)>,
}

fuzz_target!(|input: Input| {
    let expected = input
        .segments
        .into_iter()
        .take(32)
        .map(|(span, text)| {
            (
                span.span(),
                String::from_utf8_lossy(&text[..text.len().min(1024)]).into_owned(),
            )
        })
        .collect::<Vec<_>>();
    let segments = expected
        .iter()
        .map(|(span, text)| {
            json!({
                "offsets": {"from": span.start().as_millis(), "to": span.end().as_millis()},
                "text": text,
            })
        })
        .collect::<Vec<_>>();
    let mut value = json!({"transcription": segments});
    let parsed =
        parse_whisper_json(&serde_json::to_vec(&value).unwrap()).expect("valid generated JSON");
    assert_eq!(parsed.len(), expected.len());
    for (actual, (span, text)) in parsed.iter().zip(&expected) {
        assert_eq!(actual.span, *span);
        assert_eq!(actual.text, *text);
        assert!(actual.tokens.is_empty());
        assert!(actual.confidence.is_none());
    }

    // Forward-compatible fields at each supported object level are ignored.
    value["future"] = json!([null, {"text": "ignored"}]);
    for segment in value["transcription"].as_array_mut().unwrap() {
        segment["future"] = json!({"nested": [1, 2, 3]});
        segment["offsets"]["future"] = json!(true);
    }
    assert_eq!(
        parse_whisper_json(&serde_json::to_vec(&value).unwrap()).unwrap(),
        parsed
    );

    let invalid = match input.invalid_kind % 8 {
        0 => r#"{"offsets":{"from":-1,"to":0},"text":"x"}"#,
        1 => r#"{"offsets":{"from":0,"to":-1},"text":"x"}"#,
        2 => r#"{"offsets":{"from":1,"to":0},"text":"x"}"#,
        3 => r#"{"offsets":{"from":0.5,"to":1},"text":"x"}"#,
        4 => r#"{"offsets":{"from":0,"to":18446744073709551616},"text":"x"}"#,
        5 => r#"{"offsets":{"from":"0","to":1},"text":"x"}"#,
        6 => r#"{"offsets":{"from":0},"text":"x"}"#,
        _ => r#"{"offsets":{"from":0,"to":1}}"#,
    };
    // One bad segment must reject the document even after valid segments.
    value["transcription"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::from_str(invalid).unwrap());
    assert!(parse_whisper_json(&serde_json::to_vec(&value).unwrap()).is_err());
});
