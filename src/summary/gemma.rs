use color_eyre::{Result, eyre::eyre};
use std::path::{Path, PathBuf};

use crate::gemma_models::generate_gemma_text;
use crate::paths::AppPaths;
use crate::speakers::SpeakerTurn;
use crate::summary_backend::{GemmaVariant, SummaryBackend};
use crate::utils::expand_path;

use super::prompt::render_gemma_summary_prompt;

pub(super) fn generate_gemma_summary(
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

fn resolve_gemma_model_dir(
    summary_model_dir: Option<&Path>,
    variant: GemmaVariant,
) -> Result<Option<PathBuf>> {
    let Some(path) = summary_model_dir else {
        return Ok(None);
    };

    let root = expand_path(path)?;
    Ok(Some(root.join(variant.dir_name())))
}
