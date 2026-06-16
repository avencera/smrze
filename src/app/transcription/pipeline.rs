use color_eyre::Result;
use std::sync::Arc;
use tracing::{debug, warn};

use super::audio::{AudioMaterializer, load_normalized_audio};
use crate::cache::{
    CachedPlainTranscript, CachedTranscript, PlainTranscriptCacheEntry, TranscriptCacheEntry,
    load_cached_plain_transcript, load_cached_transcript, plain_transcript_cache_key,
    store_plain_transcript, store_transcript, transcript_cache_key,
};
use crate::cli::TranscriptionMode;
use crate::input::ResolvedMediaInput;
use crate::paths::AppPaths;
use crate::speakers::{SpeakerTurn, build_turns};
use crate::transcript::{TranscriptToken, render_plain_transcript, render_transcript};
use crate::utils::hash_string;
use crate::workers::{DiarizationWorker, TranscriptionWorker};

pub(crate) struct TranscriptionPipeline<'a> {
    app_paths: &'a AppPaths,
    force: bool,
    transcription_mode: TranscriptionMode,
}

impl<'a> TranscriptionPipeline<'a> {
    pub(crate) fn new(
        app_paths: &'a AppPaths,
        force: bool,
        transcription_mode: TranscriptionMode,
    ) -> Self {
        Self {
            app_paths,
            force,
            transcription_mode,
        }
    }

    pub(crate) fn transcribe_resolved_input(
        &self,
        resolved_input: &ResolvedMediaInput,
    ) -> Result<CachedTranscript> {
        let cache_key = transcript_cache_key(
            &resolved_input.source_key,
            self.transcription_mode.cache_key(),
        );
        debug!(
            "Checking transcript cache for source key {}",
            resolved_input.source_key
        );
        if let Some(cached_transcript) =
            load_cached_transcript(self.app_paths, &cache_key, self.force)?
        {
            debug!(
                "Transcript cache hit for source key {}",
                resolved_input.source_key
            );
            return Ok(cached_transcript);
        }
        debug!(
            "Transcript cache miss for source key {}",
            resolved_input.source_key
        );

        let cached_audio =
            AudioMaterializer::new(self.app_paths, self.force).materialize(resolved_input)?;
        let normalized_audio = load_normalized_audio(&cached_audio.audio_path)?;
        let (transcript, turns, tokens) =
            build_transcript_from_audio(self.app_paths, normalized_audio, self.transcription_mode)?;
        store_transcript(
            self.app_paths,
            TranscriptCacheEntry {
                cache_key: &cache_key,
                source_key: &resolved_input.source_key,
                display_name: &cached_audio.display_name,
                transcript: &transcript,
                turns: &turns,
                tokens: &tokens,
            },
        )?;

        Ok(CachedTranscript {
            display_name: cached_audio.display_name,
            source_key: resolved_input.source_key.clone(),
            transcript_hash: hash_string(&transcript),
            transcript,
            turns,
            tokens,
        })
    }

    pub(crate) fn transcribe_plain_resolved_input(
        &self,
        resolved_input: &ResolvedMediaInput,
    ) -> Result<CachedPlainTranscript> {
        let cache_key = plain_transcript_cache_key(
            &resolved_input.source_key,
            self.transcription_mode.cache_key(),
        );
        debug!(
            "Checking plain transcript cache for source key {}",
            resolved_input.source_key
        );
        if let Some(cached_transcript) =
            load_cached_plain_transcript(self.app_paths, &cache_key, self.force)?
        {
            debug!(
                "Plain transcript cache hit for source key {}",
                resolved_input.source_key
            );
            return Ok(cached_transcript);
        }
        debug!(
            "Plain transcript cache miss for source key {}",
            resolved_input.source_key
        );

        if !self.force
            && let Some(cached_transcript) = self.load_structured_transcript(resolved_input)?
        {
            let transcript = render_plain_transcript(&cached_transcript.tokens);
            store_plain_transcript(
                self.app_paths,
                PlainTranscriptCacheEntry {
                    cache_key: &cache_key,
                    source_key: &cached_transcript.source_key,
                    display_name: &cached_transcript.display_name,
                    transcript: &transcript,
                },
            )?;
            return Ok(CachedPlainTranscript { transcript });
        }

        let cached_audio =
            AudioMaterializer::new(self.app_paths, self.force).materialize(resolved_input)?;
        let normalized_audio = load_normalized_audio(&cached_audio.audio_path)?;
        let transcript = build_plain_transcript_from_audio(
            self.app_paths,
            normalized_audio,
            self.transcription_mode,
        )?;
        store_plain_transcript(
            self.app_paths,
            PlainTranscriptCacheEntry {
                cache_key: &cache_key,
                source_key: &resolved_input.source_key,
                display_name: &cached_audio.display_name,
                transcript: &transcript,
            },
        )?;

        Ok(CachedPlainTranscript { transcript })
    }

