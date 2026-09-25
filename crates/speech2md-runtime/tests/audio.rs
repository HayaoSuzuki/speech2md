use std::path::{Path, PathBuf};

use speech2md_runtime::{RuntimeError, decode_to_pcm};
use tempfile::TempDir;

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
