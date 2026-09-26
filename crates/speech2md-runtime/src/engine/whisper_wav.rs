use std::path::Path;

use super::EngineError;

const WAV_HEADER_BYTES: u64 = 44;
const BYTES_PER_SAMPLE: u64 = 2;
const MINIMUM_TEMP_BYTES: u64 = 64 * 1_024 * 1_024;

/// Computes the additional temporary capacity required for Whisper staging.
///
/// # Errors
///
/// Returns [`EngineError::InsufficientTempSpace`] if byte arithmetic overflows.
pub fn required_whisper_temp_bytes(sample_count: usize) -> Result<u64, EngineError> {
    let samples = u64::try_from(sample_count).map_err(|_| EngineError::InsufficientTempSpace {
        required: u64::MAX,
        available: 0,
    })?;
    let wav_bytes = samples
        .checked_mul(BYTES_PER_SAMPLE)
        .and_then(|bytes| bytes.checked_add(WAV_HEADER_BYTES))
        .ok_or(EngineError::InsufficientTempSpace {
            required: u64::MAX,
            available: 0,
        })?;
    wav_bytes
        .checked_mul(2)
        .map(|bytes| bytes.max(MINIMUM_TEMP_BYTES))
        .ok_or(EngineError::InsufficientTempSpace {
            required: u64::MAX,
            available: 0,
        })
}

/// Writes finite 16 kHz mono float samples as finalized 16-bit PCM WAV.
///
/// # Errors
///
/// Returns an error for non-finite samples or any create/write/finalize failure.
pub fn write_whisper_wav(samples: &[f32], path: &Path) -> Result<(), EngineError> {
    let specification = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, specification)
        .map_err(|error| EngineError::Wav(error.to_string()))?;
    for (index, &sample) in samples.iter().enumerate() {
        if !sample.is_finite() {
            return Err(EngineError::InvalidSample { index });
        }
        let pcm = pcm_i16(sample);
        writer
            .write_sample(pcm)
            .map_err(|error| EngineError::Wav(error.to_string()))?;
    }
    writer
        .finalize()
        .map_err(|error| EngineError::Wav(error.to_string()))
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "the input is clamped and rounded to the complete i16 PCM range before conversion"
)]
fn pcm_i16(sample: f32) -> i16 {
    let clamped = sample.clamp(-1.0, 1.0);
    if clamped.is_sign_negative() {
        (clamped * 32_768.0).round() as i16
    } else {
        (clamped * 32_767.0).round() as i16
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{required_whisper_temp_bytes, write_whisper_wav};

    #[test]
    fn writes_one_second_of_mono_sixteen_bit_sixteen_kilohertz_pcm() {
        let directory = tempfile::tempdir().expect("temporary WAV directory");
        let path = directory.path().join("input.wav");
        write_whisper_wav(&vec![0.0; 16_000], &path).expect("write WAV");

        assert_eq!(fs::metadata(&path).expect("WAV metadata").len(), 32_044);
        let reader = hound::WavReader::open(path).expect("read WAV");
        assert_eq!(reader.spec().channels, 1);
        assert_eq!(reader.spec().sample_rate, 16_000);
        assert_eq!(reader.spec().bits_per_sample, 16);
    }

    #[test]
    fn rejects_nonfinite_samples_and_clamps_finite_amplitudes() {
        let directory = tempfile::tempdir().expect("temporary WAV directory");
        let path = directory.path().join("input.wav");
        assert!(write_whisper_wav(&[f32::NAN], &path).is_err());
        assert!(write_whisper_wav(&[f32::INFINITY], &path).is_err());

        write_whisper_wav(&[-2.0, 2.0], &path).expect("finite amplitudes are clamped");
        let samples = hound::WavReader::open(path)
            .expect("read WAV")
            .samples::<i16>()
            .collect::<Result<Vec<_>, _>>()
            .expect("read samples");
        assert_eq!(samples, vec![i16::MIN, i16::MAX]);
    }

    #[test]
    fn computes_bounded_temporary_space_and_rejects_overflow() {
        assert_eq!(
            required_whisper_temp_bytes(16_000).expect("one second fits"),
            64 * 1_024 * 1_024
        );
        assert!(required_whisper_temp_bytes(usize::MAX).is_err());
    }
}
