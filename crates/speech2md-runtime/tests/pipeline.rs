#![cfg(feature = "test-support")]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use sha2::{Digest, Sha256};
use speech2md_core::{SpeakerId, SpeakerTurn, TimeSpan, Timestamp};
use speech2md_runtime::engine::{DiarizationRequest, Diarizer, EngineError};
use speech2md_runtime::engine_artifact::{EngineSpec, EngineStore, Platform};
use speech2md_runtime::{
    ModelId, ModelStore, RuntimeError, RuntimeServices, TranscribeOptions, run_transcription,
};
use tempfile::TempDir;
use url::Url;

const OWNER_MARKER: &str = ".speech2md-owner";

#[derive(Serialize)]
struct Control<'a> {
    mode: &'a str,
    capture: &'a Path,
}

struct FixedDiarizer {
    error: bool,
    capture: PathBuf,
}

impl Diarizer for FixedDiarizer {
    fn diarize(
        &self,
        _samples: &[f32],
        _request: &DiarizationRequest,
    ) -> Result<Vec<SpeakerTurn>, EngineError> {
        if !self.capture.is_file() {
            return Err(EngineError::InvalidConfig(
                "diarization ran before transcription".into(),
            ));
        }
        if self.error {
            return Err(EngineError::Diarization("fixture failure".into()));
        }
        Ok(vec![SpeakerTurn {
            span: TimeSpan::new(Timestamp::from_millis(0), Timestamp::from_millis(250))
                .expect("fixture span"),
            speaker: SpeakerId::new(0),
            confidence: None,
        }])
    }
}

struct Fixture {
    _root: TempDir,
    input: PathBuf,
    output: PathBuf,
    temp_root: PathBuf,
    capture: PathBuf,
    engine_spec: EngineSpec,
    engine_store: EngineStore,
    model_store: ModelStore,
}

impl Fixture {
    fn new(mode: &str) -> Self {
        let root = TempDir::new().expect("fixture root");
        let input = root.path().join("meeting.wav");
        let output = root.path().join("meeting.md");
        let temp_root = root.path().join("temp");
        let engine_root = root.path().join("engines");
        let model_root = root.path().join("models");
        fs::create_dir_all(&temp_root).expect("temp root");
        fs::create_dir_all(&model_root).expect("model root");
        write_wav(&input);

        let platform = Platform::current().expect("supported test platform");
        let executable_name = if cfg!(windows) {
            "fake_whisper.exe"
        } else {
            "fake_whisper"
        };
        let executable_path = PathBuf::from("bin").join(executable_name);
        let engine_spec = EngineSpec {
            version: "fake-v1".into(),
            platform,
            url: Url::parse("https://example.invalid/fake").expect("fixture URL"),
            size: 1,
            sha256: format!("{:x}", Sha256::digest(b"fake")),
            archive_name: if cfg!(windows) {
                "fake.zip".into()
            } else {
                "fake.tar.gz".into()
            },
            executable_path: executable_path.clone(),
        };
        let install_dir = engine_root
            .join(&engine_spec.version)
            .join(platform.to_string());
        fs::create_dir_all(install_dir.join("bin")).expect("engine directory");
        let installed_executable = install_dir.join(&executable_path);
        fs::copy(env!("CARGO_BIN_EXE_fake_whisper"), &installed_executable)
            .expect("copy fake engine");
        let executable_hash = format!(
            "{:x}",
            Sha256::digest(fs::read(&installed_executable).expect("read fake engine"))
        );
        fs::write(
            install_dir.join(".speech2md-integrity"),
            format!("{}\n{}\n", engine_spec.sha256, executable_hash),
        )
        .expect("engine receipt");

        let capture = root.path().join("capture.json");
        fs::write(
            model_root.join("ggml-base.bin"),
            serde_json::to_vec(&Control {
                mode,
                capture: &capture,
            })
            .expect("control JSON"),
        )
        .expect("fake whisper model");
        fs::write(model_root.join("segmentation-3-0.onnx"), b"fixture")
            .expect("segmentation model");
        fs::write(
            model_root.join("3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx"),
            b"fixture",
        )
        .expect("embedding model");

        Self {
            _root: root,
            input,
            output,
            temp_root,
            capture,
            engine_spec,
            engine_store: EngineStore::new(engine_root),
            model_store: ModelStore::new(model_root),
        }
    }

    fn services<'a>(&'a self, diarizer: &'a dyn Diarizer) -> RuntimeServices<'a> {
        RuntimeServices::new(
            &self.engine_store,
            &self.engine_spec,
            &self.model_store,
            diarizer,
            &self.temp_root,
        )
    }

