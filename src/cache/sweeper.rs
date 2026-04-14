use color_eyre::{Result, eyre::Context};
use serde_json::Value;
use std::fs;
use std::thread;
use tracing::{debug, warn};

use crate::paths::AppPaths;

use super::support::{CacheSpec, cache_root_dir, load_manifest_from_dir};
use super::{AUDIO_CACHE_SPEC, SUMMARY_CACHE_SPEC, TRANSCRIPT_CACHE_SPEC};

pub fn spawn_cache_sweeper(app_paths: AppPaths) {
    let _ = thread::Builder::new()
        .name("smrze-cache-sweeper".to_owned())
        .spawn(move || {
            if let Err(error) = sweep_expired_entries(&app_paths) {
                warn!("Background cache sweep failed: {error:#}");
            }
        });
}

pub(crate) fn sweep_expired_entries(app_paths: &AppPaths) -> Result<()> {
    for spec in [
        &AUDIO_CACHE_SPEC,
        &TRANSCRIPT_CACHE_SPEC,
        &SUMMARY_CACHE_SPEC,
    ] {
        sweep_cache_spec(app_paths, spec)?;
    }
    Ok(())
}

fn sweep_cache_spec(app_paths: &AppPaths, spec: &CacheSpec) -> Result<()> {
    let root_dir = cache_root_dir(app_paths, spec);
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

        match load_manifest_from_dir::<Value>(&entry_path, spec) {
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
    use super::sweep_expired_entries;
    use crate::cache::{
        MANIFEST_FILE_NAME, cache_entry_dir, ensure_audio_cache_entry_dir, write_manifest,
    };
    use crate::paths::AppPaths;
    use color_eyre::Result;
    use serde_json::json;
    use std::fs;

    fn test_paths(name: &str) -> AppPaths {
        AppPaths {
            cache_dir: std::env::temp_dir().join(name),
        }
    }

    #[test]
    fn sweeper_removes_expired_entries() -> Result<()> {
        let app_paths = test_paths("smrze-cache-sweeper-expired");
        let key = "source-key";
        let entry_dir = ensure_audio_cache_entry_dir(&app_paths, key)?;
        write_manifest(
            &entry_dir.join(MANIFEST_FILE_NAME),
            &json!({
                "created_at_ms": 1_u64,
            }),
        )?;

        sweep_expired_entries(&app_paths)?;

        assert!(!cache_entry_dir(&app_paths, &crate::cache::AUDIO_CACHE_SPEC, key).exists());
        let _ = fs::remove_dir_all(&app_paths.cache_dir);
        Ok(())
    }
}
