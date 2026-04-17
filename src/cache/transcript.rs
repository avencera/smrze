use color_eyre::{Result, eyre::Context};
use serde::{Deserialize, Serialize};
use std::fs;

use crate::paths::AppPaths;
use crate::speakers::SpeakerTurn;
use crate::transcript::TranscriptToken;
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
    pub tokens: Vec<TranscriptToken>,
}

#[derive(Debug, Clone)]
pub struct TranscriptCacheEntry<'a> {
    pub cache_key: &'a str,
    pub source_key: &'a str,
    pub display_name: &'a str,
    pub transcript: &'a str,
    pub turns: &'a [SpeakerTurn],
    pub tokens: &'a [TranscriptToken],
}

#[derive(Debug, Serialize, Deserialize)]
struct TranscriptManifest {
    created_at_ms: u64,
    source_key: String,
    display_name: String,
    transcript_hash: String,
    transcript_file_name: String,
    turns_file_name: String,
    #[serde(default)]
    tokens_file_name: String,
}

pub(crate) fn load_transcript(
    app_paths: &AppPaths,
    cache_key: &str,
) -> Result<Option<CachedTranscript>> {
    let Some(manifest) =
        load_manifest::<TranscriptManifest>(app_paths, &TRANSCRIPT_CACHE_SPEC, cache_key)?
    else {
        return Ok(None);
    };

    let transcript_path = cache_file_path(
        app_paths,
        &TRANSCRIPT_CACHE_SPEC,
        cache_key,
        &manifest.transcript_file_name,
    );
    let turns_path = cache_file_path(
        app_paths,
        &TRANSCRIPT_CACHE_SPEC,
        cache_key,
        &manifest.turns_file_name,
    );
    if manifest.tokens_file_name.is_empty() {
        return Ok(None);
    }
    let tokens_path = cache_file_path(
        app_paths,
        &TRANSCRIPT_CACHE_SPEC,
        cache_key,
        &manifest.tokens_file_name,
    );
    if !transcript_path.exists() || !turns_path.exists() || !tokens_path.exists() {
        return Ok(None);
    }

    let transcript = fs::read_to_string(&transcript_path)
        .with_context(|| format!("failed to read {}", transcript_path.display()))?;
    let turns = serde_json::from_reader(
        fs::File::open(&turns_path)
            .with_context(|| format!("failed to open {}", turns_path.display()))?,
    )
    .with_context(|| format!("failed to parse {}", turns_path.display()))?;
    let tokens = serde_json::from_reader(
        fs::File::open(&tokens_path)
            .with_context(|| format!("failed to open {}", tokens_path.display()))?,
    )
    .with_context(|| format!("failed to parse {}", tokens_path.display()))?;

    Ok(Some(CachedTranscript {
        display_name: manifest.display_name,
        source_key: manifest.source_key,
        transcript_hash: manifest.transcript_hash,
        transcript,
        turns,
        tokens,
    }))
}

pub fn load_cached_transcript(
    app_paths: &AppPaths,
    cache_key: &str,
    force: bool,
) -> Result<Option<CachedTranscript>> {
    load_cache_entry(
        app_paths,
        &TRANSCRIPT_CACHE_SPEC,
        cache_key,
        force,
        load_transcript,
    )
}

pub fn store_transcript(app_paths: &AppPaths, entry: TranscriptCacheEntry<'_>) -> Result<()> {
    let entry_dir = ensure_cache_entry_dir(app_paths, &TRANSCRIPT_CACHE_SPEC, entry.cache_key)?;
    let transcript_path = entry_dir.join("transcript.txt");
    let turns_path = entry_dir.join("turns.json");
    let tokens_path = entry_dir.join("tokens.json");
    write_text_file(&transcript_path, entry.transcript)?;
    write_json_file(&turns_path, &entry.turns)?;
    write_json_file(&tokens_path, &entry.tokens)?;
    write_manifest(
        &entry_dir.join(MANIFEST_FILE_NAME),
        &TranscriptManifest {
            created_at_ms: now_millis_u64()?,
            source_key: entry.source_key.to_owned(),
            display_name: entry.display_name.to_owned(),
            transcript_hash: hash_string(entry.transcript),
            transcript_file_name: "transcript.txt".to_owned(),
            turns_file_name: "turns.json".to_owned(),
            tokens_file_name: "tokens.json".to_owned(),
        },
    )?;
    Ok(())
}

pub fn transcript_cache_key(source_key: &str, transcription_mode: &str) -> String {
    format!("{source_key}\n{transcription_mode}")
}

#[cfg(test)]
mod tests {
    use super::{TranscriptCacheEntry, load_transcript, store_transcript, transcript_cache_key};
    use crate::cache::{MANIFEST_FILE_NAME, cache_entry_dir};
    use crate::paths::AppPaths;
    use crate::speakers::SpeakerTurn;
    use crate::transcript::TranscriptToken;
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
        let tokens = vec![TranscriptToken {
            text: " hello".to_owned(),
            start: 1.0,
            end: 1.2,
        }];
        store_transcript(
            &app_paths,
            TranscriptCacheEntry {
                cache_key: "source-key\nvad",
                source_key: "source-key",
                display_name: "meeting",
                transcript: "[00:00:01.000-00:00:02.000] Speaker 1: Hello",
                turns: &turns,
                tokens: &tokens,
            },
        )?;

        let cached_transcript =
            load_transcript(&app_paths, "source-key\nvad")?.expect("transcript cache should load");
        assert_eq!(cached_transcript.display_name, "meeting");
        assert_eq!(cached_transcript.source_key, "source-key");
        assert_eq!(cached_transcript.turns, turns);
        assert_eq!(cached_transcript.tokens, tokens);

        let manifest_path = cache_entry_dir(
            &app_paths,
            &crate::cache::TRANSCRIPT_CACHE_SPEC,
            "source-key\nvad",
        )
        .join(MANIFEST_FILE_NAME);
        let manifest: Value = serde_json::from_str(&fs::read_to_string(&manifest_path)?)?;
        assert_eq!(manifest["source_key"], "source-key");
        assert_eq!(manifest["display_name"], "meeting");
        assert_eq!(manifest["transcript_file_name"], "transcript.txt");
        assert_eq!(manifest["turns_file_name"], "turns.json");
        assert_eq!(manifest["tokens_file_name"], "tokens.json");
        assert!(manifest["transcript_hash"].as_str().is_some());

        let _ = fs::remove_dir_all(&app_paths.cache_dir);
        Ok(())
    }

    #[test]
    fn transcript_cache_key_varies_by_mode() {
        assert_ne!(
            transcript_cache_key("source-key", "fast"),
            transcript_cache_key("source-key", "vad")
        );
    }
}
