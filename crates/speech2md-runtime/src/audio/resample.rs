use rubato::{FftFixedInOut, Resampler};

use crate::RuntimeError;

pub(super) const OUTPUT_SAMPLE_RATE: u32 = 16_000;

pub(super) fn resample(samples: &[f32], input_rate: u32) -> Result<Vec<f32>, RuntimeError> {
    if input_rate == OUTPUT_SAMPLE_RATE {
        return Ok(samples.to_vec());
    }
    let chunk_size = samples.len().clamp(64, 4096);
    let mut resampler = FftFixedInOut::<f32>::new(
        input_rate as usize,
        OUTPUT_SAMPLE_RATE as usize,
        chunk_size,
        1,
    )
    .map_err(|error| RuntimeError::Resample(error.to_string()))?;
    let input_frames = resampler.input_frames_next();
    let delay = resampler.output_delay();
    let mut position = 0;
    let mut normalized = Vec::new();
    while samples.len() - position >= input_frames {
        let input = [&samples[position..position + input_frames]];
        append_resampled(
            &mut normalized,
            resampler
                .process(&input, None)
                .map_err(|error| RuntimeError::Resample(error.to_string()))?,
        )?;
        position += input_frames;
    }
    if position < samples.len() {
        let input = [&samples[position..]];
        append_resampled(
            &mut normalized,
            resampler
                .process_partial(Some(&input), None)
                .map_err(|error| RuntimeError::Resample(error.to_string()))?,
        )?;
    }
    append_resampled(
        &mut normalized,
        resampler
            .process_partial::<&[f32]>(None, None)
            .map_err(|error| RuntimeError::Resample(error.to_string()))?,
    )?;

    let expected_length = samples
        .len()
        .saturating_mul(OUTPUT_SAMPLE_RATE as usize)
        .div_ceil(input_rate as usize);
    let available = normalized.len().saturating_sub(delay);
    if available < expected_length {
        return Err(RuntimeError::Resample(
            "resampler returned too few samples".into(),
        ));
    }
    normalized.drain(..delay);
    normalized.truncate(expected_length);
    Ok(normalized)
}

fn append_resampled(
    destination: &mut Vec<f32>,
    mut channels: Vec<Vec<f32>>,
) -> Result<(), RuntimeError> {
    let channel = channels
        .pop()
        .ok_or_else(|| RuntimeError::Resample("resampler returned no channel".into()))?;
    destination.extend(channel);
    Ok(())
}
