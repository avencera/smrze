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

pub fn parse_transcript(text: &str) -> Option<Vec<SpeakerTurn>> {
    let mut structured_turns = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Some(turn) = parse_structured_line(trimmed) {
            structured_turns.push(turn);
        }
    }
    if !structured_turns.is_empty() {
        return Some(structured_turns);
    }

    let plain_text_turns = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| SpeakerTurn {
            start: 0.0,
            end: 0.0,
            speaker: "Speaker 1".to_owned(),
            text: line.to_owned(),
        })
        .collect::<Vec<_>>();
    if plain_text_turns.is_empty() {
        return None;
    }

    Some(plain_text_turns)
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

fn parse_structured_line(line: &str) -> Option<SpeakerTurn> {
    let closing_bracket = line.find(']')?;
    if !line.starts_with('[') {
        return None;
    }

    let time_range = &line[1..closing_bracket];
    let (start, end) = time_range.split_once('-')?;
    let remainder = line.get(closing_bracket + 1..)?.trim_start();
    let (speaker, text) = remainder.split_once(':')?;
    let text = text.trim();
    if text.is_empty() {
        return None;
    }

    Some(SpeakerTurn {
        start: parse_timestamp(start.trim())?,
        end: parse_timestamp(end.trim())?,
        speaker: speaker.trim().to_owned(),
        text: text.to_owned(),
    })
}

fn parse_timestamp(value: &str) -> Option<f64> {
    let mut parts = value.split(':');
    let hours: f64 = parts.next()?.parse().ok()?;
    let minutes: f64 = parts.next()?.parse().ok()?;
    let seconds: f64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }

    Some(hours * 3600.0 + minutes * 60.0 + seconds)
}

#[cfg(test)]
mod tests {
    use super::{format_timestamp, parse_transcript, render_transcript};
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

    #[test]
    fn parses_structured_transcript_lines() {
        let turns = parse_transcript("[00:00:01.000-00:00:02.500] Speaker 1: Hello world")
            .expect("transcript should parse");
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].speaker, "Speaker 1");
        assert_eq!(turns[0].text, "Hello world");
        assert_eq!(turns[0].start, 1.0);
        assert_eq!(turns[0].end, 2.5);
    }

    #[test]
    fn falls_back_to_plain_text_lines() {
        let turns = parse_transcript("first line\n\nsecond line").expect("transcript should parse");
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].speaker, "Speaker 1");
        assert_eq!(turns[1].text, "second line");
    }
}
