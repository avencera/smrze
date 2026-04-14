use scriptrs::TimedToken;

use super::PREFERRED_SPLIT_SECONDS;

#[derive(Debug, Clone)]
pub(super) struct RawTurn {
    pub(super) start: f64,
    pub(super) end: f64,
    pub(super) speaker: String,
    pub(super) text: String,
    pub(super) locked_from_previous: bool,
    pub(super) should_split_after: bool,
}

impl RawTurn {
    pub(super) fn new(token: &TimedToken, speaker: String, locked_from_previous: bool) -> Self {
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

    pub(super) fn append_token(&mut self, token: &TimedToken) {
        self.end = token.end;
        self.text.push_str(&token.text);
        self.should_split_after =
            self.duration() >= PREFERRED_SPLIT_SECONDS && token_ends_sentence(token);
    }

    pub(super) fn duration(&self) -> f64 {
        self.end - self.start
    }
}

fn token_ends_sentence(token: &TimedToken) -> bool {
    matches!(token.text.trim_end().chars().last(), Some('.' | '?' | '!'))
}
