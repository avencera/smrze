use super::DecodedAudio;
use crate::utils::SAMPLE_RATE;

pub(super) fn normalize_audio(audio: &DecodedAudio) -> Vec<f32> {
    resample(&audio.samples, audio.sample_rate, SAMPLE_RATE)
}

pub(super) fn resample(data: &[f32], source_rate: u32, target_rate: u32) -> Vec<f32> {
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
