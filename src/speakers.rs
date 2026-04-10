use scriptrs::TimedToken;
use speakrs::{
    DiarizationResult,
    pipeline::{FRAME_DURATION_SECONDS, FRAME_STEP_SECONDS},
    segment::Segment,
};
use std::collections::HashMap;

const MERGE_GAP_SECONDS: f64 = 0.75;
const DEFAULT_RAW_SPEAKER: &str = "SPEAKER_00";
const SHORT_INTRUSION_SECONDS: f64 = 1.5;
const MINORITY_SPEAKER_RATIO: f64 = 0.12;
const PREFERRED_SPLIT_SECONDS: f64 = 12.0;
const HARD_SPLIT_SECONDS: f64 = 30.0;

#[derive(Debug, Clone, PartialEq)]
pub struct SpeakerTurn {
    pub start: f64,
    pub end: f64,
    pub speaker: String,
    pub text: String,
}

pub fn build_turns(tokens: &[TimedToken], diarization: &DiarizationResult) -> Vec<SpeakerTurn> {
    if tokens.is_empty() {
        return Vec::new();
    }

    let segments = exclusive_segments(diarization);
    let mut raw_turns = collect_raw_turns(tokens, &segments);
    smooth_speakers(&mut raw_turns);
    raw_turns = merge_raw_turns(raw_turns);

    if raw_turns.is_empty() {
        return Vec::new();
    }

    let mut display_names = HashMap::new();
    let mut next_speaker_number = 1usize;
    raw_turns
        .into_iter()
        .map(|turn| {
            let display_name = display_names
                .entry(turn.speaker.clone())
                .or_insert_with(|| {
                    let label = format!("Speaker {next_speaker_number}");
                    next_speaker_number += 1;
                    label
                })
                .clone();
            SpeakerTurn {
                start: turn.start,
                end: turn.end,
                speaker: display_name,
                text: turn.text.trim().to_owned(),
            }
        })
        .collect()
}

fn collect_raw_turns(tokens: &[TimedToken], segments: &[Segment]) -> Vec<RawTurn> {
    let mut raw_turns: Vec<RawTurn> = Vec::new();
    for token in tokens.iter().filter(|token| !token.text.trim().is_empty()) {
        let speaker = assign_speaker(token, segments);
        let should_split_at_sentence = raw_turns.last().is_some_and(|current| {
            current.should_split_after
                && current.speaker == speaker
                && token.start - current.end <= MERGE_GAP_SECONDS
        });
        if should_split_at_sentence {
            raw_turns.push(RawTurn::new(token, speaker, true));
            continue;
        }

        let Some(current) = raw_turns.last_mut() else {
            raw_turns.push(RawTurn::new(token, speaker, false));
            continue;
        };

        if current.speaker != speaker || token.start - current.end > MERGE_GAP_SECONDS {
            raw_turns.push(RawTurn::new(token, speaker, false));
            continue;
        }
        if token.end - current.start > HARD_SPLIT_SECONDS {
            raw_turns.push(RawTurn::new(token, speaker, true));
            continue;
        }

        current.append_token(token);
    }
    raw_turns
}

fn smooth_speakers(turns: &mut [RawTurn]) {
    if turns.len() < 2 {
        return;
    }

    smooth_short_intrusions(turns);
    collapse_minority_speakers(turns);
}

fn smooth_short_intrusions(turns: &mut [RawTurn]) {
    if turns.len() < 3 {
        return;
    }

    for index in 1..turns.len() - 1 {
        let previous = &turns[index - 1];
        let next = &turns[index + 1];
        let current = &turns[index];
        if previous.speaker != next.speaker || current.speaker == previous.speaker {
            continue;
        }
        if current.duration() > SHORT_INTRUSION_SECONDS {
            continue;
        }

        turns[index].speaker = previous.speaker.clone();
    }
}

