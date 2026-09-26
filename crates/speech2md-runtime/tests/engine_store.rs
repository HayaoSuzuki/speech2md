use std::io::{Cursor, Write};
use std::sync::Arc;

use flate2::Compression;
use flate2::write::GzEncoder;
use sha2::{Digest, Sha256};
use speech2md_runtime::engine_artifact::{
    EngineArchiveSource, EngineArtifactError, EngineInstaller, EngineSpec, EngineStore, Platform,
};
use tempfile::TempDir;
use url::Url;
use zip::write::SimpleFileOptions;

#[derive(Clone)]
struct BytesSource {
    bytes: Arc<[u8]>,
    fail_after_write: bool,
}

impl EngineArchiveSource for BytesSource {
    fn download(
        &self,
        _spec: &EngineSpec,
        destination: &mut dyn Write,
    ) -> Result<(), EngineArtifactError> {
        destination
            .write_all(&self.bytes)
            .map_err(|error| EngineArtifactError::Storage(error.to_string()))?;
        if self.fail_after_write {
            return Err(EngineArtifactError::Download("interrupted fixture".into()));
        }
        Ok(())
    }
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn spec(platform: Platform, archive_name: &str, bytes: &[u8]) -> EngineSpec {
    let executable_path = if platform == Platform::WindowsX86_64 {
        "bin/whisper-cli.exe"
    } else {
        "bin/whisper-cli"
    };
    EngineSpec {
        version: "test-v1".into(),
        platform,
        url: Url::parse("https://example.invalid/engine").expect("fixture URL"),
        size: u64::try_from(bytes.len()).expect("fixture length fits u64"),
        sha256: sha256(bytes),
        archive_name: archive_name.into(),
        executable_path: executable_path.into(),
    }
}

fn zip_archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, contents) in entries {
        archive
            .start_file(*name, SimpleFileOptions::default())
            .expect("start ZIP entry");
        archive.write_all(contents).expect("write ZIP entry");
    }
    archive.finish().expect("finish ZIP").into_inner()
}

fn tar_gz_archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let output = Vec::new();
    let encoder = GzEncoder::new(output, Compression::default());
    let mut archive = tar::Builder::new(encoder);
    for (name, contents) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(u64::try_from(contents.len()).expect("fixture length fits u64"));
        header.set_mode(0o644);
        header.set_cksum();
        archive
            .append_data(&mut header, *name, Cursor::new(*contents))
            .expect("append tar entry");
    }
    archive
        .into_inner()
        .expect("finish tar")
        .finish()
        .expect("finish gzip")
}

fn zip_symlink_archive() -> Vec<u8> {
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    archive
        .add_symlink(
            "bin/whisper-cli.exe",
            "../../outside",
            SimpleFileOptions::default(),
        )
        .expect("append ZIP symlink");
    archive.finish().expect("finish ZIP").into_inner()
}

fn tar_link_archive(kind: tar::EntryType) -> Vec<u8> {
    let encoder = GzEncoder::new(Vec::new(), Compression::default());
    let mut archive = tar::Builder::new(encoder);
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(kind);
    header.set_mode(0o777);
    header.set_size(0);
    archive
        .append_link(&mut header, "bin/whisper-cli", "../../outside")
        .expect("append tar link");
    archive
        .into_inner()
        .expect("finish tar")
        .finish()
        .expect("finish gzip")
}

fn install(bytes: Vec<u8>, mut engine: EngineSpec, root: &TempDir) {
    engine.size = u64::try_from(bytes.len()).expect("fixture length fits u64");
    engine.sha256 = sha256(&bytes);
    EngineInstaller::new(
        BytesSource {
            bytes: bytes.into(),
            fail_after_write: false,
        },
        EngineStore::new(root.path()),
    )
    .install(&engine)
    .expect("fixture installs");
}

#[test]
fn installs_verified_zip_and_tar_gz_archives() {
    let cases = [
        (
            Platform::WindowsX86_64,
            "engine.zip",
            zip_archive(&[("bin/whisper-cli.exe", b"windows executable")]),
        ),
        (
            Platform::LinuxX86_64,
            "engine.tar.gz",
            tar_gz_archive(&[("bin/whisper-cli", b"linux executable")]),
        ),
    ];

    for (platform, archive_name, bytes) in cases {
        let root = TempDir::new().expect("temporary engine root");
        let engine = spec(platform, archive_name, &bytes);
        install(bytes, engine.clone(), &root);
        let installed = EngineStore::new(root.path())
            .require(&engine)
            .expect("installed engine is available");
        assert_eq!(
            std::fs::read(installed.executable()).expect("read executable"),
            if platform == Platform::WindowsX86_64 {
                b"windows executable".as_slice()
            } else {
                b"linux executable".as_slice()
            }
        );
    }
}

#[test]
fn verification_or_download_failure_leaves_no_partial_install() {
    let bytes = zip_archive(&[("bin/whisper-cli.exe", b"new executable")]);
    let root = TempDir::new().expect("temporary engine root");
    let mut engine = spec(Platform::WindowsX86_64, "engine.zip", &bytes);
    engine.sha256 = sha256(b"different bytes");
    let installer = EngineInstaller::new(
        BytesSource {
            bytes: bytes.clone().into(),
            fail_after_write: false,
        },
        EngineStore::new(root.path()),
    );
    assert!(matches!(
        installer.install(&engine),
        Err(EngineArtifactError::HashMismatch { .. })
    ));

    engine.sha256 = sha256(&bytes);
    let interrupted = EngineInstaller::new(
        BytesSource {
            bytes: bytes.into(),
            fail_after_write: true,
        },
        EngineStore::new(root.path()),
    );
    assert!(matches!(
        interrupted.install(&engine),
        Err(EngineArtifactError::Download(_))
    ));
    assert!(
        std::fs::read_dir(root.path())
            .expect("read root")
            .next()
            .is_none()
    );
}

