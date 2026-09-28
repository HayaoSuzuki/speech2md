#![cfg(feature = "test-support")]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use url::Url;
use yasumaro_runtime::engine::{
    EngineError, Transcriber, TranscriptionRequest, WhisperProcessTranscriber,
};
use yasumaro_runtime::engine_artifact::{EngineSpec, EngineStore, Platform};
use yasumaro_runtime::{ModelError, ModelId, ModelStore};

const PROMPT_SENTINEL: &str = "機密-PROMPT-7e18b1";

#[derive(Serialize)]
struct Control<'a> {
    mode: &'a str,
    capture: &'a Path,
}

#[derive(Deserialize)]
struct Capture {
    args: Vec<String>,
    environment: BTreeMap<String, String>,
    prompt: String,
}

struct Fixture {
    _root: TempDir,
    temp_root: PathBuf,
    capture: PathBuf,
    spec: EngineSpec,
    store: EngineStore,
    model_store: ModelStore,
}

impl Fixture {
    fn new(mode: &str) -> Self {
        let root = TempDir::new().expect("temporary fixture root");
        let temp_root = root.path().join("temp");
        let engine_root = root.path().join("engines");
        fs::create_dir_all(&temp_root).expect("create temp root");
        let platform = Platform::current().expect("test platform is supported");
        let executable_name = if cfg!(windows) {
            "fake_whisper.exe"
        } else {
            "fake_whisper"
        };
        let executable_path = PathBuf::from("bin").join(executable_name);
        let spec = EngineSpec {
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
        let install_dir = engine_root.join(&spec.version).join(platform.to_string());
        fs::create_dir_all(install_dir.join("bin")).expect("create fake engine directory");
        let installed_executable = install_dir.join(&executable_path);
        fs::copy(env!("CARGO_BIN_EXE_fake_whisper"), &installed_executable)
            .expect("copy fake engine");
        let executable_hash = format!(
            "{:x}",
            Sha256::digest(fs::read(&installed_executable).expect("read fake engine"))
        );
        fs::write(
            install_dir.join(".yasumaro-integrity"),
            format!("{}\n{}\n", spec.sha256, executable_hash),
        )
        .expect("write fake engine integrity receipt");
        let capture = root.path().join("capture.json");
        let model_root = root.path().join("models");
        fs::create_dir_all(&model_root).expect("create model root");
        let model = model_root.join("ggml-base.bin");
        fs::write(
            &model,
            serde_json::to_vec(&Control {
                mode,
                capture: &capture,
            })
            .expect("serialize control"),
        )
        .expect("write control model");
        Self {
            _root: root,
            temp_root,
            capture,
            spec,
            store: EngineStore::new(engine_root),
            model_store: ModelStore::new(model_root),
        }
    }

    fn transcriber(&self) -> WhisperProcessTranscriber {
        let lease = self
            .store
            .require(&self.spec)
            .expect("fake engine installed")
            .acquire()
            .expect("lease fake engine");
        let model = self
            .model_store
            .acquire(ModelId::WhisperBase)
            .expect("lease fake model");
        WhisperProcessTranscriber::new(lease, model, self.temp_root.clone())
            .expect("construct transcriber")
    }

    fn test_transcriber(&self, available: u64, grace: Duration) -> WhisperProcessTranscriber {
        let lease = self
            .store
            .require(&self.spec)
            .expect("fake engine installed")
            .acquire()
            .expect("lease fake engine");
        let model = self
            .model_store
            .acquire(ModelId::WhisperBase)
            .expect("lease fake model");
        WhisperProcessTranscriber::new_for_test(
            lease,
            model,
            self.temp_root.clone(),
            available,
            grace,
        )
        .expect("construct test transcriber")
    }

    fn request(cancelled: Arc<AtomicBool>) -> TranscriptionRequest {
        TranscriptionRequest {
            prompt: Some(PROMPT_SENTINEL.into()),
            threads: 3,
            cancelled,
        }
    }

    fn capture(&self) -> Capture {
        serde_json::from_slice(&fs::read(&self.capture).expect("read capture"))
            .expect("parse capture")
    }

    fn assert_temp_empty(&self) {
        assert!(
            fs::read_dir(&self.temp_root)
                .expect("read temp root")
                .next()
                .is_none()
        );
    }
}

#[test]
fn whisper_transcriber_holds_the_model_lease_until_drop() {
    let fixture = Fixture::new("success");
    let engine = fixture
        .store
        .require(&fixture.spec)
        .expect("fake engine installed")
        .acquire()
        .expect("lease fake engine");
    let model = fixture
        .model_store
        .acquire(ModelId::WhisperBase)
        .expect("lease fake model");
    let transcriber = WhisperProcessTranscriber::new(engine, model, fixture.temp_root.clone())
        .expect("construct transcriber");

    assert_eq!(
        fixture.model_store.remove(ModelId::WhisperBase),
        Err(ModelError::ModelInUse {
            id: ModelId::WhisperBase,
        })
    );
    drop(transcriber);
    fixture
        .model_store
        .remove(ModelId::WhisperBase)
        .expect("remove model after transcriber drop");
}

#[test]
fn whisper_constructor_failure_releases_the_model_lease() {
    let fixture = Fixture::new("success");
    let engine = fixture
        .store
        .require(&fixture.spec)
        .expect("fake engine installed")
        .acquire()
        .expect("lease fake engine");
    let model = fixture
        .model_store
        .acquire(ModelId::WhisperBase)
        .expect("lease fake model");

    assert!(matches!(
        WhisperProcessTranscriber::new(engine, model, fixture.temp_root.join("missing")),
        Err(EngineError::Configuration(_))
    ));
    fixture
        .model_store
        .remove(ModelId::WhisperBase)
        .expect("remove model after constructor failure");
}

#[test]
fn passes_the_whisper_contract_without_exposing_prompt_text() {
    let fixture = Fixture::new("success");
    let segments = fixture
        .transcriber()
        .transcribe(
            &[0.0; 160],
            &Fixture::request(Arc::new(AtomicBool::new(false))),
        )
        .expect("fake transcription succeeds");
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].text, " テスト");

    let capture = fixture.capture();
    let joined = capture.args.join(" ");
    assert!(joined.contains("--language ja"));
    assert!(joined.contains("--output-json"));
    assert!(joined.contains("--threads 3"));
    assert!(joined.contains("--model"));
    assert!(joined.contains("--file"));
    assert!(joined.contains("--output-file"));
    assert!(joined.contains("--prompt-file"));
    assert!(!joined.contains("--translate"));
    assert!(!joined.contains(PROMPT_SENTINEL));
    assert!(
        !capture
            .environment
            .values()
            .any(|value| value.contains(PROMPT_SENTINEL))
    );
    assert_eq!(capture.prompt, PROMPT_SENTINEL);
    fixture.assert_temp_empty();
}

