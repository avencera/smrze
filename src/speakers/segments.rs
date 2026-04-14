use scriptrs::TimedToken;
use speakrs::{
    DiarizationResult,
    pipeline::{FRAME_DURATION_SECONDS, FRAME_STEP_SECONDS},
    segment::Segment,
};

use super::DEFAULT_RAW_SPEAKER;

pub(super) fn exclusive_segments(diarization: &DiarizationResult) -> Vec<Segment> {
    let mut exclusive = diarization.discrete_diarization.clone();
    exclusive.make_exclusive();
    exclusive.to_segments(FRAME_STEP_SECONDS, FRAME_DURATION_SECONDS)
}

pub(super) fn assign_speaker(token: &TimedToken, segments: &[Segment]) -> String {
    if segments.is_empty() {
        return DEFAULT_RAW_SPEAKER.to_owned();
    }

    let mut best_overlap = 0.0f64;
    let mut best_speaker = None;
    for segment in segments {
        let overlap = overlap_seconds(token.start, token.end, segment.start, segment.end);
        if overlap > best_overlap {
            best_overlap = overlap;
            best_speaker = Some(segment.speaker.clone());
        }
    }
    if let Some(speaker) = best_speaker
        && best_overlap > 0.0
    {
        return speaker;
    }

    let token_midpoint = (token.start + token.end) / 2.0;
    segments
        .iter()
        .min_by(|left, right| {
            distance_to_segment(token_midpoint, left)
                .total_cmp(&distance_to_segment(token_midpoint, right))
        })
        .map(|segment| segment.speaker.clone())
        .unwrap_or_else(|| DEFAULT_RAW_SPEAKER.to_owned())
}

fn overlap_seconds(start_a: f64, end_a: f64, start_b: f64, end_b: f64) -> f64 {
    (end_a.min(end_b) - start_a.max(start_b)).max(0.0)
}

fn distance_to_segment(time: f64, segment: &Segment) -> f64 {
    if (segment.start..=segment.end).contains(&time) {
        return 0.0;
    }
    if time < segment.start {
        return segment.start - time;
    }
    time - segment.end
}