    fn options(&self) -> TranscribeOptions {
        TranscribeOptions {
            input: self.input.clone(),
            output: self.output.clone(),
            title: "meeting".into(),
            whisper_model: ModelId::WhisperBase,
            speakers: Some(1),
            prompt: None,
            threads: 1,
            force: false,
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    fn diarizer(&self, error: bool) -> FixedDiarizer {
        FixedDiarizer {
            error,
            capture: self.capture.clone(),
        }
    }

    fn assert_temp_empty(&self) {
        assert_eq!(
            fs::read_dir(&self.temp_root)
                .expect("read temp root")
                .count(),
            0
        );
    }
}

fn write_wav(path: &Path) {
    let mut writer = hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .expect("create WAV");
    for _ in 0..1_600 {
        writer.write_sample(0_i16).expect("write WAV sample");
    }
    writer.finalize().expect("finalize WAV");
}

#[test]
fn writes_complete_commonmark_after_transcription_and_diarization() {
    let fixture = Fixture::new("success");
    let diarizer = fixture.diarizer(false);

    let report = run_transcription(&fixture.services(&diarizer), &fixture.options())
        .expect("pipeline succeeds");

    assert_eq!(report.segment_count, 1);
    assert_eq!(report.utterance_count, 1);
    assert_eq!(
        fs::read_to_string(&fixture.output).expect("read output"),
        "# meeting\n\n**Speaker 1**（00:00:00）\n\n&#32;テスト\n"
    );
    fixture.assert_temp_empty();
}

#[test]
fn diarizer_failure_leaves_no_output_or_pcm_temp() {
    let fixture = Fixture::new("success");
    let diarizer = fixture.diarizer(true);

    let result = run_transcription(&fixture.services(&diarizer), &fixture.options());

    assert!(matches!(result, Err(RuntimeError::Diarization(_))));
    assert!(!fixture.output.exists());
    fixture.assert_temp_empty();
}

#[test]
fn empty_transcription_is_rejected_without_an_output() {
    let fixture = Fixture::new("empty");
    let diarizer = fixture.diarizer(false);

    let result = run_transcription(&fixture.services(&diarizer), &fixture.options());

    assert!(matches!(result, Err(RuntimeError::EmptyTranscript)));
    assert!(!fixture.output.exists());
    fixture.assert_temp_empty();
}

#[test]
fn existing_output_is_preserved_without_force() {
    let fixture = Fixture::new("success");
    fs::write(&fixture.output, b"existing bytes").expect("existing output");
    let diarizer = fixture.diarizer(false);

    let result = run_transcription(&fixture.services(&diarizer), &fixture.options());

    assert!(matches!(result, Err(RuntimeError::OutputExists)));
    assert_eq!(
        fs::read(&fixture.output).expect("read existing"),
        b"existing bytes"
    );
    assert!(
        !fixture.capture.exists(),
        "output collision must fail before inference"
    );
    fixture.assert_temp_empty();
}

#[test]
fn force_replaces_an_existing_output_with_complete_bytes() {
    let fixture = Fixture::new("success");
    fs::write(&fixture.output, b"existing bytes").expect("existing output");
    let diarizer = fixture.diarizer(false);
    let mut options = fixture.options();
    options.force = true;

    run_transcription(&fixture.services(&diarizer), &options).expect("forced pipeline succeeds");

    assert_eq!(
        fs::read_to_string(&fixture.output).expect("read replacement"),
        "# meeting\n\n**Speaker 1**（00:00:00）\n\n&#32;テスト\n"
    );
    fixture.assert_temp_empty();
}

#[test]
fn startup_removes_only_old_owned_job_directories() {
    let fixture = Fixture::new("success");
    let old_owned = fixture.temp_root.join("speech2md-job-old-owned");
    let old_unowned = fixture.temp_root.join("speech2md-job-old-unowned");
    let fresh_owned = fixture.temp_root.join("speech2md-job-fresh-owned");
    for directory in [&old_owned, &old_unowned, &fresh_owned] {
        fs::create_dir(directory).expect("stale fixture directory");
    }
    fs::write(old_owned.join(OWNER_MARKER), b"speech2md\n").expect("old marker");
    fs::write(fresh_owned.join(OWNER_MARKER), b"speech2md\n").expect("fresh marker");
    let old_time = filetime::FileTime::from_unix_time(1, 0);
    filetime::set_file_mtime(&old_owned, old_time).expect("age old owned directory");
    filetime::set_file_mtime(&old_unowned, old_time).expect("age old unowned directory");
    let diarizer = fixture.diarizer(false);

    run_transcription(&fixture.services(&diarizer), &fixture.options()).expect("pipeline succeeds");

    assert!(!old_owned.exists());
    assert!(old_unowned.exists());
    assert!(fresh_owned.exists());
}

#[test]
fn pre_cancelled_run_does_not_read_input_or_create_output() {
    let fixture = Fixture::new("success");
    fs::remove_file(&fixture.input).expect("remove input to prove it is not read");
    let diarizer = fixture.diarizer(false);
    let options = fixture.options();
    options.cancelled.store(true, Ordering::Release);

    let result = run_transcription(&fixture.services(&diarizer), &options);

    assert!(matches!(result, Err(RuntimeError::Cancelled)));
    assert!(!fixture.output.exists());
    assert!(!fixture.capture.exists());
    fixture.assert_temp_empty();
}

#[test]
fn force_does_not_replace_a_directory_or_start_inference() {
    let fixture = Fixture::new("success");
    fs::create_dir(&fixture.output).expect("directory at output path");
    let diarizer = fixture.diarizer(false);
    let mut options = fixture.options();
    options.force = true;

    let result = run_transcription(&fixture.services(&diarizer), &options);

    assert!(matches!(result, Err(RuntimeError::Output(_))));
    assert!(fixture.output.is_dir());
    assert!(!fixture.capture.exists());
    fixture.assert_temp_empty();
}
