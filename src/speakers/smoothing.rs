use std::collections::HashMap;

use super::{MINORITY_SPEAKER_RATIO, RawTurn, SHORT_INTRUSION_SECONDS};

pub(super) fn smooth_speakers(turns: &mut [RawTurn]) {
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
    let speaker_durations = speaker_durations(turns);
    let total_duration = speaker_durations.values().sum::<f64>();
    if speaker_durations.len() < 2 || total_duration <= 0.0 {
        return;
    }

    let dominant = speaker_durations
        .iter()
        .max_by(|left, right| left.1.total_cmp(right.1))
        .map(|(speaker, _)| speaker.clone());
    let Some(dominant) = dominant else {
        return;
    };

    for turn in turns.iter_mut() {
        let ratio = speaker_durations
            .get(&turn.speaker)
            .copied()
            .unwrap_or_default()
            / total_duration;
        if turn.speaker != dominant && ratio <= MINORITY_SPEAKER_RATIO {
            turn.speaker = dominant.clone();
        }
    }
}

fn speaker_durations(turns: &[RawTurn]) -> HashMap<String, f64> {
    let mut durations = HashMap::<String, f64>::new();
    for turn in turns {
        *durations.entry(turn.speaker.clone()).or_default() += turn.duration();
    }
    durations
}
