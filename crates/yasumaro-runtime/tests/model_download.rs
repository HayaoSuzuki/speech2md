use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex, OnceLock, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use tempfile::TempDir;
use yasumaro_runtime::{
    ModelError, ModelId, ModelInstaller, ModelManifest, ModelSpec, ModelStore, PublishCheckpoint,
};

const MODEL: ModelId = ModelId::WhisperBase;
const FINAL_NAME: &str = "ggml-base.bin";
const PARTIAL_NAME: &str = "ggml-base.bin.part";

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

fn manifest(url: &str, bytes: &[u8], hash: String) -> ModelManifest {
    ModelManifest::new(vec![ModelSpec {
        id: MODEL,
        engine_version: "test".into(),
        url: url.parse().expect("valid local URL"),
        size: u64::try_from(bytes.len()).expect("fixture length fits u64"),
        sha256: hash,
        license: "CC0-1.0".into(),
        file_name: FINAL_NAME.into(),
    }])
    .expect("valid test manifest")
}

fn installer(
    root: &TempDir,
    url: &str,
    bytes: &[u8],
    hash: String,
    read_timeout: Duration,
) -> ModelInstaller {
    ModelInstaller::new_for_test(
        manifest(url, bytes, hash),
        ModelStore::new(root.path()),
        Duration::from_secs(1),
        read_timeout,
    )
    .expect("construct test installer")
}

fn spawn_server(
    handler: impl FnOnce(TcpStream) + Send + 'static,
) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local model server");
    let address = listener.local_addr().expect("local server address");
    let handle = thread::spawn(move || {
        let (stream, _) = listener.accept().expect("accept request");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("set server read timeout");
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .expect("set server write timeout");
        handler(stream);
    });
    (format!("http://{address}/model.bin"), handle)
}

fn read_request(stream: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 1_024];
    while !bytes.windows(4).any(|window| window == b"\r\n\r\n") {
        let count = stream.read(&mut buffer).expect("read request");
        assert!(count > 0, "request ended before headers");
        bytes.extend_from_slice(&buffer[..count]);
    }
    String::from_utf8(bytes).expect("request is UTF-8")
}

fn respond(stream: &mut TcpStream, status: &str, declared_length: usize, body: &[u8]) {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Length: {declared_length}\r\nConnection: close\r\n\r\n"
    )
    .expect("write response headers");
    stream.write_all(body).expect("write response body");
    stream.flush().expect("flush response");
}

fn assert_no_partial(root: &TempDir) {
    assert!(!root.path().join(PARTIAL_NAME).exists());
}

#[test]
fn fresh_download_sends_no_range_and_creates_no_sidecar() {
    let bytes = b"model bytes";
    let request = Arc::new(Mutex::new(String::new()));
    let captured = Arc::clone(&request);
    let (url, server) = spawn_server(move |mut stream| {
        *captured.lock().expect("request lock") = read_request(&mut stream);
        respond(&mut stream, "200 OK", bytes.len(), bytes);
    });
    let root = TempDir::new().expect("temporary model root");
    let installer = installer(&root, &url, bytes, sha256(bytes), Duration::from_secs(1));

    installer.install(&[MODEL]).expect("fresh install succeeds");
    server.join().expect("server exits");

    let request = request.lock().expect("request lock").to_ascii_lowercase();
    assert!(request.starts_with("get /model.bin http/1.1\r\n"));
    assert!(!request.contains("\r\nrange:"));
    assert!(!request.contains("\r\nif-range:"));
    assert_eq!(
        std::fs::read(root.path().join(FINAL_NAME)).expect("read installed model"),
        bytes
    );
    assert_no_partial(&root);
    let entries = std::fs::read_dir(root.path())
        .expect("read model root")
        .map(|entry| entry.expect("model entry").file_name())
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().any(|name| name == FINAL_NAME));
    assert!(entries.iter().any(|name| name == ".locks"));
}

