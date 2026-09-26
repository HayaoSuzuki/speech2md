use std::fmt::Write as _;
use std::io::{self, Write as _};
use std::process::ExitCode;

use speech2md_runtime::engine_artifact::{
    EngineArchiveSource, EngineArtifactError, EngineInstaller, EngineManifest, EngineRootResolver,
    EngineSpec, EngineStore, HttpEngineArchiveSource, Platform,
};

use crate::args::{Cli, Command, EngineCommand};

const EXIT_SUCCESS: u8 = 0;
const EXIT_CONFIGURATION: u8 = 3;
const EXIT_ENGINE: u8 = 4;

pub fn execute(cli: &Cli) -> ExitCode {
    match execute_inner(cli) {
        Ok(message) => {
            let _ignored = writeln!(io::stdout().lock(), "{message}");
            ExitCode::from(EXIT_SUCCESS)
        }
        Err(error) => {
            let code = classify(&error);
            let _ignored = writeln!(io::stderr().lock(), "error: {error}");
            let _ignored = writeln!(io::stderr().lock(), "help: {}", help(&error));
            ExitCode::from(code)
        }
    }
}

fn execute_inner(cli: &Cli) -> Result<String, EngineArtifactError> {
    let manifest = EngineManifest::embedded()?;
    let store = EngineStore::new(EngineRootResolver::resolve()?);
    match &cli.command {
        Command::Engine { command } => {
            execute_engine(*command, &manifest, &store, Platform::current()?)
        }
    }
}

fn execute_engine(
    command: EngineCommand,
    manifest: &EngineManifest,
    store: &EngineStore,
    platform: Platform,
) -> Result<String, EngineArtifactError> {
    match command {
        EngineCommand::Install => {
            let spec = manifest.select(platform)?;
            let source = HttpEngineArchiveSource::new()?;
            install(source, store, spec)
        }
        EngineCommand::List => list(manifest, store),
        EngineCommand::Verify => {
            let spec = manifest.select(platform)?;
            store.require(spec)?;
            Ok(format!(
                "Verified whisper engine {} for {}.",
                spec.version, spec.platform
            ))
        }
        EngineCommand::Prune => {
            let spec = manifest.select(platform)?;
            let report = store.prune(spec)?;
            Ok(format!(
                "Pruned {} engine installation(s); kept {} locked installation(s).",
                report.removed, report.skipped_locked
            ))
        }
    }
}

fn install<S: EngineArchiveSource>(
    source: S,
    store: &EngineStore,
    spec: &EngineSpec,
) -> Result<String, EngineArtifactError> {
    EngineInstaller::new(source, store.clone()).install(spec)?;
    Ok(format!(
        "Installed whisper engine {} for {}.",
        spec.version, spec.platform
    ))
}

fn list(manifest: &EngineManifest, store: &EngineStore) -> Result<String, EngineArtifactError> {
    if manifest.specs().is_empty() {
        return Ok("No engine artifacts are published yet.".into());
    }
    let mut output = String::new();
    for spec in manifest.specs() {
        let status = if store.require(spec).is_ok() {
            "installed"
        } else {
            "not installed"
        };
        writeln!(output, "{} {}: {status}", spec.platform, spec.version)
            .map_err(|error| EngineArtifactError::Storage(error.to_string()))?;
    }
    Ok(output.trim_end().into())
}

const fn classify(error: &EngineArtifactError) -> u8 {
    match error {
        EngineArtifactError::UnsupportedPlatform { .. }
        | EngineArtifactError::InvalidManifest(_)
        | EngineArtifactError::InvalidRoot(_) => EXIT_CONFIGURATION,
        _ => EXIT_ENGINE,
    }
}

