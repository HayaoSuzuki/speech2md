#![no_main]

use libfuzzer_sys::fuzz_target;
use yasumaro_runtime::decode_to_pcm;

fuzz_target!(|input: (u8, bool, Vec<i16>)| {
    let (rate_index, stereo, samples) = input;
    let rate = [8_000, 16_000, 22_050, 32_000, 44_100, 48_000][usize::from(rate_index) % 6];
    let channels = if stereo { 2 } else { 1 };
    let mut samples = samples.into_iter().take(16_384).collect::<Vec<_>>();
    samples.truncate(samples.len() / channels * channels);
    if samples.is_empty() {
        samples.resize(channels, 0);
    }
    let root = tempfile::tempdir().expect("fuzz temporary directory");
    let path = root.path().join("generated.wav");
    let mut writer = hound::WavWriter::create(
        &path,
        hound::WavSpec {
            channels: channels as u16,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for sample in &samples {
        writer.write_sample(*sample).unwrap();
    }
    writer.finalize().unwrap();
    let pcm = decode_to_pcm(&path, root.path()).expect("valid generated WAV");
    assert_eq!(
        pcm.samples().len(),
        (samples.len() / channels * 16_000).div_ceil(rate as usize)
    );
    assert!(pcm.samples().iter().all(|sample| sample.is_finite()));
    if rate == 16_000 {
        for (frame, actual) in samples.chunks_exact(channels).zip(pcm.samples()) {
            let expected = frame
                .iter()
                .map(|sample| f32::from(*sample) / 32768.0)
                .sum::<f32>()
                / channels as f32;
            assert_eq!(*actual, expected);
        }
    }
    if samples.iter().all(|sample| *sample == 0) {
        assert!(
            pcm.samples()
                .iter()
                .all(|sample| sample.abs() <= f32::EPSILON)
        );
    }
});
