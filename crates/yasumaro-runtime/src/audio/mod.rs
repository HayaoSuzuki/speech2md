mod decode;
mod pcm;
mod resample;

use std::path::Path;

pub use pcm::DecodedPcm;

use crate::RuntimeError;
use decode::decode_mono;
use pcm::store_pcm;
use resample::resample;

/// Decode an audio stream and normalize it to file-backed 16 kHz mono `f32` PCM.
///
/// # Errors
///
/// Returns an error when the input cannot be read or probed, contains no supported
/// audio stream, cannot be decoded or resampled, or the PCM backing store cannot be made.
pub fn decode_to_pcm(input: &Path, temp_root: &Path) -> Result<DecodedPcm, RuntimeError> {
    tracing::info!(target: "yasumaro_runtime::audio", "audio decode started");
    let (samples, input_rate) = decode_mono(input)?;
    let normalized = resample(&samples, input_rate)?;
    let decoded = store_pcm(&normalized, temp_root)?;
    tracing::info!(
        target: "yasumaro_runtime::audio",
        input_sample_rate = input_rate,
        output_sample_rate = decoded.sample_rate(),
        sample_count = decoded.samples().len(),
        "audio decode completed"
    );
    Ok(decoded)
}
