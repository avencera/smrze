mod raw_turn;
mod segments;

use scriptrs::TimedToken;
use serde::{Deserialize, Serialize};
use speakrs::{DiarizationResult, segment::Segment};
use std::collections::HashMap;

use raw_turn::RawTurn;
use segments::{assign_speaker, exclusive_segments};

pub(super) const MERGE_GAP_SECONDS: f64 = 0.75;
pub(super) const DEFAULT_RAW_SPEAKER: &str = "SPEAKER_00";
pub(super) const PREFERRED_SPLIT_SECONDS: f64 = 12.0;
pub(super) const HARD_SPLIT_SECONDS: f64 = 30.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    raw_turns = merge_raw_turns(raw_turns);

    if raw_turns.is_empty() {
        return Vec::new();
    }

    let mut display_names = HashMap::new();
    let mut next_speaker_number = 1usize;
    raw_turns
        .into_iter()
        .map(|turn| SpeakerTurn {
            start: turn.start,
            end: turn.end,
            speaker: display_names
                .entry(turn.speaker.clone())
                .or_insert_with(|| {
                    let label = format!("Speaker {next_speaker_number}");
                    next_speaker_number += 1;
                    label
                })
                .clone(),
            text: turn.text.trim().to_owned(),
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
    fn preserves_brief_speaker_intrusions() {
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
        assert_eq!(turns.len(), 3);
        assert_eq!(turns[0].speaker, "Speaker 1");
        assert_eq!(turns[1].speaker, "Speaker 2");
        assert_eq!(turns[2].speaker, "Speaker 1");
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