fn collapse_minority_speakers(turns: &mut [RawTurn]) {
    let mut durations = HashMap::<String, f64>::new();
    let mut total_duration = 0.0f64;
    for turn in turns.iter() {
        let duration = turn.duration();
        total_duration += duration;
        *durations.entry(turn.speaker.clone()).or_default() += duration;
    }

    if durations.len() < 2 || total_duration <= 0.0 {
        return;
    }

    let dominant = durations
        .iter()
        .max_by(|left, right| left.1.total_cmp(right.1))
        .map(|(speaker, _)| speaker.clone());
    let Some(dominant) = dominant else {
        return;
    };

    for turn in turns.iter_mut() {
        let ratio = durations.get(&turn.speaker).copied().unwrap_or_default() / total_duration;
        if turn.speaker != dominant && ratio <= MINORITY_SPEAKER_RATIO {
            turn.speaker = dominant.clone();
        }
    }
}

fn merge_raw_turns(turns: Vec<RawTurn>) -> Vec<RawTurn> {
    let mut merged: Vec<RawTurn> = Vec::new();
    for turn in turns {
        match merged.last_mut() {
            Some(current)
                if current.speaker == turn.speaker
                    && !turn.locked_from_previous
                    && turn.start - current.end <= MERGE_GAP_SECONDS =>
            {
                current.end = turn.end;
                current.text.push_str(&turn.text);
            }
            _ => merged.push(turn),
        }
    }
    merged
}

fn exclusive_segments(diarization: &DiarizationResult) -> Vec<Segment> {
    let mut exclusive = diarization.discrete_diarization.clone();
    exclusive.make_exclusive();
    exclusive.to_segments(FRAME_STEP_SECONDS, FRAME_DURATION_SECONDS)
}

