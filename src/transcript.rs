use crate::speakers::SpeakerTurn;

pub fn render_transcript(turns: &[SpeakerTurn]) -> String {
    if turns.is_empty() {
        return String::new();
    }

    let mut lines = Vec::with_capacity(turns.len());
    for turn in turns {
        if turn.text.is_empty() {
            continue;
        }
        lines.push(format!(
            "[{}-{}] {}: {}",
            format_timestamp(turn.start),
            format_timestamp(turn.end),
            turn.speaker,
            turn.text
        ));
    }
    lines.join("\n")
}

pub fn format_timestamp(seconds: f64) -> String {
    let safe_seconds = if seconds.is_finite() && seconds >= 0.0 {
        seconds
    } else {
        0.0
    };
    let total_millis = (safe_seconds * 1000.0).round() as u64;
    let hours = total_millis / 3_600_000;
    let minutes = (total_millis % 3_600_000) / 60_000;
    let secs = (total_millis % 60_000) / 1000;
    let millis = total_millis % 1000;
    format!("{hours:02}:{minutes:02}:{secs:02}.{millis:03}")
}

#[cfg(test)]
mod tests {
    use super::{format_timestamp, render_transcript};
    use crate::speakers::SpeakerTurn;

    #[test]
    fn formats_timestamp_with_millis() {
        assert_eq!(format_timestamp(65.432), "00:01:05.432");
    }

    #[test]
    fn renders_turn_per_line() {
        let transcript = render_transcript(&[SpeakerTurn {
            start: 1.0,
            end: 2.5,
            speaker: "Speaker 1".to_owned(),
            text: "Hello world".to_owned(),
        }]);
        assert_eq!(
            transcript,
            "[00:00:01.000-00:00:02.500] Speaker 1: Hello world"
        );
    }
}
