use std::fs::{self, OpenOptions};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use fs4::fs_std::FileExt as _;
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use url::Url;
use yasumaro_runtime::{ModelError, ModelId, ModelSpec, ModelStore};

const MODEL: ModelId = ModelId::WhisperBase;
const FINAL_NAME: &str = "ggml-base.bin";
const PARTIAL_NAME: &str = "ggml-base.bin.part";

fn store() -> (TempDir, ModelStore) {
    let root = TempDir::new().expect("temporary model root");
    let store = ModelStore::new(root.path());
    (root, store)
}

fn spec(id: ModelId, file_name: &str, bytes: &[u8]) -> ModelSpec {
    ModelSpec {
        id,
        engine_version: "test".into(),
        url: Url::parse("https://example.invalid/model").expect("fixture URL"),
        size: u64::try_from(bytes.len()).expect("fixture length fits u64"),
        sha256: format!("{:x}", Sha256::digest(bytes)),
        license: "CC0-1.0".into(),
        file_name: file_name.into(),
    }
}

#[test]
fn acquire_rejects_corrupt_regular_files_for_all_transcription_models() {
    let expected = b"expected";
    for (id, file_name) in [
        (ModelId::WhisperBase, "ggml-base.bin"),
        (ModelId::SpeakerSegmentation, "segmentation-3-0.onnx"),
        (
            ModelId::SpeakerEmbedding,
            "3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx",
        ),
    ] {
        let (root, store) = store();
        fs::write(root.path().join(file_name), b"corrupt!").expect("write corrupt model");
        let model = spec(id, file_name, expected);

        assert!(matches!(
            store.acquire(&model),
            Err(ModelError::HashMismatch { id: actual, .. }) if actual == id
        ));
    }
}

#[test]
fn acquire_holds_a_shared_lease_for_a_regular_model() {
    let (root, store) = store();
    let final_path = root.path().join(FINAL_NAME);
    let bytes = b"verified model";
    fs::write(&final_path, bytes).expect("write model");
    let model = spec(MODEL, FINAL_NAME, bytes);

    let lease = store.acquire(&model).expect("acquire model lease");

    assert_eq!(lease.path(), final_path);
    assert!(matches!(
        store.remove(MODEL),
        Err(ModelError::ModelInUse { id: MODEL })
    ));
    drop(lease);
    store.remove(MODEL).expect("remove after lease release");
    assert!(!final_path.exists());
}

#[test]
fn dropping_one_shared_lease_keeps_other_readers_locked() {
    let (root, store) = store();
    let final_path = root.path().join(FINAL_NAME);
    let bytes = b"verified model";
    fs::write(&final_path, bytes).expect("write model");
    let model = spec(MODEL, FINAL_NAME, bytes);
    let first = store.acquire(&model).expect("acquire first model lease");
    let second = store.acquire(&model).expect("acquire second model lease");

    drop(first);
    assert_eq!(
        store.remove(MODEL),
        Err(ModelError::ModelInUse { id: MODEL })
    );
    drop(second);

    store
        .remove(MODEL)
        .expect("remove after both leases release");
    assert!(!final_path.exists());
}

#[test]
fn cancellable_acquire_stops_waiting_for_a_writer() {
    let (root, store) = store();
    let bytes = b"verified model";
    fs::write(root.path().join(FINAL_NAME), bytes).expect("write model");
    let model = spec(MODEL, FINAL_NAME, bytes);
    let lock_directory = root.path().join(".locks");
    fs::create_dir_all(&lock_directory).expect("create lock directory");
    let writer = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_directory.join("whisper-base.lock"))
        .expect("open lock");
    writer.lock_exclusive().expect("hold writer lock");
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_cancelled = Arc::clone(&cancelled);
    let (result_tx, result_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        let result = store.acquire_with_cancellation(&model, worker_cancelled.as_ref());
        let _ignored = result_tx.send(result);
    });
    thread::sleep(Duration::from_millis(50));
    assert!(matches!(
        result_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));

    cancelled.store(true, Ordering::Release);
    let result = result_rx
        .recv_timeout(Duration::from_millis(250))
        .expect("cancelled acquire returns while writer stays locked");
    fs4::fs_std::FileExt::unlock(&writer).expect("release writer lock");
    worker.join().expect("acquire worker exits");

    assert!(matches!(result, Err(ModelError::Cancelled { id: MODEL })));
}

