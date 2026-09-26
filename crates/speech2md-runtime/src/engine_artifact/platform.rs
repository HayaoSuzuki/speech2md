use std::fmt;

use serde::Deserialize;

use super::EngineArtifactError;

/// A platform for which speech2md can distribute a Whisper executable.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq)]
pub enum Platform {
    /// 64-bit Windows on x86.
    #[serde(rename = "windows-x86_64")]
    WindowsX86_64,
    /// macOS on Apple Silicon.
    #[serde(rename = "macos-aarch64")]
    MacosAarch64,
    /// macOS on 64-bit Intel.
    #[serde(rename = "macos-x86_64")]
    MacosX86_64,
    /// 64-bit Linux on x86.
    #[serde(rename = "linux-x86_64")]
    LinuxX86_64,
}

impl Platform {
    /// Resolves Rust target identifiers to a supported distribution platform.
    ///
    /// # Errors
    ///
    /// Returns [`EngineArtifactError::UnsupportedPlatform`] for all other pairs.
    pub fn from_target(os: &str, arch: &str) -> Result<Self, EngineArtifactError> {
        match (os, arch) {
            ("windows", "x86_64") => Ok(Self::WindowsX86_64),
            ("macos", "aarch64") => Ok(Self::MacosAarch64),
            ("macos", "x86_64") => Ok(Self::MacosX86_64),
            ("linux", "x86_64") => Ok(Self::LinuxX86_64),
            _ => Err(EngineArtifactError::UnsupportedPlatform {
                os: os.into(),
                arch: arch.into(),
            }),
        }
    }

    /// Resolves the platform on which this binary was compiled.
    ///
    /// # Errors
    ///
    /// Returns [`EngineArtifactError::UnsupportedPlatform`] on unsupported targets.
    pub fn current() -> Result<Self, EngineArtifactError> {
        Self::from_target(std::env::consts::OS, std::env::consts::ARCH)
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::WindowsX86_64 => "windows-x86_64",
            Self::MacosAarch64 => "macos-aarch64",
            Self::MacosX86_64 => "macos-x86_64",
            Self::LinuxX86_64 => "linux-x86_64",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Platform;

    #[test]
    fn rejects_unknown_platform() {
        assert!(Platform::from_target("freebsd", "x86_64").is_err());
        assert!(Platform::from_target("windows", "aarch64").is_err());
    }

    #[test]
    fn resolves_supported_targets() {
        let cases = [
            ("windows", "x86_64", Platform::WindowsX86_64),
            ("macos", "aarch64", Platform::MacosAarch64),
            ("macos", "x86_64", Platform::MacosX86_64),
            ("linux", "x86_64", Platform::LinuxX86_64),
        ];

        for (os, arch, expected) in cases {
            assert_eq!(
                Platform::from_target(os, arch).expect("target is supported"),
                expected
            );
        }
    }
}
