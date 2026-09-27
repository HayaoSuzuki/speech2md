use std::fs::File;
use std::path::Path;

use symphonia::core::audio::GenericAudioBufferRef;
use symphonia::core::codecs::audio::{AudioDecoderOptions, CODEC_ID_NULL_AUDIO};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::formats::probe::Hint;
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;

use crate::RuntimeError;

pub(super) fn decode_mono(input: &Path) -> Result<(Vec<f32>, u32), RuntimeError> {
    let source = MediaSourceStream::new(
        Box::new(File::open(input)?),
        MediaSourceStreamOptions::default(),
    );
    let mut format = symphonia::default::get_probe()
        .probe(
            &Hint::new(),
            source,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|error| RuntimeError::Probe(error.to_string()))?;
    let track = format
        .tracks()
        .iter()
        .find(|track| {
            track
                .codec_params
                .as_ref()
                .and_then(|params| params.audio())
                .is_some_and(|params| {
                    params.codec != CODEC_ID_NULL_AUDIO && params.sample_rate.is_some()
                })
        })
        .ok_or(RuntimeError::NoAudioStream)?;
    let codec_params = track
        .codec_params
        .as_ref()
        .and_then(|params| params.audio())
        .ok_or(RuntimeError::NoAudioStream)?;
    let track_id = track.id;
    let expected_rate = codec_params
        .sample_rate
        .ok_or(RuntimeError::NoAudioStream)?;
    let expected_frames = track
        .num_frames
        .and_then(|frames| usize::try_from(frames).ok());
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(codec_params, &AudioDecoderOptions::default())
        .map_err(|error| RuntimeError::Decode(error.to_string()))?;
    let mut mono = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(error) => return Err(RuntimeError::Decode(error.to_string())),
        };
        if packet.track_id != track_id {
            continue;
        }
        let audio_buffer = match decoder.decode(&packet) {
            Ok(buffer) => buffer,
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(error) => return Err(RuntimeError::Decode(error.to_string())),
        };
        append_mono(&mut mono, &audio_buffer, expected_rate)?;
    }

    if mono.is_empty() {
        return Err(RuntimeError::NoAudioStream);
    }
    if let Some(frame_count) = expected_frames {
        mono.truncate(frame_count);
    }
    Ok((mono, expected_rate))
}

fn append_mono(
    mono: &mut Vec<f32>,
    audio_buffer: &GenericAudioBufferRef<'_>,
    expected_rate: u32,
) -> Result<(), RuntimeError> {
    if audio_buffer.spec().rate() != expected_rate {
        return Err(RuntimeError::Decode(
            "sample rate changed within the stream".into(),
        ));
    }
    let channel_count = audio_buffer.num_planes();
    if channel_count == 0 {
        return Err(RuntimeError::Decode("decoded audio has no channels".into()));
    }
    let channel_count_u16 = u16::try_from(channel_count)
        .map_err(|_| RuntimeError::Decode("channel count exceeds u16".into()))?;
    let channel_divisor = f32::from(channel_count_u16);
    let mut interleaved: Vec<f32> = Vec::new();
    audio_buffer.copy_to_vec_interleaved(&mut interleaved);
    mono.extend(
        interleaved
            .chunks_exact(channel_count)
            .map(|frame| frame.iter().copied().sum::<f32>() / channel_divisor),
    );
    Ok(())
}
