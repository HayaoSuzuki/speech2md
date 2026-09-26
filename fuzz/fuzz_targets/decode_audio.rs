#![no_main]

use libfuzzer_sys::fuzz_target;
use yasumaro_runtime::decode_to_pcm;

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }

    let Ok(root) = tempfile::tempdir() else {
        return;
    };
    let input = root.path().join("input.media");
    if std::fs::write(&input, data).is_err() {
        return;
    }

    let _result = decode_to_pcm(&input, root.path());
});
