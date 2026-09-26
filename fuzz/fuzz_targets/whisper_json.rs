#![no_main]

use libfuzzer_sys::fuzz_target;
use yasumaro_runtime::engine::parse_whisper_json;

fuzz_target!(|data: &[u8]| {
    let _result = parse_whisper_json(data);
});
