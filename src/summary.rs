use color_eyre::{Result, eyre::eyre};

use crate::foundation_models::{
    SummaryDocument, SummaryRequest, SummaryTurn, summarize_transcript,
};
use crate::speakers::SpeakerTurn;

pub fn generate_summary(title: &str, turns: &[SpeakerTurn]) -> Result<SummaryDocument> {
    let request = SummaryRequest {
        title: title.to_owned(),
        turns: turns
            .iter()
            .filter(|turn| !turn.text.trim().is_empty())
            .map(|turn| SummaryTurn {
                speaker: turn.speaker.clone(),
                text: turn.text.clone(),
            })
            .collect(),
    };
    if request.turns.is_empty() {
        return Err(eyre!("cannot summarize an empty transcript"));
    }

    summarize_transcript(request).map_err(|error| eyre!(error.message()))
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
