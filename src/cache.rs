use color_eyre::{
    Result,
    eyre::{Context, eyre},
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::{debug, warn};

use crate::paths::AppPaths;
use crate::utils::{ensure_parent_dir, hash_string};

const MANIFEST_FILE_NAME: &str = "manifest.json";

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

    fn ttl(self) -> Duration {
        match self {
            Self::Audio => Duration::from_secs(24 * 60 * 60),
            Self::Transcript => Duration::from_secs(30 * 24 * 60 * 60),
            Self::Summary => Duration::from_secs(90 * 24 * 60 * 60),
        }
    }
}

pub fn cache_entry_dir(app_paths: &AppPaths, kind: CacheKind, key_material: &str) -> PathBuf {
    cache_root_dir(app_paths, kind).join(hash_string(key_material))
}

pub fn cache_root_dir(app_paths: &AppPaths, kind: CacheKind) -> PathBuf {
    app_paths.cache_dir.join("artifacts").join(kind.dir_name())
}

pub fn cache_file_path(
    app_paths: &AppPaths,
    kind: CacheKind,
    key_material: &str,
    file_name: &str,
) -> PathBuf {
    cache_entry_dir(app_paths, kind, key_material).join(file_name)
}

pub fn clear_cache_entry(app_paths: &AppPaths, kind: CacheKind, key_material: &str) -> Result<()> {
    remove_dir_if_exists(&cache_entry_dir(app_paths, kind, key_material))
}

pub fn ensure_cache_entry_dir(
    app_paths: &AppPaths,
    kind: CacheKind,
    key_material: &str,
) -> Result<PathBuf> {
    let entry_dir = cache_entry_dir(app_paths, kind, key_material);
    fs::create_dir_all(&entry_dir)
        .with_context(|| format!("failed to create {}", entry_dir.display()))?;
    Ok(entry_dir)
}

pub fn load_manifest<T: DeserializeOwned>(
    app_paths: &AppPaths,
    kind: CacheKind,
    key_material: &str,
) -> Result<Option<T>> {
    load_manifest_from_dir(&cache_entry_dir(app_paths, kind, key_material), kind)
}

pub fn load_manifest_from_dir<T: DeserializeOwned>(
    entry_dir: &Path,
    kind: CacheKind,
) -> Result<Option<T>> {
    let manifest_path = entry_dir.join(MANIFEST_FILE_NAME);
    if !manifest_path.exists() {
        return Ok(None);
    }

    let manifest_text = fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let manifest_value: Value = serde_json::from_str(&manifest_text)
        .with_context(|| format!("failed to parse {}", manifest_path.display()))?;
    let created_at_ms = manifest_value
        .get("created_at_ms")
        .and_then(Value::as_u64)
        .ok_or_else(|| eyre!("{} is missing created_at_ms", manifest_path.display()))?;

    if is_expired(created_at_ms, kind.ttl())? {
        remove_dir_if_exists(entry_dir)?;
        return Ok(None);
    }

    let manifest = serde_json::from_value(manifest_value)
        .with_context(|| format!("failed to decode {}", manifest_path.display()))?;
    Ok(Some(manifest))
}

pub fn write_manifest<T: Serialize>(path: &Path, manifest: &T) -> Result<()> {
    write_json_file(path, manifest)
}

pub fn write_json_file<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    ensure_parent_dir(path)?;
    let temp_path = temp_path(path)?;
    {
        let file = fs::File::create(&temp_path)
            .with_context(|| format!("failed to create {}", temp_path.display()))?;
        serde_json::to_writer_pretty(file, value)
            .with_context(|| format!("failed to write {}", temp_path.display()))?;
    }
    fs::rename(&temp_path, path)
        .with_context(|| format!("failed to replace {}", path.display()))?;
    Ok(())
}

pub fn write_text_file(path: &Path, content: &str) -> Result<()> {
    ensure_parent_dir(path)?;
    let temp_path = temp_path(path)?;
    fs::write(&temp_path, content)
        .with_context(|| format!("failed to write {}", temp_path.display()))?;
    fs::rename(&temp_path, path)
        .with_context(|| format!("failed to replace {}", path.display()))?;
    Ok(())
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

fn is_expired(created_at_ms: u64, ttl: Duration) -> Result<bool> {
    let now_ms = now_millis_u64()?;
    let ttl_ms = u64::try_from(ttl.as_millis()).map_err(|_| eyre!("ttl overflow"))?;
    Ok(now_ms.saturating_sub(created_at_ms) > ttl_ms)
}

fn temp_path(path: &Path) -> Result<PathBuf> {
    let stamp = now_millis_u64()?;
    Ok(path.with_extension(format!("tmp-{stamp}")))
}

fn now_millis_u64() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| eyre!("system clock before unix epoch: {error}"))?
        .as_millis()
        .try_into()
        .map_err(|_| eyre!("system time does not fit into u64"))
}

fn remove_dir_if_exists(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }

    fs::remove_dir_all(path).with_context(|| format!("failed to remove {}", path.display()))?;
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
