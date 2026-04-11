use color_eyre::{
    Result,
    eyre::{Context, eyre},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;
use tracing::{debug, warn};

use crate::paths::AppPaths;
use crate::speakers::SpeakerTurn;
use crate::summary::SummaryMode;
use crate::summary_backend::SummaryBackend;
use crate::utils::{ensure_parent_dir, hash_string, now_millis_u64};

const MANIFEST_FILE_NAME: &str = "manifest.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheKind {
    Audio,
    Transcript,
    Summary,
}

#[derive(Debug, Clone)]
pub struct CachedAudio {
    pub display_name: String,
    pub audio_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct CachedTranscript {
    pub display_name: String,
    pub source_key: String,
    pub transcript_hash: String,
    pub transcript: String,
    pub turns: Vec<SpeakerTurn>,
}

#[derive(Debug, Clone)]
pub struct CachedSummary {
    pub markdown: String,
    pub backend: SummaryBackend,
}

#[derive(Debug, Clone)]
pub struct AudioCacheEntry<'a> {
    pub source_key: &'a str,
    pub display_name: &'a str,
    pub audio_file_name: &'a str,
    pub media_file_name: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct TranscriptCacheEntry<'a> {
    pub source_key: &'a str,
    pub display_name: &'a str,
    pub transcript: &'a str,
    pub turns: &'a [SpeakerTurn],
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
struct AudioManifest {
    created_at_ms: u64,
    source_key: String,
    display_name: String,
    audio_file_name: String,
    #[serde(default)]
    media_file_name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct TranscriptManifest {
    created_at_ms: u64,
    source_key: String,
    display_name: String,
    transcript_hash: String,
    transcript_file_name: String,
    turns_file_name: String,
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

pub fn load_audio(app_paths: &AppPaths, source_key: &str) -> Result<Option<CachedAudio>> {
    let Some(manifest) = load_manifest::<AudioManifest>(app_paths, CacheKind::Audio, source_key)?
    else {
        return Ok(None);
    };
    let audio_path = cache_file_path(
        app_paths,
        CacheKind::Audio,
        source_key,
        &manifest.audio_file_name,
    );
    if !audio_path.exists() {
        return Ok(None);
    }

    Ok(Some(CachedAudio {
        display_name: manifest.display_name,
        audio_path,
    }))
}

pub fn store_audio(app_paths: &AppPaths, entry: AudioCacheEntry<'_>) -> Result<PathBuf> {
    let entry_dir = ensure_cache_entry_dir(app_paths, CacheKind::Audio, entry.source_key)?;
    let manifest_path = entry_dir.join(MANIFEST_FILE_NAME);
    write_manifest(
        &manifest_path,
        &AudioManifest {
            created_at_ms: now_millis_u64()?,
            source_key: entry.source_key.to_owned(),
            display_name: entry.display_name.to_owned(),
            audio_file_name: entry.audio_file_name.to_owned(),
            media_file_name: entry.media_file_name.map(ToOwned::to_owned),
        },
    )?;
    Ok(entry_dir.join(entry.audio_file_name))
}

pub fn load_transcript(app_paths: &AppPaths, source_key: &str) -> Result<Option<CachedTranscript>> {
    let Some(manifest) =
        load_manifest::<TranscriptManifest>(app_paths, CacheKind::Transcript, source_key)?
    else {
        return Ok(None);
    };

    let transcript_path = cache_file_path(
        app_paths,
        CacheKind::Transcript,
        source_key,
        &manifest.transcript_file_name,
    );
    let turns_path = cache_file_path(
        app_paths,
        CacheKind::Transcript,
        source_key,
        &manifest.turns_file_name,
    );
    if !transcript_path.exists() || !turns_path.exists() {
        return Ok(None);
    }

    let transcript = fs::read_to_string(&transcript_path)
        .with_context(|| format!("failed to read {}", transcript_path.display()))?;
    let turns = serde_json::from_reader(
        fs::File::open(&turns_path)
            .with_context(|| format!("failed to open {}", turns_path.display()))?,
    )
    .with_context(|| format!("failed to parse {}", turns_path.display()))?;

    Ok(Some(CachedTranscript {
        display_name: manifest.display_name,
        source_key: manifest.source_key,
        transcript_hash: manifest.transcript_hash,
        transcript,
        turns,
    }))
}

pub fn store_transcript(app_paths: &AppPaths, entry: TranscriptCacheEntry<'_>) -> Result<()> {
    let entry_dir = ensure_cache_entry_dir(app_paths, CacheKind::Transcript, entry.source_key)?;
    let transcript_path = entry_dir.join("transcript.txt");
    let turns_path = entry_dir.join("turns.json");
    write_text_file(&transcript_path, entry.transcript)?;
    write_json_file(&turns_path, &entry.turns)?;
    write_manifest(
        &entry_dir.join(MANIFEST_FILE_NAME),
        &TranscriptManifest {
            created_at_ms: now_millis_u64()?,
            source_key: entry.source_key.to_owned(),
            display_name: entry.display_name.to_owned(),
            transcript_hash: hash_string(entry.transcript),
            transcript_file_name: "transcript.txt".to_owned(),
            turns_file_name: "turns.json".to_owned(),
        },
    )?;
    Ok(())
}

pub fn load_summary(app_paths: &AppPaths, cache_key: &str) -> Result<Option<CachedSummary>> {
    let Some(manifest) =
        load_manifest::<SummaryManifest>(app_paths, CacheKind::Summary, cache_key)?
    else {
        return Ok(None);
    };
    let summary_path = cache_file_path(
        app_paths,
        CacheKind::Summary,
        cache_key,
        &manifest.summary_file_name,
    );
    if !summary_path.exists() {
        return Ok(None);
    }

    let markdown = fs::read_to_string(&summary_path)
        .with_context(|| format!("failed to read {}", summary_path.display()))?;
    Ok(Some(CachedSummary {
        markdown,
        backend: parse_summary_backend(&manifest.actual_backend)?,
    }))
}

pub fn store_summary(app_paths: &AppPaths, entry: SummaryCacheEntry<'_>) -> Result<()> {
    let entry_dir = ensure_cache_entry_dir(app_paths, CacheKind::Summary, entry.cache_key)?;
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

fn remove_dir_if_exists(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }

    fs::remove_dir_all(path).with_context(|| format!("failed to remove {}", path.display()))?;
    Ok(())
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
