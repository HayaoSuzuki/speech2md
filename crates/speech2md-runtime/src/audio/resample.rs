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

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{OUTPUT_SAMPLE_RATE, resample};

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn sixteen_kilohertz_preserves_every_sample(
            samples in prop::collection::vec(-1.0_f32..=1.0, 1..2_048)
        ) {
            let output = resample(&samples, OUTPUT_SAMPLE_RATE).expect("valid PCM is accepted");

            prop_assert_eq!(
                output.iter().map(|sample| sample.to_bits()).collect::<Vec<_>>(),
                samples.iter().map(|sample| sample.to_bits()).collect::<Vec<_>>()
            );
        }

        #[test]
        fn supported_rates_produce_the_integer_ceiling_length_and_finite_samples(
            samples in prop::collection::vec(-1.0_f32..=1.0, 1..8_192),
            input_rate in prop::sample::select(vec![8_000_u32, 22_050, 32_000, 44_100, 48_000]),
        ) {
            let output = resample(&samples, input_rate).expect("supported sample rate is accepted");
            let expected_length = samples.len()
                .saturating_mul(usize::try_from(OUTPUT_SAMPLE_RATE).expect("u32 fits usize"))
                .div_ceil(usize::try_from(input_rate).expect("selected u32 fits usize"));

            prop_assert_eq!(output.len(), expected_length);
            prop_assert!(output.iter().all(|sample| sample.is_finite()));
        }
    }
}
