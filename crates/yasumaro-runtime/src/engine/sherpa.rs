use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use sherpa_onnx::{
    FastClusteringConfig, OfflineSpeakerDiarization, OfflineSpeakerDiarizationConfig,
    OfflineSpeakerSegmentationModelConfig, OfflineSpeakerSegmentationPyannoteModelConfig,
    SpeakerEmbeddingExtractorConfig,
};
use yasumaro_core::{SpeakerId, SpeakerTurn, TimeSpan, Timestamp};

use super::EngineError;

/// Per-recording speaker-count request.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DiarizationRequest {
    /// Exact number of speakers, or `None` to let clustering estimate it.
    pub num_speakers: Option<u32>,
}

impl DiarizationRequest {
    /// Validates values before entering the native engine.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InvalidConfig`] for zero or values outside the
    /// native signed integer range.
    pub fn validate(self) -> Result<(), EngineError> {
        if self.num_speakers == Some(0) {
            return Err(EngineError::InvalidConfig(
                "num_speakers must be greater than zero".into(),
            ));
        }
        if self
            .num_speakers
            .is_some_and(|count| count > i32::MAX as u32)
        {
            return Err(EngineError::InvalidConfig(
                "num_speakers exceeds the native engine range".into(),
            ));
        }
        Ok(())
    }
}

/// Engine-independent speaker diarization boundary used by the runtime pipeline.
pub trait Diarizer: Send + Sync {
    /// Produces speaker-labeled turns from normalized 16 kHz mono PCM.
    ///
    /// # Errors
    ///
    /// Returns a configuration, native processing, locking, or segment conversion error.
    fn diarize(
        &self,
        samples: &[f32],
        request: &DiarizationRequest,
    ) -> Result<Vec<SpeakerTurn>, EngineError>;
}

/// CPU-only sherpa-onnx speaker diarization adapter.
pub struct SherpaDiarizer {
    segmentation_model: String,
    embedding_model: String,
    num_threads: i32,
    native: Mutex<OfflineSpeakerDiarization>,
}

impl SherpaDiarizer {
    /// Loads both speaker models into a CPU-only native diarizer.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid paths, thread counts, or native initialization failure.
    pub fn new(
        segmentation_model: &Path,
        embedding_model: &Path,
        num_threads: usize,
    ) -> Result<Self, EngineError> {
        let automatic = DiarizationRequest { num_speakers: None };
        let config = build_config(segmentation_model, embedding_model, num_threads, automatic)?;
        let native = OfflineSpeakerDiarization::create(&config)
            .ok_or_else(|| EngineError::Diarization("could not initialize sherpa-onnx".into()))?;
        if native.sample_rate() != 16_000 {
            return Err(EngineError::InvalidConfig(format!(
                "sherpa-onnx expects {} Hz instead of 16000 Hz",
                native.sample_rate()
            )));
        }
        Ok(Self {
            segmentation_model: path_text(segmentation_model)?,
            embedding_model: path_text(embedding_model)?,
            num_threads: i32::try_from(num_threads).map_err(|error| {
                EngineError::InvalidConfig(format!("invalid thread count: {error}"))
            })?,
            native: Mutex::new(native),
        })
    }
}

impl Diarizer for SherpaDiarizer {
    fn diarize(
        &self,
        samples: &[f32],
        request: &DiarizationRequest,
    ) -> Result<Vec<SpeakerTurn>, EngineError> {
        request.validate()?;
        let config = build_config_from_text(
            self.segmentation_model.clone(),
            self.embedding_model.clone(),
            self.num_threads,
            *request,
        )?;
        let native = self
            .native
            .lock()
            .map_err(|_error| EngineError::Diarization("sherpa-onnx lock was poisoned".into()))?;
        native.set_config(&config);
        let result = native
            .process(samples)
            .ok_or_else(|| EngineError::Diarization("native processing failed".into()))?;
        let segments = result.sort_by_start_time();
        drop(result);
        drop(native);
        convert_segments(segments.into_iter().map(|segment| NativeSegment {
            start_seconds: segment.start,
            end_seconds: segment.end,
            speaker: segment.speaker,
        }))
    }
}

fn build_config(
    segmentation_model: &Path,
    embedding_model: &Path,
    num_threads: usize,
    request: DiarizationRequest,
) -> Result<OfflineSpeakerDiarizationConfig, EngineError> {
    let threads = i32::try_from(num_threads)
        .map_err(|error| EngineError::InvalidConfig(format!("invalid thread count: {error}")))?;
    build_config_from_text(
        path_text(segmentation_model)?,
        path_text(embedding_model)?,
        threads,
        request,
    )
}

fn build_config_from_text(
    segmentation_model: String,
    embedding_model: String,
    num_threads: i32,
    request: DiarizationRequest,
) -> Result<OfflineSpeakerDiarizationConfig, EngineError> {
    request.validate()?;
    if num_threads <= 0 {
        return Err(EngineError::InvalidConfig(
            "num_threads must be greater than zero".into(),
        ));
    }
    let num_clusters = request
        .num_speakers
        .map_or(Ok(-1), |count| {
            i32::try_from(count).map_err(|error| error.to_string())
        })
        .map_err(EngineError::InvalidConfig)?;
    Ok(OfflineSpeakerDiarizationConfig {
        segmentation: OfflineSpeakerSegmentationModelConfig {
            pyannote: OfflineSpeakerSegmentationPyannoteModelConfig {
                model: Some(segmentation_model),
                ..Default::default()
            },
            num_threads,
            debug: false,
            provider: Some("cpu".into()),
        },
        embedding: SpeakerEmbeddingExtractorConfig {
            model: Some(embedding_model),
            num_threads,
            debug: false,
            provider: Some("cpu".into()),
        },
        clustering: FastClusteringConfig {
            num_clusters,
            compute_confidence: false,
            ..Default::default()
        },
        ..Default::default()
    })
}

