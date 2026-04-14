use color_eyre::{Result, eyre::eyre};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use crate::paths::AppPaths;
use crate::summary::SummaryMode;
use crate::summary_backend::SummaryBackend;
use crate::utils::now_millis_u64;

use super::access::load_cache_entry;
use super::support::{
    CacheSpec, MANIFEST_FILE_NAME, cache_file_path, ensure_cache_entry_dir, load_manifest,
    write_manifest, write_text_file,
};

pub(crate) const SUMMARY_CACHE_SPEC: CacheSpec = CacheSpec::new(
    "summaries",
    std::time::Duration::from_secs(90 * 24 * 60 * 60),
);

#[derive(Debug, Clone)]
pub struct CachedSummary {
    pub markdown: String,
    pub backend: SummaryBackend,
}

#[derive(Debug, Clone)]
pub struct SummaryCacheEntry<'a> {
    pub cache_key: &'a str,
    pub source_key: &'a str,
    pub display_name: &'a str,
    pub transcript_hash: &'a str,
    pub requested_mode: SummaryMode,
    pub summary_model_dir: Option<&'a Path>,
    pub markdown: &'a str,
    pub backend: SummaryBackend,
}

#[derive(Debug, Serialize, Deserialize)]
struct SummaryManifest {
    created_at_ms: u64,
    source_key: String,
    display_name: String,
    transcript_hash: String,
    requested_mode: String,
    actual_backend: String,
    #[serde(default)]
    summary_model_dir: Option<String>,
    summary_file_name: String,
}

pub(crate) fn load_summary(app_paths: &AppPaths, cache_key: &str) -> Result<Option<CachedSummary>> {
    let Some(manifest) =
        load_manifest::<SummaryManifest>(app_paths, &SUMMARY_CACHE_SPEC, cache_key)?
    else {
        return Ok(None);
    };
    let summary_path = cache_file_path(
        app_paths,
        &SUMMARY_CACHE_SPEC,
        cache_key,
        &manifest.summary_file_name,
    );
    if !summary_path.exists() {
        return Ok(None);
    }

    let markdown = fs::read_to_string(&summary_path)?;
    Ok(Some(CachedSummary {
        markdown,
        backend: parse_summary_backend(&manifest.actual_backend)?,
    }))
}

pub fn load_cached_summary(
    app_paths: &AppPaths,
    cache_key: &str,
    force: bool,
) -> Result<Option<CachedSummary>> {
    load_cache_entry(
        app_paths,
        &SUMMARY_CACHE_SPEC,
        cache_key,
        force,
        load_summary,
    )
}

pub fn store_summary(app_paths: &AppPaths, entry: SummaryCacheEntry<'_>) -> Result<()> {
    let entry_dir = ensure_cache_entry_dir(app_paths, &SUMMARY_CACHE_SPEC, entry.cache_key)?;
    let summary_path = entry_dir.join("summary.md");
    write_text_file(&summary_path, entry.markdown)?;
    write_manifest(
        &entry_dir.join(MANIFEST_FILE_NAME),
        &SummaryManifest {
            created_at_ms: now_millis_u64()?,
            source_key: entry.source_key.to_owned(),
            display_name: entry.display_name.to_owned(),
            transcript_hash: entry.transcript_hash.to_owned(),
            requested_mode: entry.requested_mode.requested_key().to_owned(),
            actual_backend: entry.backend.cache_key().to_owned(),
            summary_model_dir: entry
                .summary_model_dir
                .map(|path| path.display().to_string()),
            summary_file_name: "summary.md".to_owned(),
        },
    )?;
    Ok(())
}

pub fn summary_cache_key(
    source_key: &str,
    transcript_hash: &str,
    requested_mode: SummaryMode,
    summary_model_dir: Option<&Path>,
) -> String {
    format!(
        "{source_key}\n{transcript_hash}\n{}\n{}",
        requested_mode.requested_key(),
        summary_model_dir
            .map(|path| path.display().to_string())
            .unwrap_or_default()
    )
}

fn parse_summary_backend(value: &str) -> Result<SummaryBackend> {
    match value {
        "apple-foundation" => Ok(SummaryBackend::AppleFoundation),
        "gemma4-e2b" => Ok(SummaryBackend::Gemma4E2b),
        "gemma4-e4b" => Ok(SummaryBackend::Gemma4E4b),
        _ => Err(eyre!("unknown summary backend {value}")),
    }
}

#[cfg(test)]
mod tests {
    use super::{SummaryCacheEntry, load_summary, store_summary};
    use crate::cache::{MANIFEST_FILE_NAME, cache_entry_dir};
    use crate::paths::AppPaths;
    use crate::summary::SummaryMode;
    use crate::summary_backend::SummaryBackend;
    use color_eyre::Result;
    use serde_json::Value;
    use std::fs;

    fn test_paths(name: &str) -> AppPaths {
        AppPaths {
            cache_dir: std::env::temp_dir().join(name),
        }
    }

    #[test]
    fn summary_cache_round_trip_preserves_manifest_shape() -> Result<()> {
        let app_paths = test_paths("smrze-cache-summary-round-trip");
        let cache_key = "source-key\nhash\nauto\n";
        store_summary(
            &app_paths,
            SummaryCacheEntry {
                cache_key,
                source_key: "source-key",
                display_name: "meeting",
                transcript_hash: "hash",
                requested_mode: SummaryMode::Auto,
                summary_model_dir: None,
                markdown: "# Summary",
                backend: SummaryBackend::AppleFoundation,
            },
        )?;

        let cached_summary =
            load_summary(&app_paths, cache_key)?.expect("summary cache should load");
        assert_eq!(cached_summary.markdown, "# Summary");
        assert_eq!(cached_summary.backend, SummaryBackend::AppleFoundation);

        let manifest_path =
            cache_entry_dir(&app_paths, &crate::cache::SUMMARY_CACHE_SPEC, cache_key)
                .join(MANIFEST_FILE_NAME);
        let manifest: Value = serde_json::from_str(&fs::read_to_string(&manifest_path)?)?;
        assert_eq!(manifest["source_key"], "source-key");
        assert_eq!(manifest["display_name"], "meeting");
        assert_eq!(manifest["transcript_hash"], "hash");
        assert_eq!(manifest["requested_mode"], "auto");
        assert_eq!(manifest["actual_backend"], "apple-foundation");
        assert_eq!(manifest["summary_file_name"], "summary.md");

        let _ = fs::remove_dir_all(&app_paths.cache_dir);
        Ok(())
    }
}
