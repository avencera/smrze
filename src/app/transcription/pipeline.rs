use color_eyre::Result;
use std::sync::Arc;
use tracing::{debug, warn};

use super::audio::{AudioMaterializer, load_normalized_audio};
use crate::cache::{
    CacheKind, CachedTranscript, TranscriptCacheEntry, load_cache_entry, load_transcript,
    store_transcript,
};
use crate::input::ResolvedMediaInput;
use crate::paths::AppPaths;
use crate::speakers::{SpeakerTurn, build_turns};
use crate::transcript::render_transcript;
use crate::utils::hash_string;
use crate::workers::{DiarizationWorker, TranscriptionWorker};

pub(crate) struct TranscriptionPipeline<'a> {
    app_paths: &'a AppPaths,
    force: bool,
}

impl<'a> TranscriptionPipeline<'a> {
    pub(crate) fn new(app_paths: &'a AppPaths, force: bool) -> Self {
        Self { app_paths, force }
    }

    pub(crate) fn transcribe_resolved_input(
        &self,
        resolved_input: &ResolvedMediaInput,
    ) -> Result<CachedTranscript> {
        if let Some(cached_transcript) = load_cache_entry(
            self.app_paths,
            CacheKind::Transcript,
            &resolved_input.source_key,
            self.force,
            load_transcript,
        )? {
            return Ok(cached_transcript);
        }

        let cached_audio =
            AudioMaterializer::new(self.app_paths, self.force).materialize(resolved_input)?;
        let normalized_audio = load_normalized_audio(&cached_audio.audio_path)?;
        let (transcript, turns) = build_transcript_from_audio(self.app_paths, normalized_audio)?;
        store_transcript(
            self.app_paths,
            TranscriptCacheEntry {
                source_key: &resolved_input.source_key,
                display_name: &resolved_input.display_name,
                transcript: &transcript,
                turns: &turns,
            },
        )?;

        Ok(CachedTranscript {
            display_name: cached_audio.display_name,
            source_key: resolved_input.source_key.clone(),
            transcript_hash: hash_string(&transcript),
            transcript,
            turns,
        })
    }
}

fn build_transcript_from_audio(
    app_paths: &AppPaths,
    normalized_audio: Arc<[f32]>,
) -> Result<(String, Vec<SpeakerTurn>)> {
    let diarization_worker = DiarizationWorker::spawn(app_paths.speakrs_model_cache());
    let transcription_worker = TranscriptionWorker::spawn(app_paths.scriptrs_model_cache());
    execute_transcription_pipeline(normalized_audio, diarization_worker, transcription_worker)
}

fn execute_transcription_pipeline(
    normalized_audio: Arc<[f32]>,
    diarization_worker: DiarizationWorker,
    transcription_worker: TranscriptionWorker,
) -> Result<(String, Vec<SpeakerTurn>)> {
    let diarization = match diarization_worker.run(Arc::clone(&normalized_audio)) {
        Ok(diarization) => diarization,
        Err(error) => {
            if let Err(cancel_error) = transcription_worker.cancel() {
                warn!(
                    "Failed to stop transcription worker after diarization error: {cancel_error:#}"
                );
            }
            return Err(error);
        }
    };
    debug!(
        "diarization produced {} segments",
        diarization.segments.len()
    );

    let transcription = transcription_worker.run(normalized_audio)?;
    debug!(
        "transcription produced {} timed tokens",
        transcription.tokens.len()
    );

    let turns = build_turns(&transcription.tokens, &diarization);
    Ok((render_transcript(&turns), turns))
}