fn path_text(path: &Path) -> Result<String, EngineError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| EngineError::InvalidConfig("model paths must contain valid Unicode".into()))
}

#[derive(Clone, Copy, Debug)]
struct NativeSegment {
    start_seconds: f32,
    end_seconds: f32,
    speaker: i32,
}

fn convert_segments(
    segments: impl IntoIterator<Item = NativeSegment>,
) -> Result<Vec<SpeakerTurn>, EngineError> {
    let mut turns = segments
        .into_iter()
        .map(|segment| {
            if !segment.start_seconds.is_finite()
                || !segment.end_seconds.is_finite()
                || segment.start_seconds < 0.0
                || segment.end_seconds < segment.start_seconds
            {
                return Err(EngineError::InvalidSegment(
                    "speaker timestamps must be finite, non-negative, and ordered".into(),
                ));
            }
            let speaker = u32::try_from(segment.speaker).map_err(|_error| {
                EngineError::InvalidSegment("speaker identifier must be non-negative".into())
            })?;
            let start = seconds_to_millis(segment.start_seconds)?;
            let end = seconds_to_millis(segment.end_seconds)?;
            let span = TimeSpan::new(Timestamp::from_millis(start), Timestamp::from_millis(end))
                .map_err(|error| EngineError::InvalidSegment(error.to_string()))?;
            Ok(SpeakerTurn {
                span,
                speaker: SpeakerId::new(speaker),
                confidence: None,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    turns.sort_by_key(|turn| {
        (
            turn.span.start().as_millis(),
            turn.span.end().as_millis(),
            turn.speaker.as_u32(),
        )
    });
    Ok(turns)
}

fn seconds_to_millis(seconds: f32) -> Result<u64, EngineError> {
    let duration = Duration::try_from_secs_f32(seconds)
        .ok()
        .and_then(|value| value.checked_add(Duration::from_micros(500)))
        .ok_or_else(|| EngineError::InvalidSegment("speaker timestamp is out of range".into()))?;
    u64::try_from(duration.as_millis())
        .map_err(|error| EngineError::InvalidSegment(error.to_string()))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{DiarizationRequest, NativeSegment, build_config, convert_segments};
    use crate::engine::EngineError;

    #[test]
    fn rejects_zero_as_an_exact_speaker_count() {
        let request = DiarizationRequest {
            num_speakers: Some(0),
        };

        assert!(matches!(
            request.validate(),
            Err(EngineError::InvalidConfig(_))
        ));
    }

    #[test]
    fn builds_auto_and_exact_cluster_configs_for_cpu_models() {
        let automatic = build_config(
            Path::new("segmentation.onnx"),
            Path::new("embedding.onnx"),
            2,
            DiarizationRequest { num_speakers: None },
        )
        .expect("automatic clustering config is valid");
        let exact = build_config(
            Path::new("segmentation.onnx"),
            Path::new("embedding.onnx"),
            2,
            DiarizationRequest {
                num_speakers: Some(3),
            },
        )
        .expect("exact clustering config is valid");

        assert_eq!(automatic.clustering.num_clusters, -1);
        assert_eq!(exact.clustering.num_clusters, 3);
        assert_eq!(automatic.segmentation.num_threads, 2);
        assert_eq!(automatic.segmentation.provider.as_deref(), Some("cpu"));
        assert_eq!(
            automatic.segmentation.pyannote.model.as_deref(),
            Some("segmentation.onnx")
        );
        assert_eq!(automatic.embedding.model.as_deref(), Some("embedding.onnx"));
    }

    #[test]
    fn rounds_native_seconds_and_sorts_turns_by_start_time() {
        let turns = convert_segments([
            NativeSegment {
                start_seconds: 2.0,
                end_seconds: 2.5,
                speaker: 1,
            },
            NativeSegment {
                start_seconds: 1.2346,
                end_seconds: 1.9996,
                speaker: 0,
            },
        ])
        .expect("valid native segments convert");

        assert_eq!(turns[0].span.start().as_millis(), 1_235);
        assert_eq!(turns[0].span.end().as_millis(), 2_000);
        assert_eq!(turns[0].speaker.as_u32(), 0);
        assert_eq!(turns[1].span.start().as_millis(), 2_000);
    }

    #[test]
    fn rejects_negative_nonfinite_reversed_or_negative_speaker_segments() {
        let invalid = [
            NativeSegment {
                start_seconds: -0.1,
                end_seconds: 1.0,
                speaker: 0,
            },
            NativeSegment {
                start_seconds: f32::NAN,
                end_seconds: 1.0,
                speaker: 0,
            },
            NativeSegment {
                start_seconds: 2.0,
                end_seconds: 1.0,
                speaker: 0,
            },
            NativeSegment {
                start_seconds: 0.0,
                end_seconds: 1.0,
                speaker: -1,
            },
        ];

        for segment in invalid {
            assert!(matches!(
                convert_segments([segment]),
                Err(EngineError::InvalidSegment(_))
            ));
        }
    }
}
