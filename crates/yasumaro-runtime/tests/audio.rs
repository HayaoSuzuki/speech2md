use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use proptest::prelude::*;
use tempfile::TempDir;
use yasumaro_runtime::{RuntimeError, decode_to_pcm};

#[derive(Clone, Default)]
struct CapturedLogs(Arc<Mutex<Vec<u8>>>);

impl Write for CapturedLogs {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("capture log lock").write(buffer)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl CapturedLogs {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().expect("capture log lock").clone()).expect("logs are UTF-8")
    }
}

fn captured_logs() -> &'static CapturedLogs {
    static LOGS: OnceLock<CapturedLogs> = OnceLock::new();
    LOGS.get_or_init(|| {
        let logs = CapturedLogs::default();
        let writer = logs.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .without_time()
            .with_writer(move || writer.clone())
            .finish();
        tracing::subscriber::set_global_default(subscriber).expect("install test subscriber");
        logs
    })
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn decodes_wav_mp3_and_m4a_to_equivalent_mono_16khz() {
    let temp_root = TempDir::new().expect("temporary root");
    let decoded = ["tone.wav", "tone.mp3", "tone.m4a"].map(|name| {
        decode_to_pcm(&fixture(name), temp_root.path()).expect("supported audio fixture")
    });

    assert!(
        decoded
            .iter()
            .all(|pcm| pcm.sample_rate() == 16_000 && pcm.channels() == 1)
    );
    let lengths = decoded.map(|pcm| pcm.samples().len());
    assert!(
        lengths
            .iter()
            .all(|length| (31_500..=32_500).contains(length)),
        "unexpected normalized lengths: {lengths:?}"
    );
    let shortest = lengths.iter().min().copied().expect("three lengths");
    let longest = lengths.iter().max().copied().expect("three lengths");
    // AAC uses 1,024-frame packets, so container duration may exceed the source
    // by a small codec boundary even after declared padding is removed.
    assert!(
        longest - shortest <= 512,
        "normalized lengths differ too much: {lengths:?}"
    );
}

#[test]
fn probes_content_instead_of_trusting_the_extension() {
    let temp_root = TempDir::new().expect("temporary root");
    let disguised = temp_root.path().join("actually-mp3.wav");
    std::fs::copy(fixture("tone.mp3"), &disguised).expect("copy fixture");

    let decoded = decode_to_pcm(&disguised, temp_root.path()).expect("probe MP3 content");

    assert_eq!(decoded.sample_rate(), 16_000);
    assert!(!decoded.samples().is_empty());
}

#[test]
fn rejects_a_container_without_an_audio_stream() {
    let temp_root = TempDir::new().expect("temporary root");

    let result = decode_to_pcm(&fixture("no-audio.mp4"), temp_root.path());

    assert!(matches!(result, Err(RuntimeError::NoAudioStream)));
}

#[test]
fn preserves_silence_as_nonempty_pcm() {
    let temp_root = TempDir::new().expect("temporary root");

    let decoded =
        decode_to_pcm(&fixture("silent.wav"), temp_root.path()).expect("valid silent audio");

    assert!(!decoded.samples().is_empty());
    assert!(
        decoded
            .samples()
            .iter()
            .all(|sample| sample.abs() <= f32::EPSILON)
    );
}

#[test]
fn emits_decode_lifecycle_without_exposing_the_input_path() {
    let logs = captured_logs();
    let temp_root = TempDir::new().expect("temporary root");

    decode_to_pcm(&fixture("tone.wav"), temp_root.path()).expect("decode fixture");

    let output = logs.text();
    assert!(output.contains("audio decode started"));
    assert!(output.contains("audio decode completed"));
    assert!(!output.contains("tone.wav"));
    assert!(!output.contains("yasumaro-runtime"));
}

fn pcm_frames() -> impl Strategy<Value = Vec<(i16, i16)>> {
    prop_oneof![
        prop::sample::select(vec![1_usize, 63, 64, 65, 4095, 4096, 4097]),
        1_usize..5000,
    ]
    .prop_flat_map(|length| prop::collection::vec((any::<i16>(), any::<i16>()), length))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn generated_integer_wav_preserves_frame_count_and_mono_samples(
        frames in pcm_frames(),
        stereo in any::<bool>(),
        rate in prop::sample::select(vec![8_000_u32, 16_000, 22_050, 32_000, 44_100, 48_000]),
    ) {
        let root = TempDir::new().expect("temporary audio directory");
        let path = root.path().join("generated.wav");
        // Every generated waveform exercises exact sample preservation as well
        // as the selected rate's length and finite-output contracts.
        for sample_rate in [16_000, rate] {
        let mut writer = hound::WavWriter::create(&path, hound::WavSpec {
            channels: if stereo { 2 } else { 1 }, sample_rate, bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        }).expect("create generated WAV");
        for &(left, right) in &frames {
            writer.write_sample(left).expect("write left channel");
            if stereo {
                writer.write_sample(right).expect("write right channel");
            }
        }
        writer.finalize().expect("finalize WAV");
        let pcm = decode_to_pcm(&path, root.path()).expect("valid generated WAV");
        let expected_length = (frames.len() * 16_000).div_ceil(usize::try_from(sample_rate).expect("sample rate fits"));
        prop_assert_eq!(pcm.samples().len(), expected_length);
        prop_assert!(pcm.samples().iter().all(|sample| sample.is_finite()));
        if sample_rate == 16_000 {
            for (&(left, right), actual) in frames.iter().zip(pcm.samples()) {
                let expected = if stereo {
                    (f32::from(left) + f32::from(right)) / 65_536.0
                } else {
                    f32::from(left) / 32_768.0
                };
                prop_assert_eq!(actual.to_bits(), expected.to_bits());
            }
        }
        }
    }
}
