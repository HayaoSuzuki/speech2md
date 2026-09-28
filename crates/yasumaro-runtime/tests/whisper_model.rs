use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use yasumaro_runtime::engine::{Transcriber, TranscriptionRequest, WhisperProcessTranscriber};
use yasumaro_runtime::engine_artifact::{EngineManifest, EngineStore, Platform};
use yasumaro_runtime::{ModelId, ModelStore, decode_to_pcm};

#[test]
#[ignore = "requires locally installed engine, model, and self-authored Japanese fixture"]
fn transcribes_a_local_japanese_fixture_without_downloading() {
    let Some(engine_root) = std::env::var_os("YASUMARO_ENGINE_DIR").map(PathBuf::from) else {
        return;
    };
    let Some(model_root) = std::env::var_os("YASUMARO_MODEL_DIR").map(PathBuf::from) else {
        return;
    };
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/japanese-short.wav");
    if !fixture.is_file() {
        return;
    }
    let temporary = tempfile::tempdir().expect("temporary processing root");
    let spec = EngineManifest::embedded()
        .expect("embedded engine manifest")
        .select(Platform::current().expect("supported platform"))
        .expect("published artifact")
        .clone();
    let lease = EngineStore::new(engine_root)
        .require(&spec)
        .expect("installed engine")
        .acquire()
        .expect("engine lease");
    let model = ModelStore::new(model_root)
        .acquire(ModelId::WhisperBase)
        .expect("installed whisper base model");
    let transcriber = WhisperProcessTranscriber::new(lease, model, temporary.path().to_path_buf())
        .expect("process transcriber");
    let pcm = decode_to_pcm(&fixture, temporary.path()).expect("decode fixture");
    let segments = transcriber
        .transcribe(
            pcm.samples(),
            &TranscriptionRequest {
                prompt: None,
                threads: 1,
                cancelled: Arc::new(AtomicBool::new(false)),
            },
        )
        .expect("transcribe fixture");

    assert!(!segments.is_empty());
    assert!(
        segments
            .iter()
            .all(|segment| !segment.text.trim().is_empty())
    );
    assert!(
        segments
            .windows(2)
            .all(|pair| pair[0].span.start() <= pair[1].span.start())
    );
}