#[test]
fn bounded_redirect_is_followed_without_transfer_retry() {
    let bytes = b"redirect target model";
    let requests = Arc::new(AtomicUsize::new(0));
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind redirect server");
    let address = listener.local_addr().expect("redirect server address");
    let server_requests = Arc::clone(&requests);
    let server = thread::spawn(move || {
        let (mut first, _) = listener.accept().expect("accept redirect request");
        let _request = read_request(&mut first);
        server_requests.fetch_add(1, Ordering::Relaxed);
        write!(
            first,
            "HTTP/1.1 302 Found\r\nLocation: http://{address}/second\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
        .expect("write redirect");
        listener.set_nonblocking(true).expect("set nonblocking");
        let deadline = Instant::now() + Duration::from_millis(300);
        while Instant::now() < deadline {
            match listener.accept() {
                Ok((mut second, _)) => {
                    let _request = read_request(&mut second);
                    server_requests.fetch_add(1, Ordering::Relaxed);
                    respond(&mut second, "200 OK", bytes.len(), bytes);
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("accept redirect target: {error}"),
            }
        }
    });
    let url = format!("http://{address}/first");
    let root = TempDir::new().expect("temporary model root");
    let installer = installer(&root, &url, bytes, sha256(bytes), Duration::from_secs(1));

    let result = installer.install(&[MODEL]);
    server.join().expect("redirect server exits");

    result.expect("redirected download succeeds");
    assert_eq!(requests.load(Ordering::Relaxed), 2);
    assert_eq!(
        std::fs::read(root.path().join(FINAL_NAME)).expect("read redirected model"),
        bytes
    );
    assert_no_partial(&root);
}

#[test]
fn valid_final_skips_network_and_removes_stale_partial() {
    let bytes = b"verified model";
    let root = TempDir::new().expect("temporary model root");
    std::fs::write(root.path().join(FINAL_NAME), bytes).expect("write final model");
    std::fs::write(root.path().join(PARTIAL_NAME), b"stale").expect("write stale partial");
    let installer = installer(
        &root,
        "http://127.0.0.1:9/not-used",
        bytes,
        sha256(bytes),
        Duration::from_millis(50),
    );

    installer.install(&[MODEL]).expect("valid final is reused");

    assert_no_partial(&root);
    assert_eq!(
        std::fs::read(root.path().join(FINAL_NAME)).expect("read reused final model"),
        bytes
    );
}

#[test]
fn valid_final_survives_stale_partial_cleanup_failure() {
    let bytes = b"verified model";
    let root = TempDir::new().expect("temporary model root");
    std::fs::write(root.path().join(FINAL_NAME), bytes).expect("write final model");
    let partial = root.path().join(PARTIAL_NAME);
    std::fs::create_dir(&partial).expect("create partial directory");
    std::fs::write(partial.join("child"), b"blocks cleanup").expect("write child");
    let installer = installer(
        &root,
        "http://127.0.0.1:9/not-used",
        bytes,
        sha256(bytes),
        Duration::from_millis(50),
    );

    assert!(matches!(
        installer.install(&[MODEL]),
        Err(ModelError::Storage(_))
    ));
    assert_eq!(
        std::fs::read(root.path().join(FINAL_NAME)).expect("read preserved final model"),
        bytes
    );
}

#[test]
fn invalid_final_is_removed_before_failed_download() {
    let expected = b"expected model";
    let root = TempDir::new().expect("temporary model root");
    std::fs::write(root.path().join(FINAL_NAME), b"invalid").expect("write invalid final");
    let installer = installer(
        &root,
        "http://127.0.0.1:9/unreachable",
        expected,
        sha256(expected),
        Duration::from_millis(50),
    );

    assert!(matches!(
        installer.install(&[MODEL]),
        Err(ModelError::Download { .. })
    ));
    assert!(!root.path().join(FINAL_NAME).exists());
    assert_no_partial(&root);
}

#[test]
fn interrupted_response_cleans_partial() {
    let bytes = b"truncated";
    let (url, server) = spawn_server(move |mut stream| {
        let _request = read_request(&mut stream);
        respond(&mut stream, "200 OK", bytes.len() + 100, bytes);
    });
    let root = TempDir::new().expect("temporary model root");
    let installer = installer(&root, &url, bytes, sha256(bytes), Duration::from_secs(1));

    assert!(installer.install(&[MODEL]).is_err());
    server.join().expect("server exits");
    assert_no_partial(&root);
    assert!(!root.path().join(FINAL_NAME).exists());
}

#[test]
fn read_timeout_cleans_partial() {
    let bytes = b"never sent";
    let (url, server) = spawn_server(move |mut stream| {
        let _request = read_request(&mut stream);
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            bytes.len()
        )
        .expect("write headers");
        stream.flush().expect("flush headers");
        thread::sleep(Duration::from_millis(200));
    });
    let root = TempDir::new().expect("temporary model root");
    let installer = installer(&root, &url, bytes, sha256(bytes), Duration::from_millis(30));

    assert!(matches!(
        installer.install(&[MODEL]),
        Err(ModelError::Download { .. })
    ));
    server.join().expect("server exits");
    assert_no_partial(&root);
}

#[test]
fn cancellation_stops_waiting_for_response_headers() {
    let bytes = b"model bytes";
    let (request_tx, request_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (url, server) = spawn_server(move |mut stream| {
        let _request = read_request(&mut stream);
        request_tx.send(()).expect("signal received request");
        release_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("release stalled response");
    });
    let root = TempDir::new().expect("temporary model root");
    let installer = installer(&root, &url, bytes, sha256(bytes), Duration::from_secs(1));
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_flag = Arc::clone(&cancelled);
    let (result_tx, result_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        result_tx
            .send(installer.install_with_cancellation(&[MODEL], &worker_flag))
            .expect("send install result");
    });

    request_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("request reaches server");
    cancelled.store(true, Ordering::Release);
    let prompt = result_rx.recv_timeout(Duration::from_millis(250));
    release_tx.send(()).expect("release response headers");
    let completed_while_stalled = prompt.is_ok();
    let result = prompt.unwrap_or_else(|_| {
        result_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("installer eventually exits")
    });
    worker.join().expect("installer worker exits");
    server.join().expect("server exits");

    assert!(
        completed_while_stalled,
        "cancellation must not wait for response headers"
    );
    assert_eq!(result, Err(ModelError::Cancelled { id: MODEL }));
    assert_no_partial(&root);
}

#[test]
fn cancellation_stops_a_stalled_response_body_without_server_progress() {
    let bytes = b"first-second";
    let (first_sent_tx, first_sent_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (url, server) = spawn_server(move |mut stream| {
        let _request = read_request(&mut stream);
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            bytes.len()
        )
        .expect("write headers");
        stream.write_all(b"first-").expect("write first chunk");
        stream.flush().expect("flush first chunk");
        first_sent_tx.send(()).expect("signal first chunk");
        release_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("release stalled body");
    });
    let root = TempDir::new().expect("temporary model root");
    let installer = installer(&root, &url, bytes, sha256(bytes), Duration::from_secs(1));
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_flag = Arc::clone(&cancelled);
    let (result_tx, result_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        result_tx
            .send(installer.install_with_cancellation(&[MODEL], &worker_flag))
            .expect("send install result");
    });

    first_sent_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("receive first chunk signal");
    let partial_path = root.path().join(PARTIAL_NAME);
    let write_deadline = Instant::now() + Duration::from_secs(2);
    while std::fs::metadata(&partial_path).map_or(true, |metadata| metadata.len() == 0) {
        assert!(
            Instant::now() < write_deadline,
            "installer writes the first chunk before cancellation"
        );
        thread::sleep(Duration::from_millis(5));
    }
    cancelled.store(true, Ordering::Release);
    let prompt = result_rx.recv_timeout(Duration::from_millis(250));
    release_tx.send(()).expect("release stalled body");
    let completed_while_stalled = prompt.is_ok();
    let result = prompt.unwrap_or_else(|_| {
        result_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("installer eventually exits")
    });
    worker.join().expect("installer worker exits");
    server.join().expect("server exits");

    assert!(
        completed_while_stalled,
        "cancellation must not wait for additional response bytes"
    );
    assert_eq!(result, Err(ModelError::Cancelled { id: MODEL }));
    assert_no_partial(&root);
}

