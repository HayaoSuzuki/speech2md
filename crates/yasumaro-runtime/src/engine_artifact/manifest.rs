use std::collections::HashSet;
use std::path::{Component, PathBuf};

use serde::Deserialize;
use url::Url;

use super::{EngineArtifactError, Platform};

const EMBEDDED_MANIFEST: &str = include_str!("../../../../engines/manifest.json");

/// Metadata needed to verify and locate one packaged Whisper executable.
#[derive(Clone, Debug, Deserialize)]
pub struct EngineSpec {
    /// Upstream/package version identifier.
    pub version: String,
    /// Target operating system and CPU architecture.
    pub platform: Platform,
    /// HTTPS download location in the yasumaro GitHub Releases area.
    pub url: Url,
    /// Exact archive size in bytes.
    pub size: u64,
    /// Lowercase hexadecimal SHA-256 digest.
    pub sha256: String,
    /// File name used for the downloaded archive.
    pub archive_name: String,
    /// Relative path to `whisper-cli` inside the extracted archive.
    pub executable_path: PathBuf,
}

/// A validated collection of platform-specific Whisper engine artifacts.
#[derive(Clone, Debug, Deserialize)]
pub struct EngineManifest {
    artifacts: Vec<EngineSpec>,
}

impl EngineManifest {
    /// Builds and validates an engine manifest. An empty manifest is valid while
    /// release artifacts are being prepared.
    ///
    /// # Errors
    ///
    /// Returns an error for duplicate platforms or unsafe/incomplete metadata.
    pub fn new(artifacts: Vec<EngineSpec>) -> Result<Self, EngineArtifactError> {
        let manifest = Self { artifacts };
        manifest.validate()?;
        Ok(manifest)
    }