fn assign_speaker(token: &TimedToken, segments: &[Segment]) -> String {
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

#[derive(Debug, Clone)]
struct RawTurn {
    start: f64,
    end: f64,
    speaker: String,
    text: String,
    locked_from_previous: bool,
    should_split_after: bool,
}

impl RawTurn {
    fn new(token: &TimedToken, speaker: String, locked_from_previous: bool) -> Self {
        let mut turn = Self {
            start: token.start,
            end: token.end,
            speaker,
            text: token.text.clone(),
            locked_from_previous,
            should_split_after: false,
        };
        turn.should_split_after =
            turn.duration() >= PREFERRED_SPLIT_SECONDS && token_ends_sentence(token);
        turn
    }

    fn append_token(&mut self, token: &TimedToken) {
        self.end = token.end;
        self.text.push_str(&token.text);
        self.should_split_after =
            self.duration() >= PREFERRED_SPLIT_SECONDS && token_ends_sentence(token);
    }

    fn duration(&self) -> f64 {
        self.end - self.start
    }
}

fn token_ends_sentence(token: &TimedToken) -> bool {
    matches!(token.text.trim_end().chars().last(), Some('.' | '?' | '!'))
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_RAW_SPEAKER, SpeakerTurn, build_turns};
    use ndarray::{Array2, Array3};
    use scriptrs::TimedToken;
    use speakrs::pipeline::{
        ChunkEmbeddings, ChunkSpeakerClusters, DecodedSegmentations, DiarizationResult,
        DiscreteDiarization, SpeakerCountTrack,
    };

    fn diarization_result(activations: Array2<f32>) -> DiarizationResult {
        DiarizationResult {
            segmentations: DecodedSegmentations(Array3::zeros((0, 0, 0))),
            embeddings: ChunkEmbeddings(Array3::zeros((0, 0, 0))),
            speaker_count: SpeakerCountTrack(vec![]),
            hard_clusters: ChunkSpeakerClusters(Array2::zeros((0, 0))),
            discrete_diarization: DiscreteDiarization(activations),
            segments: Vec::new(),
        }
    }

    fn token(start: f64, end: f64, text: &str) -> TimedToken {
        TimedToken {
            token_id: 0,
            text: text.to_owned(),
            start,
            end,
            confidence: 1.0,
        }
    }

    #[test]
    fn merges_short_same_speaker_pauses() {
        let diarization =
            diarization_result(Array2::from_shape_vec((4, 1), vec![1.0, 1.0, 1.0, 1.0]).unwrap());
        let turns = build_turns(
            &[token(0.0, 0.2, " hello"), token(0.5, 0.7, " world")],
            &diarization,
        );
        assert_eq!(
            turns,
            vec![SpeakerTurn {
                start: 0.0,
                end: 0.7,
                speaker: "Speaker 1".to_owned(),
                text: "hello world".to_owned(),
            }]
        );
    }

    #[test]
    fn falls_back_to_default_speaker_when_no_segments_exist() {
        let diarization = diarization_result(Array2::zeros((0, 0)));
        let turns = build_turns(&[token(0.0, 0.2, " hello")], &diarization);
        assert_eq!(turns[0].speaker, "Speaker 1");
        assert_eq!(DEFAULT_RAW_SPEAKER, "SPEAKER_00");
    }

    #[test]
    fn smooths_brief_speaker_intrusions() {
        let diarization = diarization_result(
            Array2::from_shape_vec(
                (7, 2),
                vec![
                    1.0, 0.0, //
                    1.0, 0.0, //
                    0.0, 1.0, //
                    0.0, 1.0, //
                    1.0, 0.0, //
                    1.0, 0.0, //
                    1.0, 0.0, //
                ],
            )
            .unwrap(),
        );
        let turns = build_turns(
            &[
                token(0.0, 0.2, " hello"),
                token(0.2, 0.4, " there"),
                token(0.4, 0.8, " my"),
                token(0.8, 1.0, " friend"),
                token(1.0, 1.2, " again"),
            ],
            &diarization,
        );
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].speaker, "Speaker 1");
    }

    #[test]
    fn splits_at_sentence_boundary_after_preferred_duration() {
        let diarization = diarization_result(Array2::zeros((0, 0)));
        let turns = build_turns(
            &[
                token(0.0, 4.0, " hello"),
                token(4.0, 8.0, " there"),
                token(8.0, 12.4, " friend."),
                token(12.4, 13.0, " next"),
                token(13.0, 13.5, " thought"),
            ],
            &diarization,
        );

        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].speaker, "Speaker 1");
        assert_eq!(turns[1].speaker, "Speaker 1");
        assert_eq!(turns[0].start, 0.0);
        assert_eq!(turns[0].end, 12.4);
        assert_eq!(turns[0].text, "hello there friend.");
        assert_eq!(turns[1].start, 12.4);
        assert_eq!(turns[1].end, 13.5);
        assert_eq!(turns[1].text, "next thought");
    }

    #[test]
    fn forces_split_before_exceeding_hard_duration() {
        let diarization = diarization_result(Array2::zeros((0, 0)));
        let turns = build_turns(
            &[
                token(0.0, 10.0, " one"),
                token(10.0, 20.0, " two"),
                token(20.0, 29.5, " three"),
                token(29.5, 30.5, " four"),
                token(30.5, 31.0, " five"),
            ],
            &diarization,
        );

        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].speaker, "Speaker 1");
        assert_eq!(turns[1].speaker, "Speaker 1");
        assert_eq!(turns[0].end, 29.5);
        assert_eq!(turns[0].text, "one two three");
        assert_eq!(turns[1].start, 29.5);
        assert_eq!(turns[1].text, "four five");
    }

    #[test]
    fn keeps_forced_same_speaker_splits_separate() {
        let diarization = diarization_result(Array2::zeros((0, 0)));
        let turns = build_turns(
            &[
                token(0.0, 6.0, " first"),
                token(6.0, 12.0, " sentence."),
                token(12.0, 18.0, " second"),
                token(18.0, 24.5, " sentence."),
            ],
            &diarization,
        );

        assert_eq!(turns.len(), 2);
        assert!(turns.iter().all(|turn| turn.speaker == "Speaker 1"));
        assert_eq!(turns[0].text, "first sentence.");
        assert_eq!(turns[1].text, "second sentence.");
    }
}