    fn load_structured_transcript(
        &self,
        resolved_input: &ResolvedMediaInput,
    ) -> Result<Option<CachedTranscript>> {
        let cache_key = transcript_cache_key(
            &resolved_input.source_key,
            self.transcription_mode.cache_key(),
        );
        load_cached_transcript(self.app_paths, &cache_key, false)
    }
}

fn build_transcript_from_audio(
    app_paths: &AppPaths,
    normalized_audio: Arc<[f32]>,
    transcription_mode: TranscriptionMode,
) -> Result<(String, Vec<SpeakerTurn>, Vec<TranscriptToken>)> {
    let diarization_worker = DiarizationWorker::spawn(app_paths.speakrs_model_cache());
    let transcription_worker =
        TranscriptionWorker::spawn(app_paths.scriptrs_model_cache(), transcription_mode);
    execute_transcription_pipeline(normalized_audio, diarization_worker, transcription_worker)
}

fn build_plain_transcript_from_audio(
    app_paths: &AppPaths,
    normalized_audio: Arc<[f32]>,
    transcription_mode: TranscriptionMode,
) -> Result<String> {
    let transcription_worker =
        TranscriptionWorker::spawn(app_paths.scriptrs_model_cache(), transcription_mode);
    let transcription_worker = transcription_worker.start(normalized_audio)?;
    let transcription = transcription_worker.wait()?;
    debug!(
        "transcription produced {} timed tokens",
        transcription.tokens.len()
    );
    Ok(transcription.text)
}

fn execute_transcription_pipeline(
    normalized_audio: Arc<[f32]>,
    diarization_worker: DiarizationWorker,
    transcription_worker: TranscriptionWorker,
) -> Result<(String, Vec<SpeakerTurn>, Vec<TranscriptToken>)> {
    let diarization_worker = diarization_worker.start(Arc::clone(&normalized_audio))?;
    let transcription_worker = match transcription_worker.start(normalized_audio) {
        Ok(worker) => worker,
        Err(error) => {
            if let Err(wait_error) = diarization_worker.wait() {
                warn!(
                    "Failed to clean up diarization worker after transcription start error: {wait_error:#}"
                );
            }
            return Err(error);
        }
    };

    let diarization = diarization_worker.wait();
    let transcription = transcription_worker.wait();
    let (diarization, transcription) = match (diarization, transcription) {
        (Ok(diarization), Ok(transcription)) => (diarization, transcription),
        (Err(error), Ok(_)) => return Err(error),
        (Ok(_), Err(error)) => return Err(error),
        (Err(error), Err(transcription_error)) => {
            warn!("Transcription also failed: {transcription_error:#}");
            return Err(error);
        }
    };

    debug!(
        "diarization produced {} segments",
        diarization.segments.len()
    );
    debug!(
        "transcription produced {} timed tokens",
        transcription.tokens.len()
    );

    let turns = build_turns(&transcription.tokens, &diarization);
    let tokens = transcription
        .tokens
        .iter()
        .map(TranscriptToken::from)
        .collect();
    Ok((render_transcript(&turns), turns, tokens))
}
