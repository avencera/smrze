use crate::speakers::SpeakerTurn;
use scriptrs::TimedToken;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscriptToken {
    pub text: String,
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TranscriptTurnJson {
    pub start_ms: u64,
    pub end_ms: u64,
    pub speaker: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TranscriptWord {
    pub word: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

impl From<&TimedToken> for TranscriptToken {
    fn from(value: &TimedToken) -> Self {
        Self {
            text: value.text.clone(),
            start: value.start,
            end: value.end,
        }
    }
}

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

pub fn transcript_turns_json(turns: &[SpeakerTurn]) -> Vec<TranscriptTurnJson> {
    turns
        .iter()
        .filter(|turn| !turn.text.is_empty())
        .map(|turn| {
            let (start_ms, end_ms) = normalized_time_range(turn.start, turn.end, None);
            TranscriptTurnJson {
                start_ms,
                end_ms,
                speaker: turn.speaker.clone(),
                text: turn.text.clone(),
            }
        })
        .collect()
}

pub fn build_word_timings(tokens: &[TranscriptToken]) -> Vec<TranscriptWord> {
    let mut words = Vec::new();
    let mut current = PendingWord::default();
    let mut last_end_ms = None;

    for token in tokens.iter().filter(|token| !token.text.trim().is_empty()) {
        if token_starts_new_word(&token.text) && current.has_text() {
            if let Some(word) = current.finish(last_end_ms) {
                last_end_ms = Some(word.end_ms);
                words.push(word);
            }
        }

        current.push(token);
    }

    if let Some(word) = current.finish(last_end_ms) {
        words.push(word);
    }

    words
}

pub fn render_word_lines(words: &[TranscriptWord]) -> String {
    words
        .iter()
        .map(|word| {
            format!(
                "[{}-{}] {}",
                format_timestamp(word.start_ms as f64 / 1000.0),
                format_timestamp(word.end_ms as f64 / 1000.0),
                word.word
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
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
    let total_millis = seconds_to_millis(seconds);
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

fn seconds_to_millis(seconds: f64) -> u64 {
    if seconds.is_finite() && seconds >= 0.0 {
        (seconds * 1000.0).round() as u64
    } else {
        0
    }
}

fn normalized_time_range(start: f64, end: f64, min_start_ms: Option<u64>) -> (u64, u64) {
    let mut start_ms = seconds_to_millis(start);
    if let Some(min_start_ms) = min_start_ms {
        start_ms = start_ms.max(min_start_ms);
    }

    let mut end_ms = seconds_to_millis(end);
    if end_ms < start_ms {
        end_ms = start_ms;
    }
    (start_ms, end_ms)
}

fn token_starts_new_word(text: &str) -> bool {
    text.chars().next().is_some_and(char::is_whitespace)
}

fn trim_word_punctuation(text: &str) -> Option<&str> {
    let start = text
        .char_indices()
        .find_map(|(index, character)| character.is_alphanumeric().then_some(index))?;
    let end = text.char_indices().rev().find_map(|(index, character)| {
        character
            .is_alphanumeric()
            .then_some(index + character.len_utf8())
    })?;
    let trimmed = text.get(start..end)?.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

#[derive(Debug, Default)]
struct PendingWord {
    text: String,
    start: Option<f64>,
    end: Option<f64>,
}

impl PendingWord {
    fn has_text(&self) -> bool {
        self.start.is_some() && !self.text.trim().is_empty()
    }

    fn push(&mut self, token: &TranscriptToken) {
        let piece = token.text.trim_start_matches(char::is_whitespace);
        if piece.is_empty() {
            return;
        }

        if self.start.is_none() {
            self.start = Some(token.start);
        }
        self.end = Some(token.end);
        self.text.push_str(piece);
    }

    fn finish(&mut self, min_start_ms: Option<u64>) -> Option<TranscriptWord> {
        let text = std::mem::take(&mut self.text);
        let start = self.start.take()?;
        let end = self.end.take()?;
        let word = trim_word_punctuation(&text)?.to_owned();
        let (start_ms, end_ms) = normalized_time_range(start, end, min_start_ms);
        Some(TranscriptWord {
            word,
            start_ms,
            end_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        TranscriptToken, TranscriptWord, format_timestamp, parse_transcript, render_transcript,
        render_word_lines, transcript_turns_json,
    };
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
    fn transcript_turn_json_uses_millisecond_ranges() {
        let turns = transcript_turns_json(&[SpeakerTurn {
            start: 1.234,
            end: 2.345,
            speaker: "Speaker 1".to_owned(),
            text: "Hello".to_owned(),
        }]);
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].start_ms, 1234);
        assert_eq!(turns[0].end_ms, 2345);
        assert_eq!(turns[0].speaker, "Speaker 1");
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

    #[test]
    fn builds_word_timings_for_single_token_words() {
        let words = super::build_word_timings(&[
            TranscriptToken {
                text: " hello".to_owned(),
                start: 0.0,
                end: 0.4,
            },
            TranscriptToken {
                text: " world".to_owned(),
                start: 0.4,
                end: 0.8,
            },
        ]);
        assert_eq!(
            words,
            vec![
                TranscriptWord {
                    word: "hello".to_owned(),
                    start_ms: 0,
                    end_ms: 400,
                },
                TranscriptWord {
                    word: "world".to_owned(),
                    start_ms: 400,
                    end_ms: 800,
                }
            ]
        );
    }

    #[test]
    fn joins_multi_piece_words() {
        let words = super::build_word_timings(&[
            TranscriptToken {
                text: " can".to_owned(),
                start: 0.0,
                end: 0.1,
            },
            TranscriptToken {
                text: "'".to_owned(),
                start: 0.1,
                end: 0.2,
            },
            TranscriptToken {
                text: "t".to_owned(),
                start: 0.2,
                end: 0.3,
            },
        ]);
        assert_eq!(
            words,
            vec![TranscriptWord {
                word: "can't".to_owned(),
                start_ms: 0,
                end_ms: 300,
            }]
        );
    }

    #[test]
    fn trims_outer_punctuation_and_drops_empty_spans() {
        let words = super::build_word_timings(&[
            TranscriptToken {
                text: " ...".to_owned(),
                start: 0.0,
                end: 0.1,
            },
            TranscriptToken {
                text: " \"hello,\"".to_owned(),
                start: 0.1,
                end: 0.4,
            },
        ]);
        assert_eq!(
            words,
            vec![TranscriptWord {
                word: "hello".to_owned(),
                start_ms: 100,
                end_ms: 400,
            }]
        );
    }

    #[test]
    fn preserves_internal_hyphens() {
        let words = super::build_word_timings(&[
            TranscriptToken {
                text: " rock".to_owned(),
                start: 0.0,
                end: 0.1,
            },
            TranscriptToken {
                text: "-".to_owned(),
                start: 0.1,
                end: 0.2,
            },
            TranscriptToken {
                text: "n".to_owned(),
                start: 0.2,
                end: 0.3,
            },
            TranscriptToken {
                text: "-".to_owned(),
                start: 0.3,
                end: 0.4,
            },
            TranscriptToken {
                text: "roll".to_owned(),
                start: 0.4,
                end: 0.5,
            },
        ]);
        assert_eq!(
            words,
            vec![TranscriptWord {
                word: "rock-n-roll".to_owned(),
                start_ms: 0,
                end_ms: 500,
            }]
        );
    }

    #[test]
    fn renders_word_lines_with_timestamps() {
        let text = render_word_lines(&[TranscriptWord {
            word: "hello".to_owned(),
            start_ms: 1234,
            end_ms: 1567,
        }]);
        assert_eq!(text, "[00:00:01.234-00:00:01.567] hello");
    }
}