#[test]
fn classifies_nonzero_missing_and_invalid_output_without_child_text() {
    for (mode, expected) in [
        ("nonzero", "process exit"),
        ("missing", "missing output"),
        ("invalid", "invalid whisper JSON"),
    ] {
        let fixture = Fixture::new(mode);
        let error = fixture
            .transcriber()
            .transcribe(
                &[0.0; 16],
                &Fixture::request(Arc::new(AtomicBool::new(false))),
            )
            .expect_err("mode fails");
        let rendered = error.to_string();
        assert!(rendered.contains(expected), "{rendered}");
        assert!(!rendered.contains(PROMPT_SENTINEL));
        assert!(rendered.len() < 1_024);
        if mode == "nonzero" {
            assert!(matches!(
                error,
                EngineError::ProcessExit {
                    stdout_bytes: 65_536,
                    stdout_truncated: true,
                    stderr_bytes: 65_536,
                    stderr_truncated: true,
                    ..
                }
            ));
        }
        fixture.assert_temp_empty();
    }
}

#[test]
fn cancellation_terminates_or_kills_and_cleans_up() {
    for mode in ["hang", "ignore-terminate"] {
        let fixture = Fixture::new(mode);
        let cancelled = Arc::new(AtomicBool::new(false));
        let trigger = Arc::clone(&cancelled);
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            trigger.store(true, Ordering::Release);
        });
        let started = Instant::now();
        let transcriber = fixture.test_transcriber(u64::MAX, Duration::from_millis(150));
        assert!(matches!(
            transcriber.transcribe(&[0.0; 16], &Fixture::request(cancelled)),
            Err(EngineError::Cancelled)
        ));
        thread.join().expect("cancellation trigger exits");
        assert!(started.elapsed() < Duration::from_secs(3));
        drop(transcriber);
        fixture.assert_temp_empty();
        assert_eq!(
            fixture
                .store
                .prune(&fixture.spec)
                .expect("prune after drop")
                .skipped_locked,
            0
        );
    }
}

#[test]
fn rejects_pre_cancelled_or_insufficient_space_before_spawning() {
    let fixture = Fixture::new("success");
    let cancelled = Arc::new(AtomicBool::new(true));
    assert!(matches!(
        fixture
            .test_transcriber(u64::MAX, Duration::from_millis(10))
            .transcribe(&[0.0; 16], &Fixture::request(cancelled)),
        Err(EngineError::Cancelled)
    ));
    assert!(!fixture.capture.exists());
    fixture.assert_temp_empty();

    let error = fixture
        .test_transcriber(1, Duration::from_millis(10))
        .transcribe(
            &[0.0; 16],
            &Fixture::request(Arc::new(AtomicBool::new(false))),
        )
        .expect_err("space probe rejects before spawn");
    assert!(matches!(error, EngineError::InsufficientTempSpace { .. }));
    assert!(!fixture.capture.exists());
    fixture.assert_temp_empty();
}
