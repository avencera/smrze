use color_eyre::{
    Result,
    eyre::{Context, eyre},
};
use duct::cmd;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::probe::Hint;

use crate::utils::{SAMPLE_RATE, now_millis, short_hash};

#[derive(Debug, Clone)]
pub struct DecodedAudio {
    pub sample_rate: u32,
    pub samples: Vec<f32>,
}

pub fn decode_audio(path: &std::path::Path) -> Result<DecodedAudio> {
    match decode_audio_with_symphonia(path) {
        Ok(audio) => Ok(audio),
        Err(SymphoniaError::Unsupported(_)) => decode_audio_with_ffmpeg(path),
        Err(error) => Err(error.into()),
    }
}

fn decode_audio_with_symphonia(
    path: &std::path::Path,
) -> std::result::Result<DecodedAudio, SymphoniaError> {
    let file = std::fs::File::open(path).map_err(SymphoniaError::IoError)?;
    let media_source_stream = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
        hint.with_extension(extension);
    }

    let probed = symphonia::default::get_probe().format(
        &hint,
        media_source_stream,
        &FormatOptions::default(),
        &Default::default(),
    )?;
    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or(SymphoniaError::Unsupported("no default audio track found"))?;
    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or(SymphoniaError::Unsupported("missing sample rate"))?;
    let track_id = track.id;
    let mut decoder =
        symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;

    let mut samples = Vec::new();
    let mut sample_buffer = None;

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(SymphoniaError::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(error) => return Err(error),
        };

        if packet.track_id() != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(error) => return Err(error),
        };

        let spec = *decoded.spec();
        let channels = spec.channels.count();
        let required_capacity = decoded.capacity() as u64;
        let buffer =
            sample_buffer.get_or_insert_with(|| SampleBuffer::<f32>::new(required_capacity, spec));
        if buffer.capacity() < decoded.capacity() {
            *buffer = SampleBuffer::<f32>::new(required_capacity, spec);
        }
        buffer.copy_interleaved_ref(decoded);
        let interleaved = buffer.samples();

        if channels == 1 {
            samples.extend_from_slice(interleaved);
            continue;
        }

        for frame in interleaved.chunks(channels) {
            let sum = frame.iter().copied().sum::<f32>();
            samples.push(sum / channels as f32);
        }
    }

    Ok(DecodedAudio {
        sample_rate,
        samples,
    })
}

fn decode_audio_with_ffmpeg(path: &std::path::Path) -> Result<DecodedAudio> {
    let temp_path = std::env::temp_dir().join(format!(
        "smrze-{}-{}.wav",
        short_hash(&path.display().to_string()),
        now_millis()?
    ));
    let input_path = path.display().to_string();
    let output_path = temp_path.display().to_string();

    let conversion_result = cmd(
        "ffmpeg",
        [
            "-y",
            "-i",
            input_path.as_str(),
            "-vn",
            "-ac",
            "1",
            "-ar",
            "16000",
            "-c:a",
            "pcm_s16le",
            output_path.as_str(),
        ],
    )
    .stderr_to_stdout()
    .read();

    let ffmpeg_output = match conversion_result {
        Ok(output) => output,
        Err(error) => {
            return Err(eyre!("failed to transcode audio with ffmpeg: {error}"));
        }
    };

    let wav = load_wav_file(&temp_path).with_context(|| {
        format!(
            "ffmpeg created an unreadable wav for {}: {}",
            path.display(),
            ffmpeg_output.trim()
        )
    });

    let cleanup_result = std::fs::remove_file(&temp_path);
    if let Err(error) = cleanup_result
        && error.kind() != std::io::ErrorKind::NotFound
    {
        return Err(error).with_context(|| format!("failed to remove {}", temp_path.display()));
    }

    wav
}

fn load_wav_file(path: &std::path::Path) -> Result<DecodedAudio> {
    let mut reader = hound::WavReader::open(path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    let spec = reader.spec();
    let sample_rate = spec.sample_rate;
    let channels = usize::from(spec.channels.max(1));

    let interleaved = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<std::result::Result<Vec<_>, _>>()
            .with_context(|| format!("failed to read {}", path.display()))?,
        hound::SampleFormat::Int => match spec.bits_per_sample {
            0..=16 => reader
                .samples::<i16>()
                .map(|sample| sample.map(|value| f32::from(value) / f32::from(i16::MAX)))
                .collect::<std::result::Result<Vec<_>, _>>()
                .with_context(|| format!("failed to read {}", path.display()))?,
            _ => reader
                .samples::<i32>()
                .map(|sample| sample.map(|value| value as f32 / i32::MAX as f32))
                .collect::<std::result::Result<Vec<_>, _>>()
                .with_context(|| format!("failed to read {}", path.display()))?,
        },
    };

    let samples = if channels == 1 {
        interleaved
    } else {
        interleaved
            .chunks(channels)
            .map(|frame| frame.iter().copied().sum::<f32>() / channels as f32)
            .collect()
    };

    Ok(DecodedAudio {
        sample_rate,
        samples,
    })
}

pub fn normalize_audio(audio: &DecodedAudio) -> Vec<f32> {
    resample(&audio.samples, audio.sample_rate, SAMPLE_RATE)
}

pub fn audio_fingerprint(audio: &[f32]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&(audio.len() as u64).to_le_bytes());
    for sample in audio {
        hasher.update(&sample.to_le_bytes());
    }
    hasher.finalize().to_hex().to_string()
}

fn resample(data: &[f32], source_rate: u32, target_rate: u32) -> Vec<f32> {
    if source_rate == target_rate {
        return data.to_vec();
    }
    if data.is_empty() {
        return Vec::new();
    }
    if data.len() == 1 {
        let ratio = target_rate as f64 / source_rate as f64;
        return vec![data[0]; (data.len() as f64 * ratio).ceil() as usize];
    }

    let ratio = target_rate as f64 / source_rate as f64;
    let output_len = (data.len() as f64 * ratio).ceil() as usize;
    let mut output = Vec::with_capacity(output_len);
    for index in 0..output_len {
        let source_position = index as f64 / ratio;
        let left_index = source_position.floor() as usize;
        if left_index >= data.len() - 1 {
            output.push(*data.last().unwrap_or(&0.0));
            continue;
        }

        let fraction = source_position - left_index as f64;
        let left = data[left_index] as f64;
        let right = data[left_index + 1] as f64;
        output.push((left + (right - left) * fraction) as f32);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{audio_fingerprint, resample};

    #[test]
    fn fingerprint_changes_with_audio() {
        assert_ne!(
            audio_fingerprint(&[0.0, 1.0]),
            audio_fingerprint(&[0.0, 2.0])
        );
    }

    #[test]
    fn resample_returns_same_data_for_same_rate() {
        let input = vec![0.0, 1.0, 0.5];
        assert_eq!(resample(&input, 16_000, 16_000), input);
    }
}
