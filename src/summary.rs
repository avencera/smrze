use color_eyre::{Result, eyre::eyre};
use std::path::Path;

use crate::console;
use crate::foundation_models::{SummaryError, SummaryRequest, SummaryTurn, summarize_transcript};
use crate::gemma_models::generate_gemma_text;
use crate::paths::AppPaths;
use crate::speakers::SpeakerTurn;
use crate::summary_backend::{GemmaVariant, SummaryBackend};
use crate::utils::expand_path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummaryMode {
    Auto,
    Backend(SummaryBackend),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedSummary {
    pub markdown: String,
    pub backend: SummaryBackend,
}

impl SummaryMode {
    pub const fn requested_key(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Backend(backend) => backend.cache_key(),
        }
    }

    pub const fn requested_label(self) -> &'static str {
        match self {
            Self::Auto => SummaryBackend::AppleFoundation.display_name(),
            Self::Backend(backend) => backend.display_name(),
        }
    }
}

pub fn generate_summary(
    title: &str,
    turns: &[SpeakerTurn],
    mode: SummaryMode,
    summary_model_dir: Option<&Path>,
    app_paths: &AppPaths,
) -> Result<GeneratedSummary> {
    if !turns.iter().any(has_text) {
        return Err(eyre!("cannot summarize an empty transcript"));
    }

    match mode {
        SummaryMode::Auto => match generate_apple_summary(title, turns) {
            Ok(markdown) => Ok(GeneratedSummary {
                markdown,
                backend: SummaryBackend::AppleFoundation,
            }),
            Err(SummaryError::Refusal { .. }) => {
                console::info(
                    "Apple Foundation refused this transcript, falling back to Gemma 4 E2B",
                );
                generate_with_backend(
                    title,
                    turns,
                    SummaryBackend::Gemma4E2b,
                    summary_model_dir,
                    app_paths,
                )
            }
            Err(error) => Err(eyre!(error.message())),
        },
        SummaryMode::Backend(backend) => {
            generate_with_backend(title, turns, backend, summary_model_dir, app_paths)
        }
    }
}

fn generate_with_backend(
    title: &str,
    turns: &[SpeakerTurn],
    backend: SummaryBackend,
    summary_model_dir: Option<&Path>,
    app_paths: &AppPaths,
) -> Result<GeneratedSummary> {
    let markdown = match backend {
        SummaryBackend::AppleFoundation => {
            generate_apple_summary(title, turns).map_err(|error| eyre!(error.message()))?
        }
        SummaryBackend::Gemma4E2b | SummaryBackend::Gemma4E4b => {
            generate_gemma_summary(title, turns, backend, summary_model_dir, app_paths)?
        }
    };
    Ok(GeneratedSummary { markdown, backend })
}

fn generate_apple_summary(
    title: &str,
    turns: &[SpeakerTurn],
) -> std::result::Result<String, SummaryError> {
    let omit_speaker_labels = should_omit_speaker_labels(turns);
    let request = SummaryRequest {
        title: title.to_owned(),
        turns: turns
            .iter()
            .filter(|turn| has_text(turn))
            .map(|turn| SummaryTurn {
                speaker: if omit_speaker_labels {
                    String::new()
                } else {
                    turn.speaker.clone()
                },
                text: turn.text.clone(),
            })
            .collect(),
    };
    summarize_transcript(request)
}

fn generate_gemma_summary(
    title: &str,
    turns: &[SpeakerTurn],
    backend: SummaryBackend,
    summary_model_dir: Option<&Path>,
    app_paths: &AppPaths,
) -> Result<String> {
    let variant = backend.gemma_variant().ok_or_else(|| {
        eyre!(
            "summary backend {} is not a Gemma backend",
            backend.display_name()
        )
    })?;
    let prompt = render_gemma_summary_prompt(title, turns);
    let local_model_path =
        resolve_gemma_model_dir(summary_model_dir, variant)?.map(|path| path.display().to_string());
    let raw_summary = generate_gemma_text(
        variant.model_id().to_owned(),
        local_model_path,
        prompt,
        backend.gemma_max_new_tokens(),
        app_paths,
    )
    .map_err(|error| eyre!(error.message().to_owned()))?;
    Ok(raw_summary.trim().to_owned())
}

fn has_text(turn: &SpeakerTurn) -> bool {
    !turn.text.trim().is_empty()
}