#[test]
fn size_mismatch_and_invalid_archive_preserve_an_existing_engine() {
    let valid = zip_archive(&[("bin/whisper-cli.exe", b"verified executable")]);
    let root = TempDir::new().expect("temporary engine root");
    let engine = spec(Platform::WindowsX86_64, "engine.zip", &valid);
    install(valid, engine.clone(), &root);
    let executable = EngineStore::new(root.path())
        .require(&engine)
        .expect("engine exists")
        .executable()
        .to_path_buf();

    let short = zip_archive(&[("bin/whisper-cli.exe", b"short")]);
    let mut replacement = engine;
    replacement.version = "test-v2".into();
    replacement.size = u64::try_from(short.len() + 1).expect("fixture length fits u64");
    let installer = EngineInstaller::new(
        BytesSource {
            bytes: short.into(),
            fail_after_write: false,
        },
        EngineStore::new(root.path()),
    );
    assert!(matches!(
        installer.install(&replacement),
        Err(EngineArtifactError::SizeMismatch { .. })
    ));
    assert_eq!(
        std::fs::read(executable).expect("read original executable"),
        b"verified executable"
    );
}

#[test]
fn rejects_archive_paths_outside_the_staging_directory() {
    let root = TempDir::new().expect("temporary engine root");
    let outside = root
        .path()
        .parent()
        .expect("root has parent")
        .join("escape");
    let attacks = [
        zip_archive(&[("../escape", b"outside")]),
        zip_archive(&[("C:/escape", b"outside")]),
        zip_archive(&[("/escape", b"outside")]),
    ];

    for bytes in attacks {
        let engine = spec(Platform::WindowsX86_64, "engine.zip", &bytes);
        let installer = EngineInstaller::new(
            BytesSource {
                bytes: bytes.into(),
                fail_after_write: false,
            },
            EngineStore::new(root.path()),
        );
        assert!(matches!(
            installer.install(&engine),
            Err(EngineArtifactError::InvalidArchive(_))
        ));
        assert!(!outside.exists());
    }
}

#[test]
fn rejects_archive_without_the_manifest_executable() {
    let bytes = tar_gz_archive(&[("bin/not-whisper", b"wrong executable")]);
    let root = TempDir::new().expect("temporary engine root");
    let engine = spec(Platform::LinuxX86_64, "engine.tar.gz", &bytes);
    let installer = EngineInstaller::new(
        BytesSource {
            bytes: bytes.into(),
            fail_after_write: false,
        },
        EngineStore::new(root.path()),
    );

    assert!(matches!(
        installer.install(&engine),
        Err(EngineArtifactError::InvalidArchive(_))
    ));
}

#[test]
fn rejects_symbolic_and_hard_links() {
    let attacks = [
        (Platform::WindowsX86_64, "engine.zip", zip_symlink_archive()),
        (
            Platform::LinuxX86_64,
            "engine.tar.gz",
            tar_link_archive(tar::EntryType::Symlink),
        ),
        (
            Platform::LinuxX86_64,
            "engine.tar.gz",
            tar_link_archive(tar::EntryType::Link),
        ),
    ];

    for (platform, name, bytes) in attacks {
        let root = TempDir::new().expect("temporary engine root");
        let engine = spec(platform, name, &bytes);
        let installer = EngineInstaller::new(
            BytesSource {
                bytes: bytes.into(),
                fail_after_write: false,
            },
            EngineStore::new(root.path()),
        );
        assert!(matches!(
            installer.install(&engine),
            Err(EngineArtifactError::InvalidArchive(_))
        ));
    }
}

#[test]
fn prune_preserves_a_leased_engine_then_removes_it_after_release() {
    let root = TempDir::new().expect("temporary engine root");
    let old_bytes = zip_archive(&[("bin/whisper-cli.exe", b"old")]);
    let keep_bytes = zip_archive(&[("bin/whisper-cli.exe", b"keep")]);
    let old = spec(Platform::WindowsX86_64, "old.zip", &old_bytes);
    let mut keep = spec(Platform::WindowsX86_64, "keep.zip", &keep_bytes);
    keep.version = "test-v2".into();
    install(old_bytes, old.clone(), &root);
    install(keep_bytes, keep.clone(), &root);
    let store = EngineStore::new(root.path());
    let lease = store
        .require(&old)
        .expect("old engine exists")
        .acquire()
        .expect("acquire engine lease");

    let first = store.prune(&keep).expect("prune while leased");
    assert_eq!(first.skipped_locked, 1);
    assert!(store.require(&old).is_ok());

    drop(lease);
    let second = store.prune(&keep).expect("prune after lease release");
    assert_eq!(second.removed, 1);
    assert!(store.require(&old).is_err());
    assert!(store.require(&keep).is_ok());
}

#[test]
fn require_rejects_an_executable_path_outside_its_installation() {
    let root = TempDir::new().expect("temporary engine root");
    let bytes = zip_archive(&[("bin/whisper-cli.exe", b"fixture")]);
    let mut engine = spec(Platform::WindowsX86_64, "engine.zip", &bytes);
    engine.executable_path = "../escape.exe".into();
    let version_root = root.path().join(&engine.version);
    std::fs::create_dir_all(&version_root).expect("create version root");
    std::fs::write(version_root.join("escape.exe"), b"outside installation")
        .expect("write outside executable");

    assert!(matches!(
        EngineStore::new(root.path()).require(&engine),
        Err(EngineArtifactError::InvalidManifest(_))
    ));
}
