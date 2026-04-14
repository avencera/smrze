mod apple;
mod gemma;
mod prompt;

use color_eyre::{Result, eyre::eyre};
use std::path::Path;

use crate::console;
use crate::paths::AppPaths;
use crate::speakers::SpeakerTurn;
use crate::summary_backend::SummaryBackend;

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
    if !prompt::has_text(turns) {
        return Err(eyre!("cannot summarize an empty transcript"));
    }

    match mode {
        SummaryMode::Auto => match apple::generate_apple_summary(title, turns) {
            Ok(markdown) => Ok(GeneratedSummary {
                markdown,
                backend: SummaryBackend::AppleFoundation,
            }),
            Err(crate::foundation_models::SummaryError::Refusal { .. }) => {
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
            apple::generate_apple_summary(title, turns).map_err(|error| eyre!(error.message()))?
        }
        SummaryBackend::Gemma4E2b | SummaryBackend::Gemma4E4b => {
            gemma::generate_gemma_summary(title, turns, backend, summary_model_dir, app_paths)?
        }
    };
    Ok(GeneratedSummary { markdown, backend })
}

#[cfg(test)]
mod tests {
    use super::SummaryMode;
    use super::prompt::{render_gemma_summary_prompt, should_omit_speaker_labels};
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
