use std::fs;
use std::time::{Duration, Instant};

use tempfile::TempDir;
use yasumaro_runtime::{ModelError, ModelId, ModelStore};

const MODEL: ModelId = ModelId::WhisperBase;
const FINAL_NAME: &str = "ggml-base.bin";
const PARTIAL_NAME: &str = "ggml-base.bin.part";

fn store() -> (TempDir, ModelStore) {
    let root = TempDir::new().expect("temporary model root");
    let store = ModelStore::new(root.path());
    (root, store)
}

#[test]
fn acquire_holds_a_shared_lease_for_a_regular_model() {
    let (root, store) = store();
    let final_path = root.path().join(FINAL_NAME);
    fs::write(&final_path, b"verified model").expect("write model");

    let lease = store.acquire(MODEL).expect("acquire model lease");

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
    fs::write(root.path().join(FINAL_NAME), b"verified model").expect("write model");
    let _lease = store.acquire(MODEL).expect("acquire model lease");

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
        store.acquire(MODEL),
        Err(ModelError::MissingModel { id: MODEL, .. })
    ));
}
