#![no_main]

use libfuzzer_sys::fuzz_target;
use yasumaro_runtime::decode_to_pcm;

fuzz_target!(|data: &[u8]| {
    let root = tempfile::tempdir().expect("fuzz temporary directory");
    let input = root.path().join("input.media");
    std::fs::write(&input, data).expect("write fuzz input");

    if let Ok(pcm) = decode_to_pcm(&input, root.path()) {
        assert!(!pcm.samples().is_empty());
        assert_eq!(pcm.sample_rate(), 16_000);
        assert_eq!(pcm.channels(), 1);
        // Touch the whole mapped output. Raw float WAV may contain NaN/Inf;
        // finite-sample checks belong to the generated integer-WAV target.
        std::hint::black_box(pcm.samples().iter().fold(0.0_f32, |sum, value| sum + value));
    }
});
