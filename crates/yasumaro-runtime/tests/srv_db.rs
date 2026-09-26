use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use yasumaro_core::SpeakerTurn;
use yasumaro_runtime::engine::{
    DiarizationRequest, Diarizer, SherpaDiarizer, Transcriber, TranscriptionRequest,
    WhisperProcessTranscriber,
};
use yasumaro_runtime::engine_artifact::{
    EngineManifest, EngineRootResolver, EngineStore, Platform,
};
use yasumaro_runtime::{ModelId, ModelRootResolver, ModelStore, decode_to_pcm};

const MANIFEST_FILE: &str = "evaluation.json";

#[derive(Deserialize)]
struct EvaluationManifest {
    cases: Vec<EvaluationCase>,
}

#[derive(Deserialize)]
struct EvaluationCase {
    id: String,
    dataset: u8,
    audio: PathBuf,
    reference: PathBuf,
    speech_rate_mora_per_second: f64,
    expected_speakers: u32,
}

#[derive(Serialize)]
struct EvaluationReport {
    status: &'static str,
    aggregate_cer: f64,
    cer_by_speech_rate: BTreeMap<String, f64>,
    cases: Vec<CaseMetrics>,
}

#[derive(Serialize)]
struct CaseMetrics {
    id: String,
    dataset: u8,
    speech_rate_mora_per_second: f64,
    cer: f64,
    detected_speakers: usize,
    expected_speakers: u32,
    label_consistency: Option<f64>,
    audio_seconds: f64,
    elapsed_seconds: f64,
    real_time_factor: f64,
    peak_memory_bytes: Option<u64>,
}

struct MeasuredCase {
    metrics: CaseMetrics,
    edits: usize,
    reference_chars: usize,
}

