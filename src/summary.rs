use color_eyre::{
    Result,
    eyre::{Context, eyre},
};
use serde::{Deserialize, Serialize};
use std::env;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::speakers::SpeakerTurn;

const HELPER_NAME: &str = "smrze-foundation-models";

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SummaryDocument {
    pub overview: String,
    pub key_points: Vec<String>,
    pub decisions: Vec<String>,
    pub action_items: Vec<ActionItem>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ActionItem {
    pub owner: Option<String>,
    pub task: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SummaryRequest {
    title: String,
    turns: Vec<SummaryTurn>,
}

#[derive(Serialize)]
struct SummaryTurn {
    speaker: String,
    text: String,
}

pub fn generate_summary(title: &str, turns: &[SpeakerTurn]) -> Result<SummaryDocument> {
    let helper_path = resolve_helper_path()?;
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

    run_helper(&helper_path, &request)
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

fn run_helper(helper_path: &Path, request: &SummaryRequest) -> Result<SummaryDocument> {
    let mut child = Command::new(helper_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to launch {}", helper_path.display()))?;

    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| eyre!("failed to open stdin for {}", helper_path.display()))?;
        serde_json::to_writer(&mut stdin, request)
            .with_context(|| format!("failed to write request to {}", helper_path.display()))?;
        stdin
            .flush()
            .with_context(|| format!("failed to flush stdin for {}", helper_path.display()))?;
    }

    let output = child
        .wait_with_output()
        .with_context(|| format!("failed to wait for {}", helper_path.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let message = if stderr.is_empty() {
            format!(
                "{} exited with status {}",
                helper_path.display(),
                output.status
            )
        } else {
            stderr
        };
        return Err(eyre!(message));
    }

    serde_json::from_slice(&output.stdout)
        .with_context(|| format!("failed to parse {} output", helper_path.display()))
}

fn resolve_helper_path() -> Result<PathBuf> {
    if let Some(path) = env::var_os("SMRZE_FOUNDATION_MODELS_BIN") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
    }

    let current_exe = env::current_exe().with_context(|| "failed to resolve current executable")?;
    let exe_dir = current_exe
        .parent()
        .ok_or_else(|| eyre!("current executable has no parent directory"))?;
    let sibling = exe_dir.join(HELPER_NAME);
    if sibling.is_file() {
        return Ok(sibling);
    }

    let workspace_candidate = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("apple-foundation-models")
        .join(".build")
        .join("arm64-apple-macosx")
        .join("debug")
        .join(HELPER_NAME);
    if workspace_candidate.is_file() {
        return Ok(workspace_candidate);
    }

    let workspace_release_candidate = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("apple-foundation-models")
        .join(".build")
        .join("arm64-apple-macosx")
        .join("release")
        .join(HELPER_NAME);
    if workspace_release_candidate.is_file() {
        return Ok(workspace_release_candidate);
    }

    Err(eyre!(
        "summary helper {HELPER_NAME} was not found next to smrze or in apple-foundation-models/.build. Run `just release` to install it"
    ))
}

#[cfg(test)]
mod tests {
    use super::{ActionItem, SummaryDocument, render_markdown};

    #[test]
    fn renders_markdown_sections() {
        let summary = SummaryDocument {
            overview: "A concise overview".to_owned(),
            key_points: vec!["Point one".to_owned(), "Point two".to_owned()],
            decisions: vec!["Ship on Friday".to_owned()],
            action_items: vec![
                ActionItem {
                    owner: Some("Carol".to_owned()),
                    task: "Run QA".to_owned(),
                },
                ActionItem {
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
