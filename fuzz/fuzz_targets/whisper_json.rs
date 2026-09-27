#![no_main]

use libfuzzer_sys::fuzz_target;
use yasumaro_runtime::engine::parse_whisper_json;

fuzz_target!(|data: &[u8]| {
    if let Ok(segments) = parse_whisper_json(data) {
        for segment in &segments {
            assert!(segment.span.start() <= segment.span.end());
            assert!(segment.tokens.is_empty());
            assert!(segment.confidence.is_none());
        }
        let canonical = serde_json::json!({"transcription": segments.iter().map(|segment| {
            serde_json::json!({
                "offsets": {"from": segment.span.start().as_millis(), "to": segment.span.end().as_millis()},
                "text": segment.text,
            })
        }).collect::<Vec<_>>()});
        assert_eq!(
            parse_whisper_json(&serde_json::to_vec(&canonical).unwrap()).unwrap(),
            segments
        );
    }
});
