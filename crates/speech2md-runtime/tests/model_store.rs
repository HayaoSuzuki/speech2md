use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

use sha2::{Digest, Sha256};
use speech2md_runtime::{
    ModelError, ModelId, ModelInstaller, ModelManifest, ModelSpec, ModelStore,
};
use tempfile::TempDir;

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

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn model_server(body: &'static [u8], declared_length: usize) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local model server");
    let address = listener.local_addr().expect("local server address");
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept one request");
        let mut request = [0_u8; 1_024];
        let _bytes_read = stream.read(&mut request).expect("read request");
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {declared_length}\r\nConnection: close\r\n\r\n"
        )
        .expect("write response headers");
        stream.write_all(body).expect("write response body");
    });
    (format!("http://{address}/model.bin"), handle)
}

fn manifest(url: &str, bytes: &[u8], hash: String) -> ModelManifest {
    ModelManifest::new(vec![ModelSpec {
        id: ModelId::WhisperBase,
        engine_version: "test".into(),
        url: url.parse().expect("valid local URL"),
        size: u64::try_from(bytes.len()).expect("fixture length fits u64"),
        sha256: hash,
        license: "CC0-1.0".into(),
        file_name: "ggml-base.bin".into(),
    }])
    .expect("valid test manifest")
}

fn installer_for(url: &str, bytes: &[u8], hash: String) -> (ModelInstaller, ModelStore, TempDir) {
    let root = TempDir::new().expect("temporary model root");
    let store = ModelStore::new(root.path());
    let installer = ModelInstaller::new(manifest(url, bytes, hash), store.clone())
        .expect("construct model installer");
    (installer, store, root)
}

#[test]
fn installs_only_after_sha256_matches() {
    let bytes = b"model bytes";
    let (url, server) = model_server(bytes, bytes.len());
    let (installer, store, _root) = installer_for(&url, bytes, sha256(bytes));
    let logs = captured_logs();

    installer
        .install(&[ModelId::WhisperBase])
        .expect("matching model installs");
    server.join().expect("model server exits");

    let path = store
        .require(ModelId::WhisperBase)
        .expect("installed model is available");
    assert_eq!(std::fs::read(path).expect("read installed model"), bytes);
    let output = logs.text();
    assert!(output.contains("model installation started"));
    assert!(output.contains("model installation completed"));
    assert!(output.contains("model_id=whisper-base"));
    assert!(!output.contains(&url));
    assert!(!output.contains(store.root().to_string_lossy().as_ref()));
}

#[test]
fn hash_mismatch_leaves_no_final_or_partial_file() {
    let bytes = b"corrupt model";
    let (url, server) = model_server(bytes, bytes.len());
    let (installer, store, root) = installer_for(&url, bytes, sha256(b"expected model"));

    let result = installer.install(&[ModelId::WhisperBase]);
    server.join().expect("model server exits");

    assert!(matches!(result, Err(ModelError::HashMismatch { .. })));
    assert!(matches!(
        store.require(ModelId::WhisperBase),
        Err(ModelError::MissingModel { .. })
    ));
    let entries = std::fs::read_dir(root.path())
        .expect("read model root")
        .collect::<Result<Vec<_>, _>>()
        .expect("read all entries");
    assert!(entries.is_empty());
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
            install_command: "speech2md model install speaker-embedding".into(),
        }
    );
}

#[test]
fn interrupted_response_does_not_replace_an_existing_model() {
    let old_bytes = b"existing verified model";
    let new_bytes = b"new but truncated";
    let (url, server) = model_server(new_bytes, new_bytes.len() + 100);
    let (installer, store, root) = installer_for(&url, new_bytes, sha256(new_bytes));
    std::fs::create_dir_all(root.path()).expect("create model root");
    let final_path = root.path().join("ggml-base.bin");
    std::fs::write(&final_path, old_bytes).expect("write existing model");

    let result = installer.install(&[ModelId::WhisperBase]);
    server.join().expect("model server exits");

    assert!(result.is_err());
    assert_eq!(
        std::fs::read(
            store
                .require(ModelId::WhisperBase)
                .expect("old model remains")
        )
        .expect("read old model"),
        old_bytes
    );
    assert!(!PathBuf::from(format!("{}.part", final_path.display())).exists());
}

#[test]
fn embedded_manifest_has_secure_verifiable_entries() {
    let manifest = ModelManifest::embedded().expect("embedded manifest is valid");

    assert_eq!(manifest.specs().len(), 4);
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
fn oversized_response_is_rejected_and_cleaned_up() {
    let expected = b"short";
    let response = b"short plus unexpected bytes";
    let (url, server) = model_server(response, response.len());
    let (installer, store, root) = installer_for(&url, expected, sha256(expected));

    let result = installer.install(&[ModelId::WhisperBase]);
    server.join().expect("model server exits");

    assert!(matches!(result, Err(ModelError::SizeMismatch { .. })));
    assert!(matches!(
        store.require(ModelId::WhisperBase),
        Err(ModelError::MissingModel { .. })
    ));
    assert!(
        std::fs::read_dir(root.path())
            .expect("read model root")
            .next()
            .is_none()
    );
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
