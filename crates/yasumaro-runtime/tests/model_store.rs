use sha2::{Digest, Sha256};
use tempfile::TempDir;
use yasumaro_runtime::{ModelError, ModelId, ModelInstaller, ModelManifest, ModelSpec, ModelStore};

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[test]
fn require_is_local_and_reports_the_exact_install_command() {
    let root = TempDir::new().expect("temporary model root");
    let store = ModelStore::new(root.path());

    let error = store
        .require(ModelId::SpeakerEmbedding)
        .expect_err("missing model is classified");

    assert_eq!(
        error,
        ModelError::MissingModel {
            id: ModelId::SpeakerEmbedding,
            install_command: "yasumaro model install speaker-embedding".into(),
        }
    );
}

#[test]
fn embedded_manifest_has_secure_verifiable_entries() {
    let manifest = ModelManifest::embedded().expect("embedded manifest is valid");

    assert_eq!(manifest.specs().len(), 7);
    for spec in manifest.specs() {
        assert_eq!(spec.url.scheme(), "https");
        assert!(spec.size > 0);
        assert_eq!(spec.sha256.len(), 64);
        assert!(
            spec.sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        );
        assert!(!spec.license.trim().is_empty());
    }
}

#[test]
fn duplicate_manifest_ids_and_unlisted_install_ids_are_rejected() {
    let bytes = b"model";
    let first = ModelSpec {
        id: ModelId::WhisperBase,
        engine_version: "test".into(),
        url: "http://127.0.0.1/first".parse().expect("valid test URL"),
        size: u64::try_from(bytes.len()).expect("fixture length fits u64"),
        sha256: sha256(bytes),
        license: "CC0-1.0".into(),
        file_name: "ggml-base.bin".into(),
    };
    let duplicate = ModelSpec {
        url: "http://127.0.0.1/second".parse().expect("valid test URL"),
        ..first.clone()
    };
    assert!(matches!(
        ModelManifest::new(vec![first.clone(), duplicate]),
        Err(ModelError::InvalidManifest(_))
    ));

    let root = TempDir::new().expect("temporary model root");
    let installer = ModelInstaller::new(
        ModelManifest::new(vec![first]).expect("single entry is valid"),
        ModelStore::new(root.path()),
    )
    .expect("construct installer");
    assert!(matches!(
        installer.install(&[ModelId::WhisperSmall]),
        Err(ModelError::InvalidManifest(_))
    ));
}