#[test]
#[ignore = "requires local SRV-DB audio, references, engine, and models"]
#[allow(
    clippy::print_stdout,
    reason = "the evaluation contract emits metrics JSON to stdout"
)]
fn evaluates_local_srv_db_without_copying_corpus_data() {
    let Some(root) = std::env::var_os("SRV_DB_DIR").map(PathBuf::from) else {
        println!(r#"{{"status":"skipped","reason":"SRV_DB_DIR is not set"}}"#);
        return;
    };
    let manifest_path = root.join(MANIFEST_FILE);
    let manifest: EvaluationManifest =
        serde_json::from_slice(&fs::read(&manifest_path).expect("read SRV-DB evaluation manifest"))
            .expect("parse SRV-DB evaluation manifest");
    assert!(
        !manifest.cases.is_empty(),
        "evaluation manifest has no cases"
    );

    let canonical_root = root.canonicalize().expect("canonicalize SRV_DB_DIR");
    let engine_manifest = EngineManifest::embedded().expect("load engine manifest");
    let engine_spec = engine_manifest
        .select(Platform::current().expect("supported evaluation platform"))
        .expect("published engine for evaluation platform")
        .clone();
    let engine_store =
        EngineStore::new(EngineRootResolver::resolve().expect("resolve engine root"));
    let model_store = ModelStore::new(ModelRootResolver::resolve().expect("resolve model root"));
    let segmentation = model_store
        .require(ModelId::SpeakerSegmentation)
        .expect("installed speaker segmentation model");
    let embedding = model_store
        .require(ModelId::SpeakerEmbedding)
        .expect("installed speaker embedding model");
    let threads = std::thread::available_parallelism().map_or(1, usize::from);
    let temporary = tempfile::tempdir().expect("evaluation temporary directory");

    let measured = manifest
        .cases
        .into_iter()
        .map(|case| {
            measure_case(
                &canonical_root,
                case,
                &engine_store,
                &engine_spec,
                &model_store,
                &segmentation,
                &embedding,
                temporary.path(),
                threads,
            )
        })
        .collect::<Vec<_>>();
    let report = aggregate(measured);
    println!(
        "{}",
        serde_json::to_string(&report).expect("serialize evaluation metrics")
    );
}

#[allow(
    clippy::too_many_arguments,
    reason = "the evaluation boundary names every local service explicitly"
)]
fn measure_case(
    root: &Path,
    case: EvaluationCase,
    engine_store: &EngineStore,
    engine_spec: &yasumaro_runtime::engine_artifact::EngineSpec,
    model_store: &ModelStore,
    segmentation: &Path,
    embedding: &Path,
    temporary: &Path,
    threads: usize,
) -> MeasuredCase {
    validate_case(&case);
    let started = Instant::now();
    let audio = resolve_local(root, &case.audio);
    let reference_path = resolve_local(root, &case.reference);
    let reference = fs::read_to_string(reference_path).expect("read local reference text");
    let case_temp = tempfile::tempdir_in(temporary).expect("case temporary directory");
    let pcm = decode_to_pcm(&audio, case_temp.path()).expect("decode SRV-DB audio");
    let sample_count = u32::try_from(pcm.samples().len()).expect("audio exceeds evaluation limit");
    let audio_seconds = f64::from(sample_count) / 16_000.0;
    assert!(audio_seconds > 0.0, "evaluation audio is empty");

    let lease = engine_store
        .require(engine_spec)
        .expect("installed whisper engine")
        .acquire()
        .expect("whisper engine lease");
    let whisper_model = model_store
        .require(ModelId::WhisperBase)
        .expect("installed whisper base model");
    let transcriber =
        WhisperProcessTranscriber::new(lease, whisper_model, case_temp.path().to_path_buf())
            .expect("initialize whisper process adapter");
    let diarizer =
        SherpaDiarizer::new(segmentation, embedding, threads).expect("initialize speaker diarizer");
    let segments = transcriber
        .transcribe(
            pcm.samples(),
            &TranscriptionRequest {
                prompt: None,
                threads,
                cancelled: Arc::new(AtomicBool::new(false)),
            },
        )
        .expect("transcribe SRV-DB audio");
    let turns = diarizer
        .diarize(pcm.samples(), &DiarizationRequest { num_speakers: None })
        .expect("diarize SRV-DB audio");
    let elapsed_seconds = started.elapsed().as_secs_f64();
    let hypothesis = segments
        .iter()
        .map(|segment| segment.text.as_str())
        .collect::<String>();
    let normalized_reference = normalize_for_cer(&reference);
    let normalized_hypothesis = normalize_for_cer(&hypothesis);
    let edits = edit_distance(&normalized_reference, &normalized_hypothesis);
    let reference_chars = normalized_reference.len();
    assert!(reference_chars > 0, "normalized reference text is empty");
    let detected_speakers = turns
        .iter()
        .map(|turn| turn.speaker.as_u32())
        .collect::<BTreeSet<_>>()
        .len();

    MeasuredCase {
        metrics: CaseMetrics {
            id: case.id,
            dataset: case.dataset,
            speech_rate_mora_per_second: case.speech_rate_mora_per_second,
            cer: ratio_usize(edits, reference_chars),
            detected_speakers,
            expected_speakers: case.expected_speakers,
            label_consistency: (case.dataset == 5).then(|| dominant_label_ratio(&turns)),
            audio_seconds,
            elapsed_seconds,
            real_time_factor: elapsed_seconds / audio_seconds,
            peak_memory_bytes: peak_memory_bytes(),
        },
        edits,
        reference_chars,
    }
}

fn validate_case(case: &EvaluationCase) {
    assert!(!case.id.trim().is_empty(), "evaluation case ID is empty");
    assert!(
        matches!(case.dataset, 4 | 5),
        "only SRV-DB datasets 4 and 5 are accepted"
    );
    assert!(
        case.speech_rate_mora_per_second.is_finite() && case.speech_rate_mora_per_second > 0.0,
        "speech rate must be finite and positive"
    );
    assert!(case.expected_speakers > 0, "expected speaker count is zero");
    for path in [&case.audio, &case.reference] {
        assert!(!path.is_absolute(), "manifest paths must be relative");
        assert!(
            path.components()
                .all(|component| matches!(component, Component::Normal(_))),
            "manifest paths must not traverse directories"
        );
    }
}

fn resolve_local(root: &Path, relative: &Path) -> PathBuf {
    let path = root
        .join(relative)
        .canonicalize()
        .expect("canonicalize corpus file");
    assert!(path.starts_with(root), "corpus file escapes SRV_DB_DIR");
    assert!(path.is_file(), "corpus path is not a file");
    path
}

