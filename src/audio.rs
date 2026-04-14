mod decode;
mod resample;

use color_eyre::Result;

#[derive(Debug, Clone)]
pub struct DecodedAudio {
    pub sample_rate: u32,
    pub samples: Vec<f32>,
}

pub fn decode_audio(path: &std::path::Path) -> Result<DecodedAudio> {
    decode::decode_audio(path)
}

pub(crate) fn convert_media_to_wav(
    media_path: &std::path::Path,
    output_path: &std::path::Path,
) -> Result<()> {
    decode::convert_media_to_wav(media_path, output_path)
}

pub fn normalize_audio(audio: &DecodedAudio) -> Vec<f32> {
    resample::normalize_audio(audio)
}

#[cfg(test)]
mod tests {
    use super::resample::resample;

    #[test]
    fn resample_returns_same_data_for_same_rate() {
        let input = vec![0.0, 1.0, 0.5];
        assert_eq!(resample(&input, 16_000, 16_000), input);
    }
}
