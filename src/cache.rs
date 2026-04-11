mod audio;
mod summary;
mod support;
mod transcript;

pub use audio::{AudioCacheEntry, CachedAudio, load_audio, store_audio};
pub use summary::{SummaryCacheEntry, load_summary, store_summary, summary_cache_key};
pub use transcript::{CachedTranscript, TranscriptCacheEntry, load_transcript, store_transcript};

pub(crate) use support::{
    MANIFEST_FILE_NAME, cache_entry_dir, cache_file_path, ensure_cache_entry_dir, load_manifest,
    load_manifest_from_dir, write_json_file, write_manifest, write_text_file,
};

use color_eyre::{Result, eyre::Context};
use serde_json::Value;
use std::fs;
use std::thread;
use tracing::{debug, warn};

use crate::paths::AppPaths;
use support::{cache_root_dir, remove_dir_if_exists};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheKind {
    Audio,
    Transcript,
    Summary,
}

impl CacheKind {
    fn dir_name(self) -> &'static str {
        match self {
            Self::Audio => "audio",
            Self::Transcript => "transcripts",
            Self::Summary => "summaries",
        }
    }

    fn ttl(self) -> std::time::Duration {
        match self {
            Self::Audio => std::time::Duration::from_secs(24 * 60 * 60),
            Self::Transcript => std::time::Duration::from_secs(30 * 24 * 60 * 60),
            Self::Summary => std::time::Duration::from_secs(90 * 24 * 60 * 60),
        }
    }
}

pub fn clear_cache_entry(app_paths: &AppPaths, kind: CacheKind, key_material: &str) -> Result<()> {
    remove_dir_if_exists(&cache_entry_dir(app_paths, kind, key_material))
}

pub fn spawn_cache_sweeper(app_paths: AppPaths) {
    let _ = thread::Builder::new()
        .name("smrze-cache-sweeper".to_owned())
        .spawn(move || {
            if let Err(error) = sweep_expired_entries(&app_paths) {
                warn!("Background cache sweep failed: {error:#}");
            }
        });
}

pub fn sweep_expired_entries(app_paths: &AppPaths) -> Result<()> {
    for kind in [CacheKind::Audio, CacheKind::Transcript, CacheKind::Summary] {
        sweep_cache_kind(app_paths, kind)?;
    }
    Ok(())
}

fn sweep_cache_kind(app_paths: &AppPaths, kind: CacheKind) -> Result<()> {
    let root_dir = cache_root_dir(app_paths, kind);
    if !root_dir.exists() {
        return Ok(());
    }

    for entry in
        fs::read_dir(&root_dir).with_context(|| format!("failed to read {}", root_dir.display()))?
    {
        let entry = entry.with_context(|| format!("failed to read {}", root_dir.display()))?;
        let entry_path = entry.path();
        if !entry_path.is_dir() {
            continue;
        }

        match load_manifest_from_dir::<Value>(&entry_path, kind) {
            Ok(Some(_)) | Ok(None) => {}
            Err(error) => {
                debug!(
                    "Skipping cache sweep for {} after manifest error: {error:#}",
                    entry_path.display()
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        CacheKind, cache_entry_dir, clear_cache_entry, ensure_cache_entry_dir,
        sweep_expired_entries, write_manifest,
    };
    use crate::paths::AppPaths;
    use color_eyre::Result;
    use serde_json::json;
    use std::fs;
    use std::path::PathBuf;

    fn test_paths(name: &str) -> AppPaths {
        AppPaths {
            cache_dir: std::env::temp_dir().join(name),
        }
    }

    #[test]
    fn sweeper_removes_expired_entries() -> Result<()> {
        let app_paths = test_paths("smrze-cache-sweeper-expired");
        let key = "source-key";
        let entry_dir = ensure_cache_entry_dir(&app_paths, CacheKind::Audio, key)?;
        write_manifest(
            &entry_dir.join("manifest.json"),
            &json!({
                "created_at_ms": 1_u64,
            }),
        )?;

        sweep_expired_entries(&app_paths)?;

        assert!(!cache_entry_dir(&app_paths, CacheKind::Audio, key).exists());
        let _ = fs::remove_dir_all(&app_paths.cache_dir);
        Ok(())
    }

    #[test]
    fn clear_cache_entry_ignores_missing_dirs() -> Result<()> {
        let app_paths = test_paths("smrze-cache-sweeper-clear");
        clear_cache_entry(&app_paths, CacheKind::Summary, "missing")?;
        let _ = fs::remove_dir_all(PathBuf::from(&app_paths.cache_dir));
        Ok(())
    }
}
