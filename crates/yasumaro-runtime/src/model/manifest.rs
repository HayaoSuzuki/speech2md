use std::collections::HashSet;
use std::fmt;

use serde::Deserialize;
use url::Url;

use super::ModelError;

const EMBEDDED_MANIFEST: &str = include_str!("../../../../models/manifest.json");

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "kebab-case")]
pub enum ModelId {
    WhisperBase,
    WhisperSmall,
    WhisperMedium,
    WhisperLargeV3,
    WhisperLargeV3Turbo,
    SpeakerSegmentation,
    SpeakerEmbedding,
}

impl ModelId {
    pub(super) const fn file_name(self) -> &'static str {
        match self {
            Self::WhisperBase => "ggml-base.bin",
            Self::WhisperSmall => "ggml-small.bin",
            Self::WhisperMedium => "ggml-medium.bin",
            Self::WhisperLargeV3 => "ggml-large-v3.bin",
            Self::WhisperLargeV3Turbo => "ggml-large-v3-turbo.bin",
            Self::SpeakerSegmentation => "segmentation-3-0.onnx",
            Self::SpeakerEmbedding => "3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx",
        }
    }
}

impl fmt::Display for ModelId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::WhisperBase => "whisper-base",
            Self::WhisperSmall => "whisper-small",
            Self::WhisperMedium => "whisper-medium",
            Self::WhisperLargeV3 => "whisper-large-v3",
            Self::WhisperLargeV3Turbo => "whisper-large-v3-turbo",
            Self::SpeakerSegmentation => "speaker-segmentation",
            Self::SpeakerEmbedding => "speaker-embedding",
        };
        formatter.write_str(value)
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct ModelSpec {
    pub id: ModelId,
    pub engine_version: String,
    pub url: Url,
    pub size: u64,
    pub sha256: String,
    pub license: String,
    pub file_name: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ModelManifest {
    models: Vec<ModelSpec>,
}

impl ModelManifest {
    /// Builds and validates a model manifest.
    ///
    /// # Errors
    ///
    /// Returns an error for duplicate identifiers, invalid hashes, empty metadata,
    /// zero-sized models, or a file name that does not match its model identifier.
    pub fn new(models: Vec<ModelSpec>) -> Result<Self, ModelError> {
        let manifest = Self { models };
        manifest.validate()?;
        Ok(manifest)
    }

    /// Loads the model manifest embedded in the executable.
    ///
    /// # Errors
    ///
    /// Returns an error if the built-in JSON cannot be parsed or violates manifest invariants.
    pub fn embedded() -> Result<Self, ModelError> {
        let manifest: Self = serde_json::from_str(EMBEDDED_MANIFEST)
            .map_err(|error| ModelError::InvalidManifest(error.to_string()))?;
        manifest.validate()?;
        if manifest
            .models
            .iter()
            .any(|spec| spec.url.scheme() != "https")
        {
            return Err(ModelError::InvalidManifest(
                "embedded model URLs must use HTTPS".into(),
            ));
        }
        Ok(manifest)
    }

    #[must_use]
    pub fn specs(&self) -> &[ModelSpec] {
        &self.models
    }

    pub(super) fn spec(&self, id: ModelId) -> Result<&ModelSpec, ModelError> {
        self.models
            .iter()
            .find(|spec| spec.id == id)
            .ok_or_else(|| ModelError::InvalidManifest(format!("missing specification for {id}")))
    }

    fn validate(&self) -> Result<(), ModelError> {
        let mut ids = HashSet::new();
        for spec in &self.models {
            if !ids.insert(spec.id) {
                return Err(ModelError::InvalidManifest(format!(
                    "duplicate model identifier {}",
                    spec.id
                )));
            }
            let valid_hash = spec.sha256.len() == 64
                && spec
                    .sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
            if !valid_hash
                || spec.size == 0
                || spec.engine_version.trim().is_empty()
                || spec.license.trim().is_empty()
                || spec.file_name != spec.id.file_name()
            {
                return Err(ModelError::InvalidManifest(format!(
                    "invalid specification for {}",
                    spec.id
                )));
            }
        }
        Ok(())
    }
}
