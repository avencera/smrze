use color_eyre::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::paths::AppPaths;
use crate::utils::now_millis_u64;

use super::access::load_cache_entry;
use super::support::{
    CacheSpec, MANIFEST_FILE_NAME, cache_file_path, ensure_cache_entry_dir, load_manifest,
    write_manifest,
};

pub(crate) const AUDIO_CACHE_SPEC: CacheSpec =
    CacheSpec::new("audio", std::time::Duration::from_secs(24 * 60 * 60));

#[derive(Debug, Clone)]
pub struct CachedAudio {
    pub display_name: String,
    pub audio_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct AudioCacheEntry<'a> {
    pub source_key: &'a str,
    pub display_name: &'a str,
    pub audio_file_name: &'a str,
    pub media_file_name: Option<&'a str>,
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

pub(crate) fn load_audio(app_paths: &AppPaths, source_key: &str) -> Result<Option<CachedAudio>> {
    let Some(manifest) = load_manifest::<AudioManifest>(app_paths, &AUDIO_CACHE_SPEC, source_key)?
    else {
        return Ok(None);
    };
    let audio_path = cache_file_path(
        app_paths,
        &AUDIO_CACHE_SPEC,
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

pub fn load_cached_audio(
    app_paths: &AppPaths,
    source_key: &str,
    force: bool,
) -> Result<Option<CachedAudio>> {
    load_cache_entry(app_paths, &AUDIO_CACHE_SPEC, source_key, force, load_audio)
}

pub fn store_audio(app_paths: &AppPaths, entry: AudioCacheEntry<'_>) -> Result<PathBuf> {
    let entry_dir = ensure_audio_cache_entry_dir(app_paths, entry.source_key)?;
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

#[cfg(test)]
pub(crate) fn clear_audio_cache_entry(app_paths: &AppPaths, source_key: &str) -> Result<()> {
    super::access::clear_cache_entry(app_paths, &AUDIO_CACHE_SPEC, source_key)
}

pub(crate) fn ensure_audio_cache_entry_dir(
    app_paths: &AppPaths,
    source_key: &str,
) -> Result<PathBuf> {
    ensure_cache_entry_dir(app_paths, &AUDIO_CACHE_SPEC, source_key)
}

#[cfg(test)]
mod tests {
    use super::{AudioCacheEntry, load_audio, store_audio};
    use crate::cache::MANIFEST_FILE_NAME;
    use crate::paths::AppPaths;
    use color_eyre::Result;
    use serde_json::Value;
    use std::fs;

    fn test_paths(name: &str) -> AppPaths {
        AppPaths {
            cache_dir: std::env::temp_dir().join(name),
        }
    }

    #[test]
    fn audio_cache_round_trip_preserves_manifest_shape() -> Result<()> {
        let app_paths = test_paths("smrze-cache-audio-round-trip");
        let audio_path = store_audio(
            &app_paths,
            AudioCacheEntry {
                source_key: "source-key",
                display_name: "clip",
                audio_file_name: "audio.wav",
                media_file_name: Some("download.mp3"),
            },
        )?;
        fs::write(&audio_path, "audio")?;

        let cached_audio = load_audio(&app_paths, "source-key")?.expect("audio cache should load");
        assert_eq!(cached_audio.display_name, "clip");
        assert_eq!(cached_audio.audio_path, audio_path);

        let manifest_path = audio_path
            .parent()
            .expect("audio path should have a parent")
            .join(MANIFEST_FILE_NAME);
        let manifest: Value = serde_json::from_str(&fs::read_to_string(&manifest_path)?)?;
        assert_eq!(manifest["source_key"], "source-key");
        assert_eq!(manifest["display_name"], "clip");
        assert_eq!(manifest["audio_file_name"], "audio.wav");
        assert_eq!(manifest["media_file_name"], "download.mp3");

        let _ = fs::remove_dir_all(&app_paths.cache_dir);
        Ok(())
    }
}