    /// Loads and validates the manifest embedded in this binary.
    ///
    /// # Errors
    ///
    /// Returns an error when the JSON is malformed or violates manifest invariants.
    pub fn embedded() -> Result<Self, EngineArtifactError> {
        let manifest: Self = serde_json::from_str(EMBEDDED_MANIFEST)
            .map_err(|error| EngineArtifactError::InvalidManifest(error.to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Returns all published artifact specifications.
    #[must_use]
    pub fn specs(&self) -> &[EngineSpec] {
        &self.artifacts
    }

    /// Selects the artifact published for `platform`.
    ///
    /// # Errors
    ///
    /// Returns [`EngineArtifactError::MissingArtifact`] when none is published.
    pub fn select(&self, platform: Platform) -> Result<&EngineSpec, EngineArtifactError> {
        self.artifacts
            .iter()
            .find(|spec| spec.platform == platform)
            .ok_or(EngineArtifactError::MissingArtifact(platform))
    }

    fn validate(&self) -> Result<(), EngineArtifactError> {
        let mut platforms = HashSet::new();
        for spec in &self.artifacts {
            let hash_is_valid = spec.sha256.len() == 64
                && spec
                    .sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
            let executable_is_relative_file = !spec.executable_path.as_os_str().is_empty()
                && !spec.executable_path.is_absolute()
                && spec
                    .executable_path
                    .components()
                    .all(|component| matches!(component, Component::Normal(_)));
            let archive_is_file_name = !spec.archive_name.trim().is_empty()
                && PathBuf::from(&spec.archive_name)
                    .components()
                    .all(|component| matches!(component, Component::Normal(_)));

            if !platforms.insert(spec.platform) {
                return Err(Self::invalid(format!(
                    "duplicate platform {}",
                    spec.platform
                )));
            }
            let version_is_safe = !spec.version.trim().is_empty()
                && !spec.version.contains(['/', '\\', ':'])
                && spec.version != "."
                && spec.version != "..";
            if !version_is_safe
                || spec.url.scheme() != "https"
                || spec.size == 0
                || !hash_is_valid
                || !archive_is_file_name
                || !executable_is_relative_file
            {
                return Err(Self::invalid(format!(
                    "invalid artifact specification for {}",
                    spec.platform
                )));
            }
        }
        Ok(())
    }

    const fn invalid(message: String) -> EngineArtifactError {
        EngineArtifactError::InvalidManifest(message)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use url::Url;

    use super::{EngineManifest, EngineSpec};
    use crate::engine_artifact::Platform;

    fn spec(platform: Platform) -> EngineSpec {
        let platform_name = platform.to_string();
        let extension = if platform == Platform::WindowsX86_64 {
            "zip"
        } else {
            "tar.gz"
        };
        let executable = if platform == Platform::WindowsX86_64 {
            "bin/whisper-cli.exe"
        } else {
            "bin/whisper-cli"
        };

        EngineSpec {
            version: "v1.0.0".into(),
            platform,
            url: Url::parse(&format!(
                "https://example.invalid/yasumaro-whispercpp-v1.0.0-{platform_name}.{extension}"
            ))
            .expect("fixture URL is valid"),
            size: 42,
            sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
            archive_name: format!("yasumaro-whispercpp-v1.0.0-{platform_name}.{extension}"),
            executable_path: executable.into(),
        }
    }

    #[test]
    fn selects_each_supported_platform() {
        let cases = [
            (
                Platform::WindowsX86_64,
                "yasumaro-whispercpp-v1.0.0-windows-x86_64.zip",
                "bin/whisper-cli.exe",
            ),
            (
                Platform::MacosAarch64,
                "yasumaro-whispercpp-v1.0.0-macos-aarch64.tar.gz",
                "bin/whisper-cli",
            ),
            (
                Platform::MacosX86_64,
                "yasumaro-whispercpp-v1.0.0-macos-x86_64.tar.gz",
                "bin/whisper-cli",
            ),
            (
                Platform::LinuxX86_64,
                "yasumaro-whispercpp-v1.0.0-linux-x86_64.tar.gz",
                "bin/whisper-cli",
            ),
        ];
        let manifest = EngineManifest::new(
            cases
                .iter()
                .map(|(platform, _, _)| spec(*platform))
                .collect(),
        )
        .expect("fixtures form a valid manifest");

        for (platform, archive_name, executable_path) in cases {
            let selected = manifest.select(platform).expect("platform is listed");
            assert_eq!(selected.archive_name, archive_name);
            assert_eq!(selected.executable_path.to_string_lossy(), executable_path);
        }
    }

    #[test]
    fn rejects_duplicate_platforms() {
        assert!(
            EngineManifest::new(vec![
                spec(Platform::LinuxX86_64),
                spec(Platform::LinuxX86_64),
            ])
            .is_err()
        );
    }

    #[test]
    fn rejects_non_https_url() {
        let mut invalid = spec(Platform::LinuxX86_64);
        invalid.url = Url::parse("http://example.invalid/engine.tar.gz").expect("fixture URL");
        assert!(EngineManifest::new(vec![invalid]).is_err());
    }

    #[test]
    fn rejects_invalid_sha256_or_zero_size() {
        let mut invalid_hash = spec(Platform::LinuxX86_64);
        invalid_hash.sha256 = "not-a-hash".into();
        assert!(EngineManifest::new(vec![invalid_hash]).is_err());

        let mut empty = spec(Platform::LinuxX86_64);
        empty.size = 0;
        assert!(EngineManifest::new(vec![empty]).is_err());
    }

    #[test]
    fn embedded_manifest_selects_the_published_windows_engine() {
        let manifest = EngineManifest::embedded().expect("embedded manifest is valid");
        let selected = manifest
            .select(Platform::WindowsX86_64)
            .expect("Windows artifact is published");

        assert_eq!(selected.version, "whispercpp-v1.9.4-speech2md.1");
        assert_eq!(
            selected.url.as_str(),
            "https://github.com/HayaoSuzuki/yasumaro/releases/download/whispercpp-v1.9.4-speech2md.1/speech2md-whispercpp-v1.9.4-windows-x86_64.zip"
        );
        assert_eq!(
            selected.archive_name,
            "speech2md-whispercpp-v1.9.4-windows-x86_64.zip"
        );
        assert_eq!(selected.size, 863_314);
        assert_eq!(
            selected.sha256,
            "c7d7b2eb2506910666b81bddd77ac35a7ad3b3c9eba648526085dcf8629f9862"
        );
        assert_eq!(selected.executable_path, Path::new("bin/whisper-cli.exe"));
    }
}
