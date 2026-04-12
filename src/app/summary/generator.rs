use color_eyre::Result;
use std::path::Path;
use std::time::Instant;
use tracing::debug;

use super::input::SummaryInput;
use crate::cache::{
    CacheKind, SummaryCacheEntry, load_cache_entry, load_summary, store_summary, summary_cache_key,
};
use crate::console;
use crate::paths::AppPaths;
use crate::summary::{GeneratedSummary, SummaryMode, generate_summary};

pub(super) struct SummaryGenerator<'a> {
    app_paths: &'a AppPaths,
    force: bool,
}

impl<'a> SummaryGenerator<'a> {
    pub(super) fn new(app_paths: &'a AppPaths, force: bool) -> Self {
        Self { app_paths, force }
    }

    pub(super) fn summarize(
        &self,
        summary_input: &SummaryInput,
        summary_mode: SummaryMode,
        summary_model_dir: Option<&Path>,
    ) -> Result<GeneratedSummary> {
        let cache_key = summary_cache_key(
            &summary_input.source_key,
            &summary_input.transcript_hash,
            summary_mode,
            summary_model_dir,
        );
        if let Some(cached_summary) = load_cache_entry(
            self.app_paths,
            CacheKind::Summary,
            &cache_key,
            self.force,
            load_summary,
        )? {
            return Ok(GeneratedSummary {
                markdown: cached_summary.markdown,
                backend: cached_summary.backend,
            });
        }

        console::info(format!(
            "Generating summary with {}",
            summary_mode.requested_label()
        ));
        let summary_started = Instant::now();
        let generated_summary = generate_summary(
            &summary_input.display_name,
            &summary_input.turns,
            summary_mode,
            summary_model_dir,
            self.app_paths,
        )?;
        debug!(
            "Finished summary in {:.2}s",
            summary_started.elapsed().as_secs_f64()
        );
        store_summary(
            self.app_paths,
            SummaryCacheEntry {
                cache_key: &cache_key,
                source_key: &summary_input.source_key,
                display_name: &summary_input.display_name,
                transcript_hash: &summary_input.transcript_hash,
                requested_mode: summary_mode,
                summary_model_dir,
                markdown: &generated_summary.markdown,
                backend: generated_summary.backend,
            },
        )?;
        Ok(generated_summary)
    }
}
