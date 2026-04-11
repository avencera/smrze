use color_eyre::{
    Result,
    eyre::{Context, eyre},
};
use gemma4_coreml::{
    GemmaGenerator, GenerateConfig as GemmaGenerateConfig, SummaryConfig as GemmaSummaryConfig,
    TranscriptSummarizer, TranscriptTurn as GemmaTranscriptTurn,
};
use std::path::Path;

use crate::foundation_models::{
    SummaryActionItem, SummaryDocument, SummaryRequest, SummaryTurn, summarize_transcript,
};
use crate::paths::AppPaths;
use crate::speakers::SpeakerTurn;
use crate::summary_backend::SummaryBackend;
use crate::utils::expand_path;

pub fn generate_summary(
    title: &str,
    turns: &[SpeakerTurn],
    backend: SummaryBackend,
    app_paths: &AppPaths,
    summary_model_dir: Option<&Path>,
) -> Result<SummaryDocument> {
    if filtered_turns(turns).is_empty() {
        return Err(eyre!("cannot summarize an empty transcript"));
    }

    match backend {
        SummaryBackend::AppleFoundation => generate_apple_summary(title, turns),
        SummaryBackend::Gemma4E2b | SummaryBackend::Gemma4E4b => {
            generate_gemma_summary(title, turns, backend, app_paths, summary_model_dir)
        }
    }
}

fn generate_apple_summary(title: &str, turns: &[SpeakerTurn]) -> Result<SummaryDocument> {
    let request = SummaryRequest {
        title: title.to_owned(),
        turns: filtered_turns(turns)
            .into_iter()
            .map(|turn| SummaryTurn {
                speaker: turn.speaker.clone(),
                text: turn.text.clone(),
            })
            .collect(),
    };
    summarize_transcript(request).map_err(|error| eyre!(error.message()))
}

fn generate_gemma_summary(
    title: &str,
    turns: &[SpeakerTurn],
    backend: SummaryBackend,
    app_paths: &AppPaths,
    summary_model_dir: Option<&Path>,
) -> Result<SummaryDocument> {
    let variant = backend.gemma_variant().ok_or_else(|| {
        eyre!(
            "summary backend {} is not a Gemma backend",
            backend.display_name()
        )
    })?;
    let model_root = resolve_gemma_model_root(app_paths, summary_model_dir)?;
    let bundle_root = model_root.join(variant.bundle_dir_name());
    let generator =
        GemmaGenerator::load(&bundle_root, backend.gemma_context()).with_context(|| {
            format!(
                "failed to load Gemma summary bundle {}",
                bundle_root.display()
            )
        })?;
    let summarizer = TranscriptSummarizer::new(&generator)
        .with_summary_config(GemmaSummaryConfig {
            max_chunk_chars: backend.gemma_max_chunk_chars(),
        })
        .with_generate_config(GemmaGenerateConfig {
            max_new_tokens: backend.gemma_max_new_tokens(),
        });
    let transcript_turns = filtered_turns(turns)
        .into_iter()
        .map(|turn| GemmaTranscriptTurn {
            speaker: turn.speaker,
            text: turn.text,
        })
        .collect::<Vec<_>>();
    let summary = summarizer.summarize(title, &transcript_turns)?;

    Ok(SummaryDocument {
        overview: summary.overview,
        key_points: summary.key_points,
        decisions: summary.decisions,
        action_items: summary
            .action_items
            .into_iter()
            .map(|action_item| SummaryActionItem {
                owner: action_item.owner,
                task: action_item.task,
            })
            .collect(),
    })
}

fn filtered_turns(turns: &[SpeakerTurn]) -> Vec<SpeakerTurn> {
    turns
        .iter()
        .filter(|turn| !turn.text.trim().is_empty())
        .cloned()
        .collect()
}

fn resolve_gemma_model_root(
    app_paths: &AppPaths,
    summary_model_dir: Option<&Path>,
) -> Result<std::path::PathBuf> {
    match summary_model_dir {
        Some(path) => expand_path(path),
        None => Ok(app_paths.gemma4_coreml_model_root()),
    }
}

pub fn render_markdown(summary: &SummaryDocument) -> String {
    let mut lines = vec![
        "# Summary".to_owned(),
        String::new(),
        "## Overview".to_owned(),
        summary.overview.trim().to_owned(),
        String::new(),
        "## Key Points".to_owned(),
    ];

    for key_point in &summary.key_points {
        lines.push(format!("- {}", key_point.trim()));
    }

    if !summary.decisions.is_empty() {
        lines.push(String::new());
        lines.push("## Decisions".to_owned());
        for decision in &summary.decisions {
            lines.push(format!("- {}", decision.trim()));
        }
    }

    if !summary.action_items.is_empty() {
        lines.push(String::new());
        lines.push("## Action Items".to_owned());
        for action_item in &summary.action_items {
            match action_item.owner.as_deref().map(str::trim) {
                Some(owner) if !owner.is_empty() => {
                    lines.push(format!("- {owner}: {}", action_item.task.trim()));
                }
                _ => lines.push(format!("- {}", action_item.task.trim())),
            }
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::{SummaryDocument, render_markdown};
    use crate::foundation_models::SummaryActionItem;

    #[test]
    fn renders_markdown_sections() {
        let summary = SummaryDocument {
            overview: "A concise overview".to_owned(),
            key_points: vec!["Point one".to_owned(), "Point two".to_owned()],
            decisions: vec!["Ship on Friday".to_owned()],
            action_items: vec![
                SummaryActionItem {
                    owner: Some("Carol".to_owned()),
                    task: "Run QA".to_owned(),
                },
                SummaryActionItem {
                    owner: None,
                    task: "Write release notes".to_owned(),
                },
            ],
        };

        assert_eq!(
            render_markdown(&summary),
            "# Summary\n\n## Overview\nA concise overview\n\n## Key Points\n- Point one\n- Point two\n\n## Decisions\n- Ship on Friday\n\n## Action Items\n- Carol: Run QA\n- Write release notes"
        );
    }

    #[test]
    fn omits_empty_optional_sections() {
        let summary = SummaryDocument {
            overview: "Overview only".to_owned(),
            key_points: vec!["One".to_owned()],
            decisions: Vec::new(),
            action_items: Vec::new(),
        };

        assert_eq!(
            render_markdown(&summary),
            "# Summary\n\n## Overview\nOverview only\n\n## Key Points\n- One"
        );
    }
}
