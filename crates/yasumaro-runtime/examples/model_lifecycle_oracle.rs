#![allow(
    clippy::print_stderr,
    clippy::print_stdout,
    reason = "the oracle contract emits machine-readable status and reports"
)]

use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use yasumaro_runtime::{
    ModelError, ModelId, ModelInstaller, ModelLease, ModelManifest, ModelSpec, ModelStore,
    PublishCheckpoint,
};

const FIXTURE: &str = include_str!("../tests/fixtures/lean-model-lifecycle.json");
const MODEL: ModelId = ModelId::WhisperBase;
const FINAL_NAME: &str = "ggml-base.bin";
const PARTIAL_NAME: &str = "ggml-base.bin.part";
const MODEL_BYTES: &[u8] = b"oracle model bytes";
const OPERATION_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "the fields mirror the versioned Lean state schema exactly"
)]
struct LifecycleState {
    published: bool,
    published_verified: bool,
    partial: bool,
    partial_verified: bool,
    publish_authorized: bool,
    cancel_requested: bool,
    readers: u64,
    writer: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LifecycleCase {
    name: String,
    kind: String,
    mode: String,
    scenario: String,
    start: LifecycleState,
    events: Vec<String>,
    expected: LifecycleState,
    expected_result: String,
    broken_expected: Option<LifecycleState>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LifecycleFixture {
    schema_version: u32,
    cases: Vec<LifecycleCase>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CaseStatus {
    Match,
    Mismatch,
    InfrastructureError,
}

impl CaseStatus {
    const fn label(self) -> &'static str {
        match self {
            Self::Match => "match",
            Self::Mismatch => "mismatch",
            Self::InfrastructureError => "infrastructure error",
        }
    }
}

#[derive(Debug)]
struct CaseOutcome {
    status: CaseStatus,
    actual_state: Option<LifecycleState>,
    actual_result: Option<String>,
}

fn load_fixture(source: &str) -> Result<LifecycleFixture, &'static str> {
    let fixture: LifecycleFixture =
        serde_json::from_str(source).map_err(|_| "fixture JSON is invalid")?;
    if fixture.schema_version != 1 {
        return Err("fixture schema version is unsupported");
    }
    let mut names = HashSet::new();
    for test_case in &fixture.cases {
        if !matches!(
            test_case.mode.as_str(),
            "strict" | "internal-fixture" | "model-only"
        ) {
            return Err("fixture mode is unsupported");
        }
        if !names.insert(test_case.name.as_str()) {
            return Err("fixture case name is duplicated");
        }
        if !matches!(
            test_case.kind.as_str(),
            "install" | "remove" | "sensitivity"
        ) {
            return Err("fixture case kind is unsupported");
        }
    }
    Ok(fixture)
}

#[derive(Debug)]
struct Observation {
    state: LifecycleState,
    result: String,
}

struct Environment {
    root: TempDir,
    store: ModelStore,
    manifest: ModelManifest,
    leases: Vec<ModelLease>,
    cancelled: Arc<AtomicBool>,
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn manifest(url: &str, hash: String) -> Result<ModelManifest, ()> {
    ModelManifest::new(vec![ModelSpec {
        id: MODEL,
        engine_version: "oracle".into(),
        url: url.parse().map_err(|_| ())?,
        size: u64::try_from(MODEL_BYTES.len()).map_err(|_| ())?,
        sha256: hash,
        license: "CC0-1.0".into(),
        file_name: FINAL_NAME.into(),
    }])
    .map_err(|_| ())
}

fn read_request(stream: &mut TcpStream) -> Result<(), ()> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 1_024];
    while !bytes.windows(4).any(|window| window == b"\r\n\r\n") {
        let count = stream.read(&mut buffer).map_err(|_| ())?;
        if count == 0 {
            return Err(());
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    Ok(())
}

fn spawn_server() -> Result<(String, thread::JoinHandle<Result<(), ()>>), ()> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|_| ())?;
    listener.set_nonblocking(true).map_err(|_| ())?;
    let address = listener.local_addr().map_err(|_| ())?;
    let handle = thread::spawn(move || {
        let deadline = Instant::now() + OPERATION_TIMEOUT;
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(());
                    }
                    thread::sleep(Duration::from_millis(5));
                }
                Err(_) => return Err(()),
            }
        };
        stream
            .set_read_timeout(Some(OPERATION_TIMEOUT))
            .map_err(|_| ())?;
        stream
            .set_write_timeout(Some(OPERATION_TIMEOUT))
            .map_err(|_| ())?;
        read_request(&mut stream)?;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            MODEL_BYTES.len()
        )
        .map_err(|_| ())?;
        stream.write_all(MODEL_BYTES).map_err(|_| ())?;
        stream.flush().map_err(|_| ())
    });
    Ok((format!("http://{address}/model.bin"), handle))
}

