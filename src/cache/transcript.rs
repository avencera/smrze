use color_eyre::{Result, eyre::Context};
use serde::{Deserialize, Serialize};
use std::fs;

use crate::paths::AppPaths;
use crate::speakers::SpeakerTurn;
use crate::utils::{hash_string, now_millis_u64};

use super::access::load_cache_entry;
use super::support::{
    CacheSpec, MANIFEST_FILE_NAME, cache_file_path, ensure_cache_entry_dir, load_manifest,
    write_json_file, write_manifest, write_text_file,
};

pub(crate) const TRANSCRIPT_CACHE_SPEC: CacheSpec = CacheSpec::new(
    "transcripts",
    std::time::Duration::from_secs(30 * 24 * 60 * 60),
);

#[derive(Debug, Clone)]
pub struct CachedTranscript {
    pub display_name: String,
    pub source_key: String,
    pub transcript_hash: String,
    pub transcript: String,
    pub turns: Vec<SpeakerTurn>,
}

#[derive(Debug, Clone)]
pub struct TranscriptCacheEntry<'a> {
    pub source_key: &'a str,
    pub display_name: &'a str,
    pub transcript: &'a str,
    pub turns: &'a [SpeakerTurn],
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

pub(crate) fn load_transcript(
    app_paths: &AppPaths,
    source_key: &str,
) -> Result<Option<CachedTranscript>> {
    let Some(manifest) =
        load_manifest::<TranscriptManifest>(app_paths, &TRANSCRIPT_CACHE_SPEC, source_key)?
    else {
        return Ok(None);
    };

    let transcript_path = cache_file_path(
        app_paths,
        &TRANSCRIPT_CACHE_SPEC,
        source_key,
        &manifest.transcript_file_name,
    );
    let turns_path = cache_file_path(
        app_paths,
        &TRANSCRIPT_CACHE_SPEC,
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

pub fn load_cached_transcript(
    app_paths: &AppPaths,
    source_key: &str,
    force: bool,
) -> Result<Option<CachedTranscript>> {
    load_cache_entry(
        app_paths,
        &TRANSCRIPT_CACHE_SPEC,
        source_key,
        force,
        load_transcript,
    )
}

pub fn store_transcript(app_paths: &AppPaths, entry: TranscriptCacheEntry<'_>) -> Result<()> {
    let entry_dir = ensure_cache_entry_dir(app_paths, &TRANSCRIPT_CACHE_SPEC, entry.source_key)?;
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

#[cfg(test)]
mod tests {
    use super::{TranscriptCacheEntry, load_transcript, store_transcript};
    use crate::cache::{MANIFEST_FILE_NAME, cache_entry_dir};
    use crate::paths::AppPaths;
    use crate::speakers::SpeakerTurn;
    use color_eyre::Result;
    use serde_json::Value;
    use std::fs;

    fn test_paths(name: &str) -> AppPaths {
        AppPaths {
            cache_dir: std::env::temp_dir().join(name),
        }
    }

    #[test]
    fn transcript_cache_round_trip_preserves_manifest_shape() -> Result<()> {
        let app_paths = test_paths("smrze-cache-transcript-round-trip");
        let turns = vec![SpeakerTurn {
            start: 1.0,
            end: 2.0,
            speaker: "Speaker 1".to_owned(),
            text: "Hello".to_owned(),
        }];
        store_transcript(
            &app_paths,
            TranscriptCacheEntry {
                source_key: "source-key",
                display_name: "meeting",
                transcript: "[00:00:01.000-00:00:02.000] Speaker 1: Hello",
                turns: &turns,
            },
        )?;

        let cached_transcript =
            load_transcript(&app_paths, "source-key")?.expect("transcript cache should load");
        assert_eq!(cached_transcript.display_name, "meeting");
        assert_eq!(cached_transcript.source_key, "source-key");
        assert_eq!(cached_transcript.turns, turns);

        let manifest_path = cache_entry_dir(
            &app_paths,
            &crate::cache::TRANSCRIPT_CACHE_SPEC,
            "source-key",
        )
        .join(MANIFEST_FILE_NAME);
        let manifest: Value = serde_json::from_str(&fs::read_to_string(&manifest_path)?)?;
        assert_eq!(manifest["source_key"], "source-key");
        assert_eq!(manifest["display_name"], "meeting");
        assert_eq!(manifest["transcript_file_name"], "transcript.txt");
        assert_eq!(manifest["turns_file_name"], "turns.json");
        assert!(manifest["transcript_hash"].as_str().is_some());

        let _ = fs::remove_dir_all(&app_paths.cache_dir);
        Ok(())
    }
}
