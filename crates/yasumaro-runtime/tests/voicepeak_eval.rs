use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::Serialize;
use yasumaro_runtime::engine::{DiarizationRequest, Diarizer, SherpaDiarizer};
use yasumaro_runtime::{ModelId, ModelRootResolver, ModelStore, decode_to_pcm};

const CASES: [(&str, u32); 10] = [
    ("single-female1.wav", 1),
    ("single-male1.wav", 1),
    ("single-male2.wav", 1),
    ("single-male3.wav", 1),
    ("pair-male1-male2.wav", 2),
    ("pair-male1-male3.wav", 2),
    ("pair-male2-male3.wav", 2),
    ("male-3speakers.wav", 3),
    ("balanced-4speakers.wav", 4),
    ("imbalanced-4speakers.wav", 4),
];

#[derive(Serialize)]
struct Report {
    status: &'static str,
    speaker_count_mismatches: usize,
    cases: Vec<CaseResult>,
}

#[derive(Serialize)]
struct CaseResult {
    file: &'static str,
    expected_speakers: u32,
    detected_speakers: usize,
    speaker_count_matches: bool,
    speaker_duration_ms: BTreeMap<u32, u64>,
    audio_seconds: f64,
    elapsed_seconds: f64,
    real_time_factor: f64,
}

#[test]
#[ignore = "requires local self-authored VOICEPEAK WAV files and speaker models"]
#[allow(
    clippy::print_stdout,
    reason = "the evaluation contract emits metrics JSON to stdout"
)]
fn evaluates_voicepeak_diarization_without_transcription() {
    let Some(root) = std::env::var_os("YASUMARO_VOICEPEAK_DIR").map(PathBuf::from) else {
        let skipped = serde_json::json!({
            "status": "skipped",
            "reason": "YASUMARO_VOICEPEAK_DIR is not set"
        });
        println!("{skipped}");
        return;
    };
    let root = root
        .canonicalize()
        .expect("canonicalize VOICEPEAK directory");
    let model_store = ModelStore::new(ModelRootResolver::resolve().expect("resolve model root"));
    let segmentation = model_store
        .acquire(ModelId::SpeakerSegmentation)
        .expect("installed speaker segmentation model");
    let embedding = model_store
        .acquire(ModelId::SpeakerEmbedding)
        .expect("installed speaker embedding model");
    let threads = std::thread::available_parallelism().map_or(1, usize::from);
    let diarizer =
        SherpaDiarizer::new(segmentation, embedding, threads).expect("initialize speaker diarizer");
    let temporary = tempfile::tempdir().expect("evaluation temporary directory");

    let cases = CASES
        .into_iter()
        .map(|(file, expected_speakers)| {
            evaluate_case(&root, temporary.path(), &diarizer, file, expected_speakers)
        })
        .collect::<Vec<_>>();
    let speaker_count_mismatches = cases
        .iter()
        .filter(|case| case.detected_speakers != case.expected_speakers as usize)
        .count();
    let report = Report {
        status: "ok",
        speaker_count_mismatches,
        cases,
    };
    println!(
        "{}",
        serde_json::to_string(&report).expect("serialize VOICEPEAK evaluation report")
    );
}

fn evaluate_case(
    root: &Path,
    temporary: &Path,
    diarizer: &SherpaDiarizer,
    file: &'static str,
    expected_speakers: u32,
) -> CaseResult {
    let audio = root
        .join(file)
        .canonicalize()
        .expect("canonicalize WAV file");
    assert!(
        audio.starts_with(root),
        "WAV file escapes evaluation directory"
    );
    assert!(audio.is_file(), "missing VOICEPEAK WAV file: {file}");
    let case_temp = tempfile::tempdir_in(temporary).expect("case temporary directory");
    let pcm = decode_to_pcm(&audio, case_temp.path()).expect("decode VOICEPEAK WAV");
    let sample_count = u32::try_from(pcm.samples().len()).expect("audio exceeds evaluation limit");
    let audio_seconds = f64::from(sample_count) / 16_000.0;
    let started = Instant::now();
    let turns = diarizer
        .diarize(
            pcm.samples(),
            &DiarizationRequest {
                num_speakers: Some(expected_speakers),
            },
        )
        .expect("diarize VOICEPEAK WAV");
    let elapsed_seconds = started.elapsed().as_secs_f64();
    assert!(!turns.is_empty(), "no speaker turns for {file}");
    assert!(turns.iter().all(|turn| turn.span.duration_ms() > 0));
    assert!(
        turns
            .windows(2)
            .all(|pair| pair[0].span.start() <= pair[1].span.start())
    );
    let mut speaker_duration_ms = BTreeMap::<u32, u64>::new();
    for turn in turns {
        *speaker_duration_ms
            .entry(turn.speaker.as_u32())
            .or_default() += turn.span.duration_ms();
    }

    CaseResult {
        file,
        expected_speakers,
        detected_speakers: speaker_duration_ms.len(),
        speaker_count_matches: speaker_duration_ms.len() == expected_speakers as usize,
        speaker_duration_ms,
        audio_seconds,
        elapsed_seconds,
        real_time_factor: elapsed_seconds / audio_seconds,
    }
}

#[test]
fn voicepeak_case_catalog_covers_single_pair_and_group_conditions() {
    let expected_counts = CASES.into_iter().fold(
        BTreeMap::<u32, usize>::new(),
        |mut counts, (_, speakers)| {
            *counts.entry(speakers).or_default() += 1;
            counts
        },
    );
    assert_eq!(
        expected_counts,
        BTreeMap::from([(1, 4), (2, 3), (3, 1), (4, 2)])
    );
}
