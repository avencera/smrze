use color_eyre::{Result, eyre::eyre};
use std::path::Path;

use crate::foundation_models::{SummaryRequest, SummaryTurn, summarize_transcript};
use crate::gemma_models::generate_gemma_text;
use crate::paths::AppPaths;
use crate::speakers::SpeakerTurn;
use crate::summary_backend::{GemmaVariant, SummaryBackend};
use crate::utils::expand_path;

pub fn generate_summary(
    title: &str,
    turns: &[SpeakerTurn],
    backend: SummaryBackend,
    summary_model_dir: Option<&Path>,
    app_paths: &AppPaths,
) -> Result<String> {
    if filtered_turns(turns).is_empty() {
        return Err(eyre!("cannot summarize an empty transcript"));
    }

    match backend {
        SummaryBackend::AppleFoundation => generate_apple_summary(title, turns),
        SummaryBackend::Gemma4E2b | SummaryBackend::Gemma4E4b => {
            generate_gemma_summary(title, turns, backend, summary_model_dir, app_paths)
        }
    }
}

fn generate_apple_summary(title: &str, turns: &[SpeakerTurn]) -> Result<String> {
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
        "Summarize this transcript in concise markdown for a human reader\n\nTitle: {title}\n\nTranscript:\n{transcript}\n\nWrite a short, useful summary of the main claims, evidence, and conclusions. Use only details that are explicitly supported by the transcript. Do not repeat the transcript verbatim, do not add a title, and do not return an empty response."
    )
}

#[cfg(test)]
mod tests {
    use super::render_gemma_summary_prompt;
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
        assert!(prompt.contains("ALICE: Ship it"));
        assert!(prompt.contains("concise markdown"));
    }
}
