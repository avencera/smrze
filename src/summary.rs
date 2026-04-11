use color_eyre::{Result, eyre::eyre};
use std::path::Path;

use crate::foundation_models::{
    SummaryActionItem, SummaryDocument, SummaryRequest, SummaryTurn, summarize_transcript,
};
use crate::gemma_models::generate_gemma_text;
use crate::speakers::SpeakerTurn;
use crate::summary_backend::{GemmaVariant, SummaryBackend};
use crate::utils::expand_path;

pub fn generate_summary(
    title: &str,
    turns: &[SpeakerTurn],
    backend: SummaryBackend,
    summary_model_dir: Option<&Path>,
) -> Result<SummaryDocument> {
    if filtered_turns(turns).is_empty() {
        return Err(eyre!("cannot summarize an empty transcript"));
    }

    match backend {
        SummaryBackend::AppleFoundation => generate_apple_summary(title, turns),
        SummaryBackend::Gemma4E2b | SummaryBackend::Gemma4E4b => {
            generate_gemma_summary(title, turns, backend, summary_model_dir)
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
    summary_model_dir: Option<&Path>,
) -> Result<SummaryDocument> {
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
    )
    .map_err(|error| eyre!(error.message().to_owned()))?;
    parse_summary_document(raw_summary.trim())
}

fn filtered_turns(turns: &[SpeakerTurn]) -> Vec<SpeakerTurn> {
    turns
        .iter()
        .filter(|turn| !turn.text.trim().is_empty())
        .cloned()
        .collect()
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
    let transcript = filtered_turns(turns)
        .into_iter()
        .map(|turn| format!("{}: {}", turn.speaker.trim(), turn.text.trim()))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "You summarize transcripts into exactly four sections and never add a title, preamble, or extra commentary\n\nTitle: {title}\n\nTranscript:\n{transcript}\n\nReturn only this format:\n\nOverview:\n<one concise sentence>\n\nKey Points:\n- <important point>\n- <important point>\n\nDecisions:\n- <explicit decision>\nor\n- none\n\nAction Items:\n- <owner> | <task>\n- <task>\nor\n- none\n\nUse only details that are explicitly supported by the transcript"
    )
}

fn parse_summary_document(raw: &str) -> Result<SummaryDocument> {
    let mut section = None;
    let mut overview_parts = Vec::new();
    let mut key_points = Vec::new();
    let mut decisions = Vec::new();
    let mut action_items = Vec::new();

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Some(next_section) = parse_section_header(trimmed) {
            section = Some(next_section);
            continue;
        }

        match section {
            Some(SummarySection::Overview) => overview_parts.push(trimmed.to_owned()),
            Some(SummarySection::KeyPoints) => push_list_item(&mut key_points, trimmed),
            Some(SummarySection::Decisions) => push_list_item(&mut decisions, trimmed),
            Some(SummarySection::ActionItems) => push_action_item(&mut action_items, trimmed),
            None => continue,
        }
    }

    let overview = overview_parts.join(" ").trim().to_owned();
    if overview.is_empty() {
        return Err(eyre!("summary output was missing an Overview section"));
    }
    if key_points.is_empty() {
        return Err(eyre!("summary output was missing Key Points"));
    }

    Ok(SummaryDocument {
        overview,
        key_points,
        decisions,
        action_items,
    })
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SummarySection {
    Overview,
    KeyPoints,
    Decisions,
    ActionItems,
}

fn parse_section_header(line: &str) -> Option<SummarySection> {
    let normalized = line
        .trim_start_matches('#')
        .trim()
        .trim_matches('*')
        .trim()
        .trim_end_matches(':')
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();

    match normalized.as_str() {
        "overview" => Some(SummarySection::Overview),
        "keypoints" => Some(SummarySection::KeyPoints),
        "decisions" => Some(SummarySection::Decisions),
        "actionitems" => Some(SummarySection::ActionItems),
        _ => None,
    }
}

fn push_list_item(items: &mut Vec<String>, line: &str) {
    if let Some(item) = strip_list_marker(line) {
        if is_empty_list_item(&item) {
            return;
        }
        items.push(item);
    }
}

fn push_action_item(action_items: &mut Vec<SummaryActionItem>, line: &str) {
    let Some(item) = strip_list_marker(line) else {
        return;
    };
    if is_empty_list_item(&item) {
        return;
    }

    let mut parts = item.splitn(2, '|').map(str::trim);
    let first = parts.next().unwrap_or_default();
    let second = parts.next();

    let action_item = match second {
        Some(task) if !task.is_empty() => SummaryActionItem {
            owner: optional_not_empty(first),
            task: task.to_owned(),
        },
        _ => SummaryActionItem {
            owner: None,
            task: first.to_owned(),
        },
    };
    action_items.push(action_item);
}

fn strip_list_marker(line: &str) -> Option<String> {
    let item = line
        .strip_prefix("- ")
        .or_else(|| line.strip_prefix("* "))
        .unwrap_or(line)
        .trim();
    if item.is_empty() {
        return None;
    }
    Some(item.to_owned())
}

fn is_empty_list_item(item: &str) -> bool {
    matches!(item.trim().to_ascii_lowercase().as_str(), "none" | "n/a")
}

fn optional_not_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    Some(trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{
        SummaryDocument, parse_summary_document, render_gemma_summary_prompt, render_markdown,
    };
    use crate::foundation_models::SummaryActionItem;
    use crate::speakers::SpeakerTurn;

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

    #[test]
    fn parses_summary_document() {
        let summary = parse_summary_document(
            "Overview:\nA concise overview\n\nKey Points:\n- Point one\n- Point two\n\nDecisions:\n- Ship it\n\nAction Items:\n- Carol | Run QA\n- Write release notes",
        )
        .unwrap();

        assert_eq!(
            summary,
            SummaryDocument {
                overview: "A concise overview".to_owned(),
                key_points: vec!["Point one".to_owned(), "Point two".to_owned()],
                decisions: vec!["Ship it".to_owned()],
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
            }
        );
    }

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
        assert!(prompt.contains("ALICE: Ship it"));
        assert!(prompt.contains("Return only this format"));
    }
}