fn normalize_for_cer(text: &str) -> Vec<char> {
    text.chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn edit_distance(reference: &[char], hypothesis: &[char]) -> usize {
    let mut previous = (0..=hypothesis.len()).collect::<Vec<_>>();
    let mut current = vec![0; hypothesis.len() + 1];
    for (reference_index, reference_character) in reference.iter().enumerate() {
        current[0] = reference_index + 1;
        for (hypothesis_index, hypothesis_character) in hypothesis.iter().enumerate() {
            let substitution = previous[hypothesis_index]
                + usize::from(reference_character != hypothesis_character);
            current[hypothesis_index + 1] = (current[hypothesis_index] + 1)
                .min(previous[hypothesis_index + 1] + 1)
                .min(substitution);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[hypothesis.len()]
}

fn dominant_label_ratio(turns: &[SpeakerTurn]) -> f64 {
    let mut durations = BTreeMap::<u32, u64>::new();
    for turn in turns {
        *durations.entry(turn.speaker.as_u32()).or_default() += turn.span.duration_ms();
    }
    let total = durations.values().sum::<u64>();
    if total == 0 {
        return 0.0;
    }
    ratio_u64(durations.values().copied().max().unwrap_or(0), total)
}

fn aggregate(measured: Vec<MeasuredCase>) -> EvaluationReport {
    let total_edits = measured.iter().map(|case| case.edits).sum::<usize>();
    let total_reference = measured
        .iter()
        .map(|case| case.reference_chars)
        .sum::<usize>();
    let mut rates = BTreeMap::<String, (usize, usize)>::new();
    for case in &measured {
        let key = format!("{:.2}", case.metrics.speech_rate_mora_per_second);
        let entry = rates.entry(key).or_default();
        entry.0 += case.edits;
        entry.1 += case.reference_chars;
    }
    EvaluationReport {
        status: "ok",
        aggregate_cer: ratio_usize(total_edits, total_reference),
        cer_by_speech_rate: rates
            .into_iter()
            .map(|(rate, (edits, reference))| (rate, ratio_usize(edits, reference)))
            .collect(),
        cases: measured.into_iter().map(|case| case.metrics).collect(),
    }
}

#[cfg(target_os = "windows")]
fn peak_memory_bytes() -> Option<u64> {
    let output = std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!("(Get-Process -Id {}).PeakWorkingSet64", std::process::id()),
        ])
        .output()
        .ok()?;
    output.status.success().then_some(())?;
    String::from_utf8(output.stdout).ok()?.trim().parse().ok()
}

fn ratio_usize(numerator: usize, denominator: usize) -> f64 {
    let numerator = u32::try_from(numerator).expect("evaluation character count exceeds u32");
    let denominator = u32::try_from(denominator).expect("evaluation character count exceeds u32");
    f64::from(numerator) / f64::from(denominator)
}

fn ratio_u64(numerator: u64, denominator: u64) -> f64 {
    let numerator = u32::try_from(numerator).expect("evaluation duration exceeds u32 milliseconds");
    let denominator =
        u32::try_from(denominator).expect("evaluation duration exceeds u32 milliseconds");
    f64::from(numerator) / f64::from(denominator)
}

#[cfg(target_os = "linux")]
fn peak_memory_bytes() -> Option<u64> {
    fs::read_to_string("/proc/self/status")
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))?
        .split_whitespace()
        .next()?
        .parse::<u64>()
        .ok()?
        .checked_mul(1024)
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
const fn peak_memory_bytes() -> Option<u64> {
    None
}

#[test]
fn character_error_rate_helpers_handle_japanese_and_empty_turns() {
    let reference = normalize_for_cer("今日は 晴れ");
    let hypothesis = normalize_for_cer("今日は雨");
    assert_eq!(reference, ['今', '日', 'は', '晴', 'れ']);
    assert_eq!(edit_distance(&reference, &hypothesis), 2);
    assert!(dominant_label_ratio(&[]).abs() < f64::EPSILON);
}