#[test]
fn progressing_response_is_not_limited_by_the_read_timeout_total() {
    let bytes = b"abcde";
    let (url, server) = spawn_server(move |mut stream| {
        stream
            .set_nodelay(true)
            .expect("send each progress byte without Nagle buffering");
        let _request = read_request(&mut stream);
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            bytes.len()
        )
        .expect("write headers");
        for byte in bytes {
            stream
                .write_all(std::slice::from_ref(byte))
                .expect("write chunk");
            stream.flush().expect("flush chunk");
            thread::sleep(Duration::from_millis(40));
        }
    });
    let root = TempDir::new().expect("temporary model root");
    let installer = installer(&root, &url, bytes, sha256(bytes), Duration::from_millis(80));

    installer
        .install(&[MODEL])
        .expect("progressing response succeeds");
    server.join().expect("server exits");
    assert_no_partial(&root);
}

#[test]
fn http_client_initialization_error_has_no_model_id_or_internal_message() {
    let error = ModelError::HttpClientInitialization;
    let display = error.to_string();

    assert_eq!(display, "HTTP client initialization failed");
    assert!(!display.contains("whisper-base"));
    assert!(!display.contains("http://"));
    assert!(!display.contains("builder"));
}

#[test]
fn cancellation_before_publish_authorization_cleans_partial() {
    let bytes = b"verified model";
    let (url, server) = spawn_server(move |mut stream| {
        let _request = read_request(&mut stream);
        respond(&mut stream, "200 OK", bytes.len(), bytes);
    });
    let root = TempDir::new().expect("temporary model root");
    let store = ModelStore::new(root.path());
    let (checkpoint_tx, checkpoint_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let resume_rx = Arc::new(Mutex::new(resume_rx));
    let observer_resume = Arc::clone(&resume_rx);
    let observer = Arc::new(move |checkpoint| {
        if checkpoint == PublishCheckpoint::BeforeAuthorization {
            checkpoint_tx.send(()).expect("signal checkpoint");
            observer_resume
                .lock()
                .expect("resume lock")
                .recv_timeout(Duration::from_secs(2))
                .expect("resume publication");
        }
    });
    let installer = ModelInstaller::new_for_test_with_publish_observer(
        manifest(&url, bytes, sha256(bytes)),
        store,
        Duration::from_secs(1),
        Duration::from_secs(1),
        observer,
    )
    .expect("construct observed installer");
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_flag = Arc::clone(&cancelled);
    let worker = thread::spawn(move || installer.install_with_cancellation(&[MODEL], &worker_flag));

    checkpoint_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("reach checkpoint before authorization");
    cancelled.store(true, Ordering::Release);
    resume_tx.send(()).expect("resume publication");
    let result = worker.join().expect("installer worker exits");
    server.join().expect("server exits");

    assert_eq!(result, Err(ModelError::Cancelled { id: MODEL }));
    assert!(!root.path().join(FINAL_NAME).exists());
    assert_no_partial(&root);
}

#[test]
fn cancellation_after_publish_authorization_keeps_success() {
    let bytes = b"verified model";
    let (url, server) = spawn_server(move |mut stream| {
        let _request = read_request(&mut stream);
        respond(&mut stream, "200 OK", bytes.len(), bytes);
    });
    let root = TempDir::new().expect("temporary model root");
    let store = ModelStore::new(root.path());
    let model_manifest = manifest(&url, bytes, sha256(bytes));
    let (checkpoint_tx, checkpoint_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let resume_rx = Arc::new(Mutex::new(resume_rx));
    let observer_resume = Arc::clone(&resume_rx);
    let observer = Arc::new(move |checkpoint| {
        if checkpoint == PublishCheckpoint::AfterAuthorization {
            checkpoint_tx.send(()).expect("signal checkpoint");
            observer_resume
                .lock()
                .expect("resume lock")
                .recv_timeout(Duration::from_secs(2))
                .expect("resume publication");
        }
    });
    let installer = ModelInstaller::new_for_test_with_publish_observer(
        model_manifest.clone(),
        store.clone(),
        Duration::from_secs(1),
        Duration::from_secs(1),
        observer,
    )
    .expect("construct observed installer");
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_flag = Arc::clone(&cancelled);
    let worker = thread::spawn(move || installer.install_with_cancellation(&[MODEL], &worker_flag));

    checkpoint_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("reach checkpoint after authorization");
    cancelled.store(true, Ordering::Release);
    resume_tx.send(()).expect("resume publication");
    let result = worker.join().expect("installer worker exits");
    server.join().expect("server exits");

    result.expect("authorized publication succeeds");
    let lease = store
        .acquire(model_manifest.spec(MODEL).expect("model specification"))
        .expect("published model remains verified");
    assert_eq!(
        std::fs::read(lease.path()).expect("read published model"),
        bytes
    );
    assert_no_partial(&root);
}

#[test]
fn midstream_cancellation_cleans_partial() {
    let bytes = b"first-second";
    let (first_sent_tx, first_sent_rx) = mpsc::channel();
    let (continue_tx, continue_rx) = mpsc::channel();
    let (url, server) = spawn_server(move |mut stream| {
        let _request = read_request(&mut stream);
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            bytes.len()
        )
        .expect("write headers");
        stream.write_all(b"first-").expect("write first chunk");
        stream.flush().expect("flush first chunk");
        first_sent_tx.send(()).expect("signal first chunk");
        continue_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("continue response");
        stream.write_all(b"second").expect("write second chunk");
    });
    let root = TempDir::new().expect("temporary model root");
    let installer = installer(&root, &url, bytes, sha256(bytes), Duration::from_secs(1));
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_flag = Arc::clone(&cancelled);
    let worker = thread::spawn(move || installer.install_with_cancellation(&[MODEL], &worker_flag));

    first_sent_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("receive first chunk signal");
    cancelled.store(true, Ordering::Release);
    continue_tx.send(()).expect("continue response");
    let result = worker.join().expect("installer thread exits");
    server.join().expect("server exits");

    assert_eq!(result, Err(ModelError::Cancelled { id: MODEL }));
    assert_no_partial(&root);
}

#[test]
fn oversized_response_cleans_partial() {
    let expected = b"short";
    let response = b"short plus unexpected bytes";
    let (url, server) = spawn_server(move |mut stream| {
        let _request = read_request(&mut stream);
        respond(&mut stream, "200 OK", response.len(), response);
    });
    let root = TempDir::new().expect("temporary model root");
    let installer = installer(
        &root,
        &url,
        expected,
        sha256(expected),
        Duration::from_secs(1),
    );

    let result = installer.install(&[MODEL]);
    server.join().expect("server exits");

    assert!(matches!(result, Err(ModelError::SizeMismatch { .. })));
    assert_no_partial(&root);
}

#[test]
fn hash_mismatch_cleans_partial() {
    let bytes = b"corrupt model";
    let (url, server) = spawn_server(move |mut stream| {
        let _request = read_request(&mut stream);
        respond(&mut stream, "200 OK", bytes.len(), bytes);
    });
    let root = TempDir::new().expect("temporary model root");
    let installer = installer(
        &root,
        &url,
        bytes,
        sha256(b"different model"),
        Duration::from_secs(1),
    );

    let result = installer.install(&[MODEL]);
    server.join().expect("server exits");

    assert!(matches!(result, Err(ModelError::HashMismatch { .. })));
    assert_no_partial(&root);
}

#[test]
fn publish_failure_cleans_partial() {
    let bytes = b"verified model";
    let (request_tx, request_rx) = mpsc::channel();
    let (respond_tx, respond_rx) = mpsc::channel();
    let (url, server) = spawn_server(move |mut stream| {
        let _request = read_request(&mut stream);
        request_tx.send(()).expect("signal request");
        respond_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("response permission");
        respond(&mut stream, "200 OK", bytes.len(), bytes);
    });
    let root = TempDir::new().expect("temporary model root");
    let installer = installer(&root, &url, bytes, sha256(bytes), Duration::from_secs(1));
    let worker = thread::spawn(move || installer.install(&[MODEL]));

    request_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("receive request signal");
    let final_path = root.path().join(FINAL_NAME);
    std::fs::create_dir(&final_path).expect("create blocking final directory");
    std::fs::write(final_path.join("child"), b"block replacement").expect("write child");
    respond_tx.send(()).expect("allow response");
    let result = worker.join().expect("installer thread exits");
    server.join().expect("server exits");

    assert!(matches!(result, Err(ModelError::Storage(_))));
    assert_no_partial(&root);
}

#[test]
fn concurrent_installs_issue_one_request_and_reuse_verified_final() {
    let bytes = b"verified concurrent model";
    let requests = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local model server");
    let address = listener.local_addr().expect("local server address");
    let server_requests = Arc::clone(&requests);
    let server_stop = Arc::clone(&stop);
    let server = thread::spawn(move || {
        let (mut first, _) = listener.accept().expect("accept first request");
        let _request = read_request(&mut first);
        server_requests.fetch_add(1, Ordering::Relaxed);
        respond(&mut first, "200 OK", bytes.len(), bytes);
        listener.set_nonblocking(true).expect("set nonblocking");
        while !server_stop.load(Ordering::Acquire) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let _request = read_request(&mut stream);
                    server_requests.fetch_add(1, Ordering::Relaxed);
                    respond(&mut stream, "200 OK", bytes.len(), bytes);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("accept request: {error}"),
            }
        }
    });
    let url = format!("http://{address}/model.bin");
    let root = TempDir::new().expect("temporary model root");
    let store = ModelStore::new(root.path());
    let model_manifest = manifest(&url, bytes, sha256(bytes));
    let first = ModelInstaller::new_for_test(
        model_manifest.clone(),
        store.clone(),
        Duration::from_secs(1),
        Duration::from_secs(1),
    )
    .expect("first installer");
    let second = ModelInstaller::new_for_test(
        model_manifest,
        store,
        Duration::from_secs(1),
        Duration::from_secs(1),
    )
    .expect("second installer");
    let start = Arc::new(Barrier::new(3));
    let first_start = Arc::clone(&start);
    let second_start = Arc::clone(&start);
    let first_worker = thread::spawn(move || {
        first_start.wait();
        first.install(&[MODEL])
    });
    let second_worker = thread::spawn(move || {
        second_start.wait();
        second.install(&[MODEL])
    });

    start.wait();
    first_worker
        .join()
        .expect("first worker exits")
        .expect("first install succeeds");
    second_worker
        .join()
        .expect("second worker exits")
        .expect("second install succeeds");
    stop.store(true, Ordering::Release);
    server.join().expect("server exits");

    assert_eq!(requests.load(Ordering::Relaxed), 1);
}