fn join_server(server: thread::JoinHandle<Result<(), ()>>) -> Result<(), ()> {
    server.join().map_err(|_| ())?
}

fn prepare_start(
    start: &LifecycleState,
    root: &TempDir,
    store: &ModelStore,
    spec: &ModelSpec,
) -> Result<Vec<ModelLease>, ()> {
    if start.writer || start.publish_authorized || start.partial_verified {
        return Err(());
    }
    if start.published_verified && !start.published {
        return Err(());
    }
    if start.published {
        let contents: &[u8] = if start.published_verified {
            MODEL_BYTES
        } else {
            b"invalid model"
        };
        fs::write(root.path().join(FINAL_NAME), contents).map_err(|_| ())?;
    }
    if start.partial {
        fs::write(root.path().join(PARTIAL_NAME), b"stale partial").map_err(|_| ())?;
    }
    let mut leases = Vec::new();
    for _ in 0..start.readers {
        leases.push(store.acquire(spec).map_err(|_| ())?);
    }
    Ok(leases)
}

fn observe_state(
    root: &TempDir,
    store: &ModelStore,
    spec: &ModelSpec,
    cancelled: &AtomicBool,
    readers: usize,
) -> Result<LifecycleState, ()> {
    let final_path = root.path().join(FINAL_NAME);
    #[allow(
        clippy::filetype_is_file,
        reason = "the oracle observes the production contract that rejects symlinks and special files"
    )]
    let published =
        fs::symlink_metadata(&final_path).is_ok_and(|metadata| metadata.file_type().is_file());
    let published_verified = if published {
        match store.acquire(spec) {
            Ok(_lease) => true,
            Err(
                ModelError::MissingModel { .. }
                | ModelError::SizeMismatch { .. }
                | ModelError::HashMismatch { .. },
            ) => false,
            Err(_) => return Err(()),
        }
    } else {
        false
    };
    let partial = fs::symlink_metadata(root.path().join(PARTIAL_NAME)).is_ok();
    let writer = if readers == 0 {
        match store.remove(MODEL) {
            Ok(()) => false,
            Err(ModelError::ModelInUse { .. }) => true,
            Err(_) => return Err(()),
        }
    } else {
        false
    };
    Ok(LifecycleState {
        published,
        published_verified,
        partial,
        partial_verified: false,
        publish_authorized: false,
        cancel_requested: cancelled.load(Ordering::Acquire),
        readers: u64::try_from(readers).map_err(|_| ())?,
        writer,
    })
}

fn result_label(result: &Result<(), ModelError>) -> String {
    match result {
        Ok(()) => "success",
        Err(ModelError::HashMismatch { .. }) => "hash-mismatch",
        Err(ModelError::Cancelled { .. }) => "cancelled",
        Err(ModelError::ModelInUse { .. }) => "model-in-use",
        Err(_) => "other-error",
    }
    .into()
}

fn build_environment(
    test_case: &LifecycleCase,
    url: &str,
    hash: String,
) -> Result<Environment, ()> {
    let root = TempDir::new().map_err(|_| ())?;
    let store = ModelStore::new(root.path());
    let manifest = manifest(url, hash)?;
    let spec = manifest.spec(MODEL).map_err(|_| ())?;
    let leases = prepare_start(&test_case.start, &root, &store, spec)?;
    let cancelled = Arc::new(AtomicBool::new(test_case.start.cancel_requested));
    Ok(Environment {
        root,
        store,
        manifest,
        leases,
        cancelled,
    })
}

fn run_install(test_case: &LifecycleCase, corrupt_hash: bool) -> Result<Observation, ()> {
    let (url, server) = spawn_server()?;
    let hash = if corrupt_hash {
        sha256(b"different model")
    } else {
        sha256(MODEL_BYTES)
    };
    let environment = build_environment(test_case, &url, hash)?;
    let installer = ModelInstaller::new_for_test(
        environment.manifest.clone(),
        environment.store.clone(),
        Duration::from_secs(1),
        Duration::from_secs(1),
    )
    .map_err(|_| ())?;
    let result = installer.install_with_cancellation(&[MODEL], &environment.cancelled);
    join_server(server)?;
    let state = observe_state(
        &environment.root,
        &environment.store,
        environment.manifest.spec(MODEL).map_err(|_| ())?,
        &environment.cancelled,
        environment.leases.len(),
    )?;
    Ok(Observation {
        state,
        result: result_label(&result),
    })
}