#[test]
fn remove_is_idempotent_and_deletes_partial_before_final() {
    let (root, store) = store();
    let final_path = root.path().join(FINAL_NAME);
    let partial_path = root.path().join(PARTIAL_NAME);
    fs::write(&final_path, b"verified model").expect("write final model");
    fs::write(&partial_path, b"stale partial").expect("write partial model");

    store.remove(MODEL).expect("first remove");
    store.remove(MODEL).expect("second remove");

    assert!(!final_path.exists());
    assert!(!partial_path.exists());
    assert!(
        root.path()
            .join(".locks")
            .join("whisper-base.lock")
            .is_file()
    );
}

#[test]
fn remove_returns_model_in_use_without_waiting() {
    let (root, store) = store();
    let bytes = b"verified model";
    fs::write(root.path().join(FINAL_NAME), bytes).expect("write model");
    let model = spec(MODEL, FINAL_NAME, bytes);
    let _lease = store.acquire(&model).expect("acquire model lease");

    let started = Instant::now();
    let error = store.remove(MODEL).expect_err("busy model is rejected");

    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(error, ModelError::ModelInUse { id: MODEL });
    assert!(root.path().join(FINAL_NAME).is_file());
}

#[test]
fn remove_preserves_final_when_partial_cleanup_fails() {
    let (root, store) = store();
    let final_path = root.path().join(FINAL_NAME);
    let partial_path = root.path().join(PARTIAL_NAME);
    fs::write(&final_path, b"verified model").expect("write final model");
    fs::create_dir(&partial_path).expect("create partial directory");
    fs::write(partial_path.join("child"), b"blocks non-recursive removal").expect("write child");

    assert!(matches!(store.remove(MODEL), Err(ModelError::Storage(_))));
    assert_eq!(
        fs::read(final_path).expect("final model remains"),
        b"verified model"
    );
}

#[test]
fn remove_distinguishes_busy_from_lock_setup_failure() {
    let (root, store) = store();
    fs::write(root.path().join(FINAL_NAME), b"verified model").expect("write model");
    fs::write(root.path().join(".locks"), b"not a directory").expect("block lock directory");

    let error = store.remove(MODEL).expect_err("lock setup fails");

    assert!(matches!(error, ModelError::Lock { id: MODEL, .. }));
    assert!(root.path().join(FINAL_NAME).is_file());
}

#[test]
fn remove_rejects_nonempty_partial_directory() {
    let (root, store) = store();
    let partial_path = root.path().join(PARTIAL_NAME);
    fs::create_dir(&partial_path).expect("create partial directory");
    fs::write(partial_path.join("child"), b"retained").expect("write child");

    assert!(matches!(store.remove(MODEL), Err(ModelError::Storage(_))));
    assert!(partial_path.join("child").is_file());
}

#[cfg(unix)]
#[test]
fn remove_unlinks_symlink_without_following_target() {
    use std::os::unix::fs::symlink;

    let (root, store) = store();
    let outside = TempDir::new().expect("outside directory");
    let target = outside.path().join("target.bin");
    let final_path = root.path().join(FINAL_NAME);
    fs::write(&target, b"outside model").expect("write target");
    symlink(&target, &final_path).expect("create model symlink");

    store.remove(MODEL).expect("unlink model symlink");

    assert!(!final_path.exists());
    assert_eq!(fs::read(target).expect("target remains"), b"outside model");
}

#[cfg(unix)]
#[test]
fn require_and_acquire_reject_a_model_symlink() {
    use std::os::unix::fs::symlink;

    let (root, store) = store();
    let outside = TempDir::new().expect("outside directory");
    let target = outside.path().join("target.bin");
    fs::write(&target, b"outside model").expect("write target");
    symlink(&target, root.path().join(FINAL_NAME)).expect("create model symlink");

    assert!(matches!(
        store.require(MODEL),
        Err(ModelError::MissingModel { id: MODEL, .. })
    ));
    assert!(matches!(
        store.acquire(&spec(MODEL, FINAL_NAME, b"outside model")),
        Err(ModelError::MissingModel { id: MODEL, .. })
    ));
}
