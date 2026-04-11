use color_eyre::{Result, eyre::eyre};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, warn};

use crate::audio::{decode_audio, normalize_audio};
use crate::cache::{
    AudioCacheEntry, CacheKind, CachedAudio, CachedTranscript, TranscriptCacheEntry,
    clear_cache_entry, ensure_cache_entry_dir, load_audio, load_transcript, store_audio,
    store_transcript,
};
use crate::cli::TranscriptArgs;
use crate::console;
use crate::input::{
    MediaInputKind, ResolvedMediaInput, convert_to_cached_audio, ensure_command,
    find_downloaded_media, resolve_media_input,
};
use crate::output::{commit_transcript, open_path, stage_transcript};
use crate::paths::{AppPaths, RunPaths};
use crate::speakers::{SpeakerTurn, build_turns};
use crate::transcript::render_transcript;
use crate::utils::hash_string;
use crate::workers::{DiarizationWorker, TranscriptionWorker};

pub(super) fn run_transcript(
    app_paths: &AppPaths,
    force: bool,
    args: &TranscriptArgs,
    run_paths: Option<&RunPaths>,
) -> Result<()> {
    let resolved_input = resolve_media_input(&args.input)?;
    let pipeline = TranscriptionPipeline::new(app_paths, force);
    let transcript = pipeline.transcribe_resolved_input(&resolved_input)?;

    if let Some(run_paths) = run_paths {
        let staged_path = stage_transcript(&run_paths.scratch_dir, &transcript.transcript)?;
        commit_transcript(&staged_path, &run_paths.final_path)?;
        println!("{}", run_paths.final_path.display());
        if args.open {
            open_path(&run_paths.final_path)?;
        }
    } else {
        println!("{}", transcript.transcript);
    }
    Ok(())
}

pub(super) struct TranscriptionPipeline<'a> {
    app_paths: &'a AppPaths,
    force: bool,
}

impl<'a> TranscriptionPipeline<'a> {
    pub(super) fn new(app_paths: &'a AppPaths, force: bool) -> Self {
        Self { app_paths, force }
    }

    pub(super) fn transcribe_resolved_input(
        &self,
        resolved_input: &ResolvedMediaInput,
    ) -> Result<CachedTranscript> {
        if self.force {
            clear_cache_entry(
                self.app_paths,
                CacheKind::Transcript,
                &resolved_input.source_key,
            )?;
        } else if let Some(cached_transcript) =
            load_transcript(self.app_paths, &resolved_input.source_key)?
        {
            return Ok(cached_transcript);
        }
        clear_cache_entry(
            self.app_paths,
            CacheKind::Transcript,
            &resolved_input.source_key,
        )?;

        let cached_audio = self.materialize_audio(resolved_input)?;
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

    fn materialize_audio(&self, resolved_input: &ResolvedMediaInput) -> Result<CachedAudio> {
        if self.force {
            clear_cache_entry(self.app_paths, CacheKind::Audio, &resolved_input.source_key)?;
        } else if let Some(cached_audio) = load_audio(self.app_paths, &resolved_input.source_key)? {
            return Ok(cached_audio);
        }
        clear_cache_entry(self.app_paths, CacheKind::Audio, &resolved_input.source_key)?;

        let entry_dir =
            ensure_cache_entry_dir(self.app_paths, CacheKind::Audio, &resolved_input.source_key)?;
        let audio_path = entry_dir.join("audio.wav");
        let mut media_file_name = None;

        match &resolved_input.kind {
            MediaInputKind::Url { url } => {
                ensure_command("yt-dlp")?;
                ensure_command("ffmpeg")?;
                let template = entry_dir.join("download.%(ext)s").display().to_string();
                let mut args = vec!["-f", "bestaudio/best"];
                if console::is_quiet() {
                    args.extend(["--quiet", "--no-warnings"]);
                }
                args.extend(["-o", template.as_str(), url]);

                let download = duct::cmd("yt-dlp", args).stdout_null();
                let download = if console::is_quiet() {
                    download.stderr_null()
                } else {
                    download
                };
                download
                    .run()
                    .map_err(|error| eyre!("failed to launch yt-dlp: {error}"))?;

                let media_path = find_downloaded_media(&entry_dir)?;
                media_file_name = media_path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .map(ToOwned::to_owned);
                convert_to_cached_audio(&media_path, &audio_path)?;
            }
            MediaInputKind::LocalFile { path } => {
                ensure_command("ffmpeg")?;
                convert_to_cached_audio(path, &audio_path)?;
            }
        }

        store_audio(
            self.app_paths,
            AudioCacheEntry {
                source_key: &resolved_input.source_key,
                display_name: &resolved_input.display_name,
                audio_file_name: "audio.wav",
                media_file_name: media_file_name.as_deref(),
            },
        )?;

        Ok(CachedAudio {
            display_name: resolved_input.display_name.clone(),
            audio_path,
        })
    }
}

fn build_transcript_from_audio(
    app_paths: &AppPaths,
    normalized_audio: Arc<[f32]>,
) -> Result<(String, Vec<SpeakerTurn>)> {
    let scriptrs_cache_dir = app_paths.scriptrs_model_cache();
    let speakrs_cache_dir = app_paths.speakrs_model_cache();
    let diarization_worker = DiarizationWorker::spawn(speakrs_cache_dir);
    let transcription_worker = TranscriptionWorker::spawn(scriptrs_cache_dir);
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
    let transcript = render_transcript(&turns);
    Ok((transcript, turns))
}

fn load_normalized_audio(audio_path: &Path) -> Result<Arc<[f32]>> {
    let decode_started = Instant::now();
    console::info("Decoding audio");
    let decoded_audio = decode_audio(audio_path)?;
    let normalized_audio = normalize_audio(&decoded_audio);
    if normalized_audio.is_empty() {
        return Err(eyre!("decoded audio was empty"));
    }

    debug!(
        "Decoded and normalized audio in {:.2}s",
        decode_started.elapsed().as_secs_f64()
    );
    debug!("normalized {} samples", normalized_audio.len());
    Ok(Arc::<[f32]>::from(normalized_audio))
}