#[test]
fn install_waits_for_an_active_model_lease() {
    let replacement = b"replacement model";
    let (request_tx, request_rx) = mpsc::channel();
    let (url, server) = spawn_server(move |mut stream| {
        let _request = read_request(&mut stream);
        request_tx.send(()).expect("signal request");
        respond(&mut stream, "200 OK", replacement.len(), replacement);
    });
    let root = TempDir::new().expect("temporary model root");
    std::fs::write(root.path().join(FINAL_NAME), b"leased old model").expect("write leased model");
    let store = ModelStore::new(root.path());
    let leased_manifest = manifest(
        "http://127.0.0.1:9/not-used",
        b"leased old model",
        sha256(b"leased old model"),
    );
    let lease = store
        .acquire(leased_manifest.spec(MODEL).expect("leased specification"))
        .expect("acquire model lease");
    let installer = installer(
        &root,
        &url,
        replacement,
        sha256(replacement),
        Duration::from_secs(1),
    );
    let start = Arc::new(Barrier::new(2));
    let worker_start = Arc::clone(&start);
    let worker = thread::spawn(move || {
        worker_start.wait();
        installer.install(&[MODEL])
    });

    start.wait();
    assert!(
        request_rx.recv_timeout(Duration::from_millis(100)).is_err(),
        "install must not inspect or replace a leased model"
    );
    drop(lease);
    request_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("request begins after lease release");
    worker
        .join()
        .expect("installer worker exits")
        .expect("install succeeds after lease release");
    server.join().expect("server exits");
    assert_eq!(
        std::fs::read(root.path().join(FINAL_NAME)).expect("read replacement model"),
        replacement
    );
}

