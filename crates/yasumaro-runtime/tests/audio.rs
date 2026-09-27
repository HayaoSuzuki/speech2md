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

/// Replace the entry count of the single `stts` atom in an ISO-BMFF fixture.
fn with_stts_entry_count(name: &str, entry_count: u32) -> Vec<u8> {
    let mut bytes = std::fs::read(fixture(name)).expect("read fixture");
    let start = bytes
        .windows(4)
        .position(|window| window == b"stts")
        .expect("fixture has a time-to-sample atom");
    // The count follows the atom type and the version and flags field.
    let count = start + 8;
    bytes[count..count + 4].copy_from_slice(&entry_count.to_be_bytes());
    bytes
}

/// Replace the first variable sample size in the `stsz` atom of an ISO-BMFF fixture.
fn with_first_sample_size(name: &str, sample_size: u32) -> Vec<u8> {
    let mut bytes = std::fs::read(fixture(name)).expect("read fixture");
    let start = bytes
        .windows(4)
        .position(|window| window == b"stsz")
        .expect("fixture has a sample-size atom");
    // The first entry follows the atom type, version and flags, constant sample
    // size, and sample count.
    let first_entry = start + 16;
    bytes[first_entry..first_entry + 4].copy_from_slice(&sample_size.to_be_bytes());
    bytes
}

/// Prefix an atom that pushes the `ftyp` marker away from the start of the file.
fn with_leading_atom(bytes: &[u8]) -> Vec<u8> {
    let mut prefixed = 8_u32.to_be_bytes().to_vec();
    prefixed.extend_from_slice(b"free");
    prefixed.extend_from_slice(bytes);
    prefixed
}

/// Prefix a valid ID3v2.4 tag containing one frame-sized padding block.
fn with_leading_id3(bytes: &[u8]) -> Vec<u8> {
    let mut prefixed = b"ID3\x04\0\0\0\0\0\x0a".to_vec();
    prefixed.extend_from_slice(&[0; 10]);
    prefixed.extend_from_slice(bytes);
    prefixed
}

#[test]
fn rejects_an_mp4_sample_table_that_reaches_past_its_atom() {
    // Fuzzing `decode_audio` mutated the entry count of a one-entry `stts` atom,
    // which made symphonia 0.5.5 read the atoms that follow it and overflow the
    // `u64` duration it accumulates. Such input must stay a probing error.
    let temp_root = TempDir::new().expect("temporary root");
    for name in ["no-audio.mp4", "tone.m4a"] {
        let input = temp_root.path().join(name);
        std::fs::write(&input, with_stts_entry_count(name, 5_373_953)).expect("write input");
        // Symphonia searches its whole probe window for the marker, so the same
        // tables are reachable when another atom comes first.
        let prefixed = temp_root.path().join(format!("prefixed-{name}"));
        std::fs::write(
            &prefixed,
            with_leading_atom(&with_stts_entry_count(name, 5_373_953)),
        )
        .expect("write prefixed input");
        let after_id3 = temp_root.path().join(format!("id3-{name}"));
        std::fs::write(
            &after_id3,
            with_leading_id3(&with_stts_entry_count(name, 5_373_953)),
        )
        .expect("write ID3-prefixed input");

        for input in [&input, &prefixed, &after_id3] {
            let error = decode_to_pcm(input, temp_root.path())
                .map(|_| ())
                .expect_err("a sample table beyond its atom");

            assert!(
                matches!(&error, RuntimeError::Probe(_)),
                "unexpected error for {}: {error}",
                input.display()
            );
        }
    }
}

#[test]
fn rejects_an_mp4_sample_larger_than_the_source_without_allocating_its_declared_size() {
    // Fuzzing changed one `stsz` entry in the 33 KiB fixture to nearly 4 GiB.
    // Symphonia must discover the short source while reading, without reserving
    // the untrusted sample size up front.
    let temp_root = TempDir::new().expect("temporary root");
    let input = temp_root.path().join("oversized-sample.m4a");
    std::fs::write(&input, with_first_sample_size("tone.m4a", u32::MAX)).expect("write input");

    let error = decode_to_pcm(&input, temp_root.path())
        .map(|_| ())
        .expect_err("sample larger than its source");

    assert!(
        matches!(error, RuntimeError::Decode(_)),
        "unexpected error: {error}"
    );
}