fn resolve_gemma_model_dir(
    summary_model_dir: Option<&Path>,
    variant: GemmaVariant,
) -> Result<Option<std::path::PathBuf>> {
    let Some(path) = summary_model_dir else {
        return Ok(None);
    };

    let root = expand_path(path)?;
    Ok(Some(root.join(variant.dir_name())))
}

fn render_gemma_summary_prompt(title: &str, turns: &[SpeakerTurn]) -> String {
    let transcript = summary_transcript_lines(turns).join("\n");

    format!(
        "Summarize this transcript in concise markdown for a human reader.\n\nTitle: {title}\n\nTranscript:\n{transcript}\n\nReturn markdown in exactly this structure:\n\n# Summary\n\n## Overview\nWrite 1 short paragraph summarizing the main topic and outcome.\n\n## Key Points\nWrite 4 to 8 bullet points covering the most important factual points from the transcript.\n\n## Decisions\nWrite bullet points for any decisions, conclusions, or recommendations that were explicitly made. If there were none, write `- None`.\n\n## Action Items\nWrite bullet points in the format `- Speaker X: action item` for any explicit next steps, commitments, or requests. If there were none, write `- None`.\n\nRules:\n- Use only details explicitly supported by the transcript.\n- Do not repeat the transcript verbatim.\n- Do not add any sections beyond the four required sections.\n- Do not omit any required section.\n- Keep the output concise and useful."
    )
}

fn summary_transcript_lines(turns: &[SpeakerTurn]) -> Vec<String> {
    let omit_speaker_labels = should_omit_speaker_labels(turns);
    turns
        .iter()
        .filter(|turn| has_text(turn))
        .map(|turn| {
            if omit_speaker_labels {
                return turn.text.trim().to_owned();
            }

            format!("{}: {}", turn.speaker.trim(), turn.text.trim())
        })
        .collect()
}

fn should_omit_speaker_labels(turns: &[SpeakerTurn]) -> bool {
    let mut speakers = turns
        .iter()
        .filter(|turn| has_text(turn))
        .map(|turn| turn.speaker.trim())
        .filter(|speaker| !speaker.is_empty());
    let Some(first_speaker) = speakers.next() else {
        return true;
    };

    speakers.all(|speaker| speaker == first_speaker)
}

#[cfg(test)]
mod tests {
    use super::{SummaryMode, render_gemma_summary_prompt, should_omit_speaker_labels};
    use crate::speakers::SpeakerTurn;

    #[test]
    fn renders_gemma_prompt_with_transcript() {
        let prompt = render_gemma_summary_prompt(
            "Weekly sync",
            &[SpeakerTurn {
                speaker: "ALICE".to_owned(),
                text: "Ship it".to_owned(),
                start: 0.0,
                end: 10.0,
            }],
        );

        assert!(prompt.contains("Title: Weekly sync"));
        assert!(prompt.contains("Ship it"));
        assert!(prompt.contains("# Summary"));
        assert!(prompt.contains("## Overview"));
        assert!(prompt.contains("## Key Points"));
        assert!(prompt.contains("## Decisions"));
        assert!(prompt.contains("## Action Items"));
    }

    #[test]
    fn single_speaker_gemma_prompt_omits_labels() {
        let prompt = render_gemma_summary_prompt(
            "Weekly sync",
            &[SpeakerTurn {
                speaker: "Speaker 1".to_owned(),
                text: "Ship it".to_owned(),
                start: 0.0,
                end: 10.0,
            }],
        );

        assert!(prompt.contains("Transcript:\nShip it"));
        assert!(!prompt.contains("Speaker 1: Ship it"));
    }

    #[test]
    fn speaker_labels_are_only_omitted_for_single_speaker_transcripts() {
        assert!(should_omit_speaker_labels(&[SpeakerTurn {
            speaker: "Speaker 1".to_owned(),
            text: "Ship it".to_owned(),
            start: 0.0,
            end: 10.0,
        }]));
        assert!(!should_omit_speaker_labels(&[
            SpeakerTurn {
                speaker: "Speaker 1".to_owned(),
                text: "Ship it".to_owned(),
                start: 0.0,
                end: 10.0,
            },
            SpeakerTurn {
                speaker: "Speaker 2".to_owned(),
                text: "Review it".to_owned(),
                start: 11.0,
                end: 20.0,
            },
        ]));
    }

    #[test]
    fn auto_mode_uses_distinct_cache_key() {
        assert_eq!(SummaryMode::Auto.requested_key(), "auto");
    }
}