#[test]
fn cancelled_install_stops_waiting_for_an_active_model_lease() {
    let bytes = b"verified leased model";
    let root = TempDir::new().expect("temporary model root");
    std::fs::write(root.path().join(FINAL_NAME), bytes).expect("write leased model");
    let store = ModelStore::new(root.path());
    let leased_manifest = manifest("http://127.0.0.1:9/not-used", bytes, sha256(bytes));
    let lease = store
        .acquire(leased_manifest.spec(MODEL).expect("leased specification"))
        .expect("acquire model lease");
    let installer = installer(
        &root,
        "http://127.0.0.1:9/not-used",
        bytes,
        sha256(bytes),
        Duration::from_secs(1),
    );
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_flag = Arc::clone(&cancelled);
    let (started_tx, started_rx) = mpsc::channel();
    let (result_tx, result_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        started_tx.send(()).expect("signal installer start");
        let result = installer.install_with_cancellation(&[MODEL], &worker_flag);
        result_tx.send(result).expect("send install result");
    });

    started_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("installer starts");
    cancelled.store(true, Ordering::Release);
    let prompt = result_rx.recv_timeout(Duration::from_millis(250));
    drop(lease);
    let completed_before_release = prompt.is_ok();
    let result = prompt.unwrap_or_else(|_| {
        result_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("installer eventually exits")
    });
    worker.join().expect("installer worker exits");

    assert!(
        completed_before_release,
        "cancellation must stop lock waiting before the lease is released"
    );
    assert_eq!(result, Err(ModelError::Cancelled { id: MODEL }));
}

