use color_eyre::{
    Result,
    eyre::{Context, eyre},
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::paths::AppPaths;
use crate::utils::{ensure_parent_dir, hash_string, now_millis_u64};

pub(crate) const MANIFEST_FILE_NAME: &str = "manifest.json";

#[derive(Debug, Clone, Copy)]
pub(crate) struct CacheSpec {
    dir_name: &'static str,
    ttl: Duration,
}

impl CacheSpec {
    pub(crate) const fn new(dir_name: &'static str, ttl: Duration) -> Self {
        Self { dir_name, ttl }
    }

    fn dir_name(self) -> &'static str {
        self.dir_name
    }

    fn ttl(self) -> Duration {
        self.ttl
    }
}

pub(crate) fn cache_entry_dir(
    app_paths: &AppPaths,
    spec: &CacheSpec,
    key_material: &str,
) -> PathBuf {
    cache_root_dir(app_paths, spec).join(hash_string(key_material))
}

pub(crate) fn cache_root_dir(app_paths: &AppPaths, spec: &CacheSpec) -> PathBuf {
    app_paths.cache_dir.join("artifacts").join(spec.dir_name())
}

pub(crate) fn cache_file_path(
    app_paths: &AppPaths,
    spec: &CacheSpec,
    key_material: &str,
    file_name: &str,
) -> PathBuf {
    cache_entry_dir(app_paths, spec, key_material).join(file_name)
}

pub(crate) fn ensure_cache_entry_dir(
    app_paths: &AppPaths,
    spec: &CacheSpec,
    key_material: &str,
) -> Result<PathBuf> {
    let entry_dir = cache_entry_dir(app_paths, spec, key_material);
    fs::create_dir_all(&entry_dir)
        .with_context(|| format!("failed to create {}", entry_dir.display()))?;
    Ok(entry_dir)
}

pub(crate) fn load_manifest<T: DeserializeOwned>(
    app_paths: &AppPaths,
    spec: &CacheSpec,
    key_material: &str,
) -> Result<Option<T>> {
    load_manifest_from_dir(&cache_entry_dir(app_paths, spec, key_material), spec)
}

pub(crate) fn load_manifest_from_dir<T: DeserializeOwned>(
    entry_dir: &Path,
    spec: &CacheSpec,
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

    if is_expired(created_at_ms, spec.ttl())? {
        remove_dir_if_exists(entry_dir)?;
        return Ok(None);
    }

    let manifest = serde_json::from_value(manifest_value)
        .with_context(|| format!("failed to decode {}", manifest_path.display()))?;
    Ok(Some(manifest))
}

pub(crate) fn write_manifest<T: Serialize>(path: &Path, manifest: &T) -> Result<()> {
    write_json_file(path, manifest)
}

pub(crate) fn write_json_file<T: Serialize>(path: &Path, value: &T) -> Result<()> {
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

pub(crate) fn write_text_file(path: &Path, content: &str) -> Result<()> {
    ensure_parent_dir(path)?;
    let temp_path = temp_path(path)?;
    fs::write(&temp_path, content)
        .with_context(|| format!("failed to write {}", temp_path.display()))?;
    fs::rename(&temp_path, path)
        .with_context(|| format!("failed to replace {}", path.display()))?;
    Ok(())
}

pub(crate) fn remove_dir_if_exists(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }

    fs::remove_dir_all(path).with_context(|| format!("failed to remove {}", path.display()))?;
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