fn run_cancel_install(
    test_case: &LifecycleCase,
    selected: PublishCheckpoint,
) -> Result<Observation, ()> {
    let (url, server) = spawn_server()?;
    let environment = build_environment(test_case, &url, sha256(MODEL_BYTES))?;
    let (checkpoint_tx, checkpoint_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let resume_rx = Arc::new(Mutex::new(resume_rx));
    let observer_resume = Arc::clone(&resume_rx);
    let observer = Arc::new(move |checkpoint| {
        if checkpoint == selected
            && checkpoint_tx.send(()).is_ok()
            && let Ok(receiver) = observer_resume.lock()
        {
            let _ = receiver.recv_timeout(OPERATION_TIMEOUT);
        }
    });
    let installer = ModelInstaller::new_for_test_with_publish_observer(
        environment.manifest.clone(),
        environment.store.clone(),
        Duration::from_secs(1),
        Duration::from_secs(1),
        observer,
    )
    .map_err(|_| ())?;
    let worker_flag = Arc::clone(&environment.cancelled);
    let (result_tx, result_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        let result = installer.install_with_cancellation(&[MODEL], &worker_flag);
        let _ = result_tx.send(result);
    });
    if checkpoint_rx.recv_timeout(OPERATION_TIMEOUT).is_err() {
        environment.cancelled.store(true, Ordering::Release);
        let _ = resume_tx.send(());
        let _ = result_rx.recv_timeout(OPERATION_TIMEOUT);
        let _ = worker.join();
        let _ = join_server(server);
        return Err(());
    }
    environment.cancelled.store(true, Ordering::Release);
    resume_tx.send(()).map_err(|_| ())?;
    let result = result_rx.recv_timeout(OPERATION_TIMEOUT).map_err(|_| ())?;
    worker.join().map_err(|_| ())?;
    join_server(server)?;
    let state = observe_state(
        &environment.root,
        &environment.store,
        environment.manifest.spec(MODEL).map_err(|_| ())?,
        &environment.cancelled,
        environment.leases.len(),
    )?;
    Ok(Observation {
        state,
        result: result_label(&result),
    })
}

fn run_remove(test_case: &LifecycleCase) -> Result<Observation, ()> {
    let environment = build_environment(
        test_case,
        "http://127.0.0.1:9/not-used",
        sha256(MODEL_BYTES),
    )?;
    let result = environment.store.remove(MODEL);
    let state = observe_state(
        &environment.root,
        &environment.store,
        environment.manifest.spec(MODEL).map_err(|_| ())?,
        &environment.cancelled,
        environment.leases.len(),
    )?;
    Ok(Observation {
        state,
        result: result_label(&result),
    })
}

fn run_scenario(test_case: &LifecycleCase) -> Result<Observation, ()> {
    match test_case.scenario.as_str() {
        "verified-publish" => run_install(test_case, false),
        "hash-mismatch" => run_install(test_case, true),
        "cancel-before-authorization" => {
            run_cancel_install(test_case, PublishCheckpoint::BeforeAuthorization)
        }
        "cancel-after-authorization" => {
            run_cancel_install(test_case, PublishCheckpoint::AfterAuthorization)
        }
        "busy-remove" | "remove-success" => run_remove(test_case),
        _ => Err(()),
    }
}

fn state_differences(expected: &LifecycleState, actual: &LifecycleState) -> Vec<&'static str> {
    let mut fields = Vec::new();
    if expected.published != actual.published {
        fields.push("published");
    }
    if expected.published_verified != actual.published_verified {
        fields.push("publishedVerified");
    }
    if expected.partial != actual.partial {
        fields.push("partial");
    }
    if expected.partial_verified != actual.partial_verified {
        fields.push("partialVerified");
    }
    if expected.publish_authorized != actual.publish_authorized {
        fields.push("publishAuthorized");
    }
    if expected.cancel_requested != actual.cancel_requested {
        fields.push("cancelRequested");
    }
    if expected.readers != actual.readers {
        fields.push("readers");
    }
    if expected.writer != actual.writer {
        fields.push("writer");
    }
    fields
}