#[test]
fn rejects_an_mp4_child_that_crosses_its_parent_boundary() {
    fn atom(kind: [u8; 4], payload: &[u8]) -> Vec<u8> {
        let size = u32::try_from(payload.len() + 8).expect("test atom fits in u32");
        let mut bytes = size.to_be_bytes().to_vec();
        bytes.extend_from_slice(&kind);
        bytes.extend_from_slice(payload);
        bytes
    }

    let mut stts_payload = vec![0, 0, 0, 0];
    stts_payload.extend_from_slice(&2_u32.to_be_bytes());
    for _ in 0..2 {
        stts_payload.extend_from_slice(&u32::MAX.to_be_bytes());
        stts_payload.extend_from_slice(&u32::MAX.to_be_bytes());
    }
    let stts = atom(*b"stts", &stts_payload);
    let minf = atom(*b"minf", &atom(*b"stbl", &stts));
    let hidden = atom(*b"trak", &atom(*b"mdia", &minf));
    let mut malformed_moov = 9_u32.to_be_bytes().to_vec();
    malformed_moov.extend_from_slice(b"moov");
    malformed_moov.extend(hidden);
    let mut bytes = atom(*b"ftyp", b"isom\0\0\x02\0isom");
    bytes.extend(malformed_moov);

    let temp_root = TempDir::new().expect("temporary root");
    let input = temp_root.path().join("crossed-parent.mp4");
    std::fs::write(&input, bytes).expect("write input");

    let error = decode_to_pcm(&input, temp_root.path())
        .map(|_| ())
        .expect_err("a child atom outside its parent must not decode");

    assert!(
        matches!(error, RuntimeError::Probe(_)),
        "unexpected error: {error}"
    );
}

#[test]
fn keeps_decoding_an_mp4_whose_sample_table_is_intact() {
    let temp_root = TempDir::new().expect("temporary root");
    let input = temp_root.path().join("tone.m4a");
    std::fs::write(&input, with_stts_entry_count("tone.m4a", 2)).expect("write input");

    let decoded = decode_to_pcm(&input, temp_root.path()).expect("unchanged sample table");

    assert_eq!(decoded.sample_rate(), 16_000);
}

#[test]
fn decodes_a_wav_whose_samples_spell_mp4_atoms() {
    // The probe selects the WAV demuxer on the leading `RIFF` marker, so samples
    // that happen to spell an `ftyp` atom and an oversized `stts` stay audio.
    let atoms: [u8; 24] = [
        0, 0, 0, 8, b'f', b't', b'y', b'p', 0, 0, 0, 16, b's', b't', b't', b's', 0, 0, 0, 0, 0, 0,
        0, 1,
    ];
    let samples = atoms
        .as_chunks::<2>()
        .0
        .iter()
        .map(|bytes| i16::from_le_bytes(*bytes))
        .collect::<Vec<_>>();
    let temp_root = TempDir::new().expect("temporary root");
    let input = temp_root.path().join("atom-shaped.wav");
    let mut writer = hound::WavWriter::create(
        &input,
        hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .expect("create WAV");
    for sample in &samples {
        writer.write_sample(*sample).expect("write sample");
    }
    writer.finalize().expect("finalize WAV");

    let decoded = decode_to_pcm(&input, temp_root.path()).expect("valid WAV with atom-shaped PCM");

    assert_eq!(decoded.samples().len(), samples.len());
}

#[cfg(unix)]
#[test]
fn decodes_an_input_that_cannot_seek() {
    // A FIFO reaches the decoder without seeking, so it is spooled before the
    // sample tables are checked.
    let temp_root = TempDir::new().expect("temporary root");
    let input = temp_root.path().join("input.fifo");
    let status = std::process::Command::new("mkfifo")
        .arg(&input)
        .status()
        .expect("run mkfifo");
    assert!(status.success(), "mkfifo failed");
    let audio = std::fs::read(fixture("tone.wav")).expect("read fixture");
    let writer = {
        let path = input.clone();
        std::thread::spawn(move || {
            std::fs::write(&path, &audio).expect("write into the FIFO");
        })
    };

    let decoded = decode_to_pcm(&input, temp_root.path()).expect("decode a FIFO");

    writer.join().expect("writer thread");
    assert_eq!(decoded.sample_rate(), 16_000);
    assert!(!decoded.samples().is_empty());
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
