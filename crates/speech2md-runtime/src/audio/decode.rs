use std::fs::File;
use std::io::ErrorKind;
use std::path::Path;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::RuntimeError;

pub(super) fn decode_mono(input: &Path) -> Result<(Vec<f32>, u32), RuntimeError> {
    let source = MediaSourceStream::new(
        Box::new(File::open(input)?),
        MediaSourceStreamOptions::default(),
    );
    let format_options = FormatOptions {
        enable_gapless: true,
        ..FormatOptions::default()
    };
    let probed = symphonia::default::get_probe()
        .format(
            &Hint::new(),
            source,
            &format_options,
            &MetadataOptions::default(),
        )
        .map_err(|error| RuntimeError::Probe(error.to_string()))?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|track| {
            track.codec_params.codec != CODEC_TYPE_NULL && track.codec_params.sample_rate.is_some()
        })
        .ok_or(RuntimeError::NoAudioStream)?;
    let track_id = track.id;
    let expected_rate = track
        .codec_params
        .sample_rate
        .ok_or(RuntimeError::NoAudioStream)?;
    let expected_frames = track
        .codec_params
        .n_frames
        .and_then(|frames| usize::try_from(frames).ok());
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|error| RuntimeError::Decode(error.to_string()))?;
    let mut mono = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(error)) if error.kind() == ErrorKind::UnexpectedEof => {
                break;
            }
            Err(error) => return Err(RuntimeError::Decode(error.to_string())),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let audio_buffer = match decoder.decode(&packet) {
            Ok(buffer) => buffer,
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(error) => return Err(RuntimeError::Decode(error.to_string())),
        };
        append_mono(&mut mono, audio_buffer, expected_rate)?;
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
    audio_buffer: symphonia::core::audio::AudioBufferRef<'_>,
    expected_rate: u32,
) -> Result<(), RuntimeError> {
    if audio_buffer.spec().rate != expected_rate {
        return Err(RuntimeError::Decode(
            "sample rate changed within the stream".into(),
        ));
    }
    let channel_count = audio_buffer.spec().channels.count();
    let channel_count_u16 = u16::try_from(channel_count)
        .map_err(|_| RuntimeError::Decode("channel count exceeds u16".into()))?;
    let channel_divisor = f32::from(channel_count_u16);
    let mut interleaved =
        SampleBuffer::<f32>::new(audio_buffer.capacity() as u64, *audio_buffer.spec());
    interleaved.copy_interleaved_ref(audio_buffer);
    mono.extend(
        interleaved
            .samples()
            .chunks_exact(channel_count)
            .map(|frame| frame.iter().copied().sum::<f32>() / channel_divisor),
    );
    Ok(())
}