fn execute_case(test_case: &LifecycleCase) -> CaseOutcome {
    if test_case.mode == "model-only" {
        let actual_state = test_case.broken_expected.clone();
        let sensitive = actual_state
            .as_ref()
            .is_some_and(|state| state != &test_case.expected);
        return CaseOutcome {
            status: if sensitive {
                CaseStatus::Match
            } else {
                CaseStatus::Mismatch
            },
            actual_state,
            actual_result: Some("broken-sensitivity".into()),
        };
    }
    match run_scenario(test_case) {
        Ok(observation) => {
            let matches = state_differences(&test_case.expected, &observation.state).is_empty()
                && test_case.expected_result == observation.result;
            CaseOutcome {
                status: if matches {
                    CaseStatus::Match
                } else {
                    CaseStatus::Mismatch
                },
                actual_state: Some(observation.state),
                actual_result: Some(observation.result),
            }
        }
        Err(()) => CaseOutcome {
            status: CaseStatus::InfrastructureError,
            actual_state: None,
            actual_result: None,
        },
    }
}

fn print_case(test_case: &LifecycleCase, outcome: &CaseOutcome) {
    println!("case: {}", test_case.name);
    println!("status: {}", outcome.status.label());
    println!("expected-result: {}", test_case.expected_result);
    println!(
        "actual-result: {}",
        outcome.actual_result.as_deref().unwrap_or("unavailable")
    );
    println!("expected-state: {:?}", test_case.expected);
    match &outcome.actual_state {
        Some(state) => println!("actual-state: {state:?}"),
        None => println!("actual-state: unavailable"),
    }
}

fn strict(fixture: &LifecycleFixture) -> bool {
    let mut passed = true;
    for test_case in fixture
        .cases
        .iter()
        .filter(|test_case| test_case.mode != "model-only")
    {
        let outcome = execute_case(test_case);
        println!("{}: {}", test_case.name, outcome.status.label());
        passed &= outcome.status == CaseStatus::Match;
    }
    passed
}

fn report(fixture: &LifecycleFixture) -> bool {
    println!("| case | mode | events | status |");
    println!("| --- | --- | --- | --- |");
    let mut passed = true;
    for test_case in &fixture.cases {
        let outcome = execute_case(test_case);
        println!(
            "| {} | {} | {} | {} |",
            test_case.name,
            test_case.mode,
            test_case.events.join(", "),
            outcome.status.label()
        );
        passed &= outcome.status == CaseStatus::Match;
    }
    passed
}

fn run(args: &[String]) -> Result<bool, &'static str> {
    let fixture = load_fixture(FIXTURE)?;
    match args {
        [flag] if flag == "--strict" => Ok(strict(&fixture)),
        [flag] if flag == "--report" => Ok(report(&fixture)),
        [flag, name] if flag == "--case" => {
            let test_case = fixture
                .cases
                .iter()
                .find(|test_case| test_case.name == *name)
                .ok_or("unknown lifecycle case")?;
            let outcome = execute_case(test_case);
            print_case(test_case, &outcome);
            Ok(outcome.status == CaseStatus::Match)
        }
        _ => Err("usage: model_lifecycle_oracle --strict | --report | --case <name>"),
    }
}

fn main() -> ExitCode {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    match run(&args) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FIXTURE, load_fixture, run, state_differences};

    #[test]
    fn rejects_an_unknown_schema_version() {
        let source = FIXTURE.replacen("\"schemaVersion\":1", "\"schemaVersion\":2", 1);

        assert_eq!(
            load_fixture(&source).expect_err("schema version is rejected"),
            "fixture schema version is unsupported"
        );
    }

    #[test]
    fn rejects_an_unknown_case_mode() {
        let source = FIXTURE.replacen("\"mode\":\"strict\"", "\"mode\":\"unknown\"", 1);

        assert_eq!(
            load_fixture(&source).expect_err("case mode is rejected"),
            "fixture mode is unsupported"
        );
    }

    #[test]
    fn field_differences_name_the_changed_state_field() {
        let fixture = load_fixture(FIXTURE).expect("fixture is valid");
        let expected = &fixture.cases[0].expected;
        let mut actual = expected.clone();
        actual.publish_authorized = !actual.publish_authorized;

        assert_eq!(
            state_differences(expected, &actual),
            vec!["publishAuthorized"]
        );
    }

    #[test]
    fn rejects_an_unknown_case_name() {
        assert_eq!(
            run(&["--case".into(), "missing-case".into()]).expect_err("unknown case is rejected"),
            "unknown lifecycle case"
        );
    }
}
