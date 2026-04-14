use crate::foundation_models::SummaryTurn;
use crate::speakers::SpeakerTurn;

pub(super) fn has_text(turns: &[SpeakerTurn]) -> bool {
    turns.iter().any(turn_has_text)
}

pub(super) fn summary_request_turns(turns: &[SpeakerTurn]) -> Vec<SummaryTurn> {
    let omit_speaker_labels = should_omit_speaker_labels(turns);
    turns
        .iter()
        .filter(|turn| turn_has_text(turn))
        .map(|turn| SummaryTurn {
            speaker: if omit_speaker_labels {
                String::new()
            } else {
                turn.speaker.clone()
            },
            text: turn.text.clone(),
        })
        .collect()
}

pub(super) fn render_gemma_summary_prompt(title: &str, turns: &[SpeakerTurn]) -> String {
    let transcript = summary_transcript_lines(turns).join("\n");

    format!(
        "Summarize this transcript in concise markdown for a human reader.\n\nTitle: {title}\n\nTranscript:\n{transcript}\n\nReturn markdown in exactly this structure:\n\n# Summary\n\n## Overview\nWrite 1 short paragraph summarizing the main topic and outcome.\n\n## Key Points\nWrite 4 to 8 bullet points covering the most important factual points from the transcript.\n\n## Decisions\nWrite bullet points for any decisions, conclusions, or recommendations that were explicitly made. If there were none, write `- None`.\n\n## Action Items\nWrite bullet points in the format `- Speaker X: action item` for any explicit next steps, commitments, or requests. If there were none, write `- None`.\n\nRules:\n- Use only details explicitly supported by the transcript.\n- Do not repeat the transcript verbatim.\n- Do not add any sections beyond the four required sections.\n- Do not omit any required section.\n- Keep the output concise and useful."
    )
}

pub(super) fn should_omit_speaker_labels(turns: &[SpeakerTurn]) -> bool {
    let mut speakers = turns
        .iter()
        .filter(|turn| turn_has_text(turn))
        .map(|turn| turn.speaker.trim())
        .filter(|speaker| !speaker.is_empty());
    let Some(first_speaker) = speakers.next() else {
        return true;
    };

    speakers.all(|speaker| speaker == first_speaker)
}

fn summary_transcript_lines(turns: &[SpeakerTurn]) -> Vec<String> {
    let omit_speaker_labels = should_omit_speaker_labels(turns);
    turns
        .iter()
        .filter(|turn| turn_has_text(turn))
        .map(|turn| {
            if omit_speaker_labels {
                return turn.text.trim().to_owned();
            }

            format!("{}: {}", turn.speaker.trim(), turn.text.trim())
        })
        .collect()
}

fn turn_has_text(turn: &SpeakerTurn) -> bool {
    !turn.text.trim().is_empty()
}
