use crate::foundation_models::{SummaryError, SummaryRequest, summarize_transcript};
use crate::speakers::SpeakerTurn;

use super::prompt::summary_request_turns;

pub(super) fn generate_apple_summary(
    title: &str,
    turns: &[SpeakerTurn],
) -> std::result::Result<String, SummaryError> {
    summarize_transcript(SummaryRequest {
        title: title.to_owned(),
        turns: summary_request_turns(turns),
    })
}