const fn help(error: &EngineArtifactError) -> &'static str {
    match error {
        EngineArtifactError::MissingArtifact(_) => {
            "no artifact has been published for this platform; check a newer speech2md release"
        }
        EngineArtifactError::MissingEngine { .. } | EngineArtifactError::CorruptEngine { .. } => {
            "run `speech2md engine install`"
        }
        EngineArtifactError::UnsupportedPlatform { .. } => {
            "use windows-x86_64, macos-aarch64, macos-x86_64, or linux-x86_64"
        }
        EngineArtifactError::Download(_) => {
            "check the network connection and retry `speech2md engine install`"
        }
        _ => "run `speech2md engine verify` for local engine state",
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use speech2md_runtime::engine_artifact::{EngineArchiveSource, EngineSpec};
    use tempfile::TempDir;
    use url::Url;

    use super::{
        EXIT_CONFIGURATION, EngineArtifactError, EngineCommand, EngineManifest, EngineStore,
        Platform, classify, execute_engine, install,
    };

    struct FailingSource;

    impl EngineArchiveSource for FailingSource {
        fn download(
            &self,
            _spec: &EngineSpec,
            _destination: &mut dyn Write,
        ) -> Result<(), EngineArtifactError> {
            Err(EngineArtifactError::Download("offline fixture".into()))
        }
    }

    fn spec(version: &str) -> EngineSpec {
        EngineSpec {
            version: version.into(),
            platform: Platform::WindowsX86_64,
            url: Url::parse("https://example.invalid/engine.zip").expect("fixture URL"),
            size: 1,
            sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
            archive_name: "engine.zip".into(),
            executable_path: "bin/whisper-cli.exe".into(),
        }
    }

    fn write_installed(root: &TempDir, engine: &EngineSpec) {
        let install_dir = root
            .path()
            .join(&engine.version)
            .join(engine.platform.to_string());
        let executable = install_dir.join(&engine.executable_path);
        std::fs::create_dir_all(executable.parent().expect("executable parent"))
            .expect("create engine directory");
        std::fs::write(executable, b"fixture executable").expect("write executable");
        std::fs::write(
            install_dir.join(".speech2md-integrity"),
            format!(
                "{}\n{}\n",
                engine.sha256, "6f1af2dfc4d7f16dacf404b1f6c9fd4a65cfffb8edde6dcf957463a0e41fb1ed"
            ),
        )
        .expect("write integrity receipt");
    }

    #[test]
    fn verify_distinguishes_installed_and_corrupt_engines() {
        let root = TempDir::new().expect("temporary engine root");
        let engine = spec("v1");
        let manifest = EngineManifest::new(vec![engine.clone()]).expect("fixture manifest");
        let store = EngineStore::new(root.path());

        assert!(matches!(
            execute_engine(EngineCommand::Verify, &manifest, &store, engine.platform),
            Err(EngineArtifactError::MissingEngine { .. })
        ));
        write_installed(&root, &engine);
        assert!(
            execute_engine(EngineCommand::Verify, &manifest, &store, engine.platform)
                .expect("installed engine verifies")
                .contains("Verified")
        );
        let executable = root
            .path()
            .join(&engine.version)
            .join(engine.platform.to_string())
            .join(&engine.executable_path);
        std::fs::write(executable, b"tampered executable").expect("modify executable");
        assert!(matches!(
            execute_engine(EngineCommand::Verify, &manifest, &store, engine.platform),
            Err(EngineArtifactError::CorruptEngine { .. })
        ));
    }

    #[test]
    fn install_surfaces_network_failure_from_the_injected_source() {
        let root = TempDir::new().expect("temporary engine root");
        let error = install(FailingSource, &EngineStore::new(root.path()), &spec("v1"))
            .expect_err("fake source fails");
        assert!(matches!(error, EngineArtifactError::Download(_)));
    }

    #[test]
    fn prune_keeps_an_engine_with_a_shared_lease() {
        let root = TempDir::new().expect("temporary engine root");
        let old = spec("v1");
        let keep = spec("v2");
        write_installed(&root, &old);
        write_installed(&root, &keep);
        let store = EngineStore::new(root.path());
        let lease = store
            .require(&old)
            .expect("old engine exists")
            .acquire()
            .expect("lease old engine");
        let manifest = EngineManifest::new(vec![keep.clone()]).expect("fixture manifest");

        let output = execute_engine(EngineCommand::Prune, &manifest, &store, keep.platform)
            .expect("prune succeeds");
        assert!(output.contains("kept 1 locked"));
        assert!(store.require(&old).is_ok());
        drop(lease);
    }

    #[test]
    fn unsupported_platform_is_a_configuration_error() {
        assert_eq!(
            classify(&EngineArtifactError::UnsupportedPlatform {
                os: "freebsd".into(),
                arch: "x86_64".into(),
            }),
            EXIT_CONFIGURATION
        );
    }
}