#[test]
fn pre_cancelled_install_does_not_reuse_a_valid_final() {
    let bytes = b"verified model";
    let root = TempDir::new().expect("temporary model root");
    std::fs::write(root.path().join(FINAL_NAME), bytes).expect("write final model");
    let installer = installer(
        &root,
        "http://127.0.0.1:9/not-used",
        bytes,
        sha256(bytes),
        Duration::from_secs(1),
    );
    let cancelled = AtomicBool::new(true);

    assert_eq!(
        installer.install_with_cancellation(&[MODEL], &cancelled),
        Err(ModelError::Cancelled { id: MODEL })
    );
    assert_eq!(
        std::fs::read(root.path().join(FINAL_NAME)).expect("valid final remains"),
        bytes
    );
}

#[test]
fn errors_and_logs_do_not_expose_url_or_full_path() {
    let secret = "secret-model-location";
    let (base_url, server) = spawn_server(move |mut stream| {
        let _request = read_request(&mut stream);
        respond(&mut stream, "503 Service Unavailable", 0, b"");
    });
    let url = base_url.replace("/model.bin", &format!("/{secret}"));
    let root = TempDir::new().expect("temporary model root");
    let logs = captured_logs();
    let before = logs.text().len();
    let bytes = b"expected model";
    let installer = installer(&root, &url, bytes, sha256(bytes), Duration::from_secs(1));

    let error = installer.install(&[MODEL]).expect_err("status error");
    server.join().expect("server exits");

    let display = error.to_string();
    let output = logs.text()[before..].to_owned();
    assert!(!display.contains(secret));
    assert!(!display.contains(root.path().to_string_lossy().as_ref()));
    assert!(!output.contains(secret));
    assert!(!output.contains(root.path().to_string_lossy().as_ref()));
}

#[test]
fn local_server_tests_finish_within_a_fixed_upper_bound() {
    let started = Instant::now();
    let (url, server) = spawn_server(|mut stream| {
        let _request = read_request(&mut stream);
        respond(&mut stream, "500 Internal Server Error", 0, b"");
    });
    let root = TempDir::new().expect("temporary model root");
    let installer = installer(
        &root,
        &url,
        b"expected",
        sha256(b"expected"),
        Duration::from_secs(1),
    );

    assert!(installer.install(&[MODEL]).is_err());
    server.join().expect("server exits");
    assert!(started.elapsed() < Duration::from_secs(3));
}
