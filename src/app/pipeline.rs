use color_eyre::{
    Result,
    eyre::{Context, eyre},
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, warn};

use crate::audio::{decode_audio, normalize_audio};
use crate::cache::{
    CacheKind, CachedTranscript, SummaryCacheEntry, TranscriptCacheEntry, clear_cache_entry,
    load_summary, load_transcript, store_summary, store_transcript, summary_cache_key,
};
use crate::cli::{SummarizeArgs, TranscriptArgs};
use crate::console;
use crate::input::{is_url, local_file_source_key, materialize_audio, resolve_media_input};
use crate::output::{
    commit_summary, commit_transcript, open_path, stage_summary, stage_transcript,
};
use crate::paths::{AppPaths, RunPaths};
use crate::speakers::{SpeakerTurn, build_turns};
use crate::summary::{GeneratedSummary, SummaryMode, generate_summary};
use crate::transcript::{parse_transcript, render_transcript};
use crate::utils::{expand_path, file_stem_name, hash_string, sanitize_name};
use crate::workers::{DiarizationWorker, TranscriptionWorker};

#[derive(Debug, Clone)]
struct SummaryInput {
    display_name: String,
    source_key: String,
    transcript_hash: String,
    turns: Vec<SpeakerTurn>,
}

pub(super) fn run_transcript(
    app_paths: &AppPaths,
    force: bool,
    args: &TranscriptArgs,
    run_paths: Option<&RunPaths>,
) -> Result<()> {
    let resolved_input = resolve_media_input(&args.input)?;
    let pipeline = Pipeline { app_paths, force };
    let transcript = pipeline.transcribe_media_input(&resolved_input)?;

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

pub(super) fn run_summarize(
    app_paths: &AppPaths,
    force: bool,
    args: &SummarizeArgs,
    run_paths: Option<&RunPaths>,
) -> Result<()> {
    let pipeline = Pipeline { app_paths, force };
    let summary_mode = selected_summary_mode(args);
    let summary_model_dir = resolve_summary_model_dir(args.summary_model_dir.as_deref())?;
    let summary_input = pipeline.resolve_summary_input(&args.input)?;
    let generated_summary =
        pipeline.summarize(&summary_input, summary_mode, summary_model_dir.as_deref())?;

    if let Some(run_paths) = run_paths {
        let staged_path = stage_summary(&run_paths.scratch_dir, &generated_summary.markdown)?;
        commit_summary(&staged_path, &run_paths.summary_path)?;
        println!("{}", run_paths.summary_path.display());
        if args.open {
            open_path(&run_paths.summary_path)?;
        }
    } else {
        println!("{}", generated_summary.markdown);
    }
    Ok(())
}

struct Pipeline<'a> {
    app_paths: &'a AppPaths,
    force: bool,
}

impl<'a> Pipeline<'a> {
    fn resolve_summary_input(&self, input: &str) -> Result<SummaryInput> {
        if is_url(input) {
            let resolved_input = resolve_media_input(input)?;
            return self.transcript_summary_input(&resolved_input);
        }

        let path = expand_path(Path::new(input))?
            .canonicalize()
            .with_context(|| format!("failed to resolve {input}"))?;
        if !path.exists() {
            return Err(eyre!("input file not found: {}", path.display()));
        }

        if let Some(summary_input) = try_load_transcript_file(&path)? {
            return Ok(summary_input);
        }

        let resolved_input = resolve_media_input(input)?;
        self.transcript_summary_input(&resolved_input)
    }

    fn transcript_summary_input(
        &self,
        resolved_input: &crate::input::ResolvedMediaInput,
    ) -> Result<SummaryInput> {
        let transcript = self.transcribe_media_input(resolved_input)?;
        Ok(SummaryInput {
            display_name: transcript.display_name,
            source_key: transcript.source_key,
            transcript_hash: transcript.transcript_hash,
            turns: transcript.turns,
        })
    }

    fn transcribe_media_input(
        &self,
        resolved_input: &crate::input::ResolvedMediaInput,
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

        let cached_audio = materialize_audio(self.app_paths, resolved_input, self.force)?;
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

    fn summarize(
        &self,
        summary_input: &SummaryInput,
        summary_mode: SummaryMode,
        summary_model_dir: Option<&Path>,
    ) -> Result<GeneratedSummary> {
        let cache_key = summary_cache_key(
            &summary_input.source_key,
            &summary_input.transcript_hash,
            summary_mode,
            summary_model_dir,
        );
        if self.force {
            clear_cache_entry(self.app_paths, CacheKind::Summary, &cache_key)?;
        } else if let Some(cached_summary) = load_summary(self.app_paths, &cache_key)? {
            return Ok(GeneratedSummary {
                markdown: cached_summary.markdown,
                backend: cached_summary.backend,
            });
        }
        clear_cache_entry(self.app_paths, CacheKind::Summary, &cache_key)?;

        console::info(format!(
            "Generating summary with {}",
            summary_mode_label(summary_mode)
        ));
        let summary_started = Instant::now();
        let generated_summary = generate_summary(
            &summary_input.display_name,
            &summary_input.turns,
            summary_mode,
            summary_model_dir,
            self.app_paths,
        )?;
        debug!(
            "Finished summary in {:.2}s",
            summary_started.elapsed().as_secs_f64()
        );
        store_summary(
            self.app_paths,
            SummaryCacheEntry {
                cache_key: &cache_key,
                source_key: &summary_input.source_key,
                display_name: &summary_input.display_name,
                transcript_hash: &summary_input.transcript_hash,
                requested_mode: summary_mode,
                summary_model_dir,
                markdown: &generated_summary.markdown,
                backend: generated_summary.backend,
            },
        )?;
        Ok(generated_summary)
    }
}

fn try_load_transcript_file(path: &Path) -> Result<Option<SummaryInput>> {
    let transcript_text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::InvalidData => return Ok(None),
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {}", path.display()));
        }
    };
    let Some(turns) = parse_transcript(&transcript_text) else {
        return Ok(None);
    };

    Ok(Some(SummaryInput {
        display_name: sanitize_name(&file_stem_name(path)?),
        source_key: local_file_source_key(path)?,
        transcript_hash: hash_string(&transcript_text),
        turns,
    }))
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

fn selected_summary_mode(args: &SummarizeArgs) -> SummaryMode {
    match args.summary_backend {
        Some(backend) => SummaryMode::Backend(backend),
        None => SummaryMode::Auto,
    }
}

fn resolve_summary_model_dir(summary_model_dir: Option<&Path>) -> Result<Option<PathBuf>> {
    summary_model_dir.map(expand_path).transpose()
}

fn summary_mode_label(summary_mode: SummaryMode) -> &'static str {
    match summary_mode {
        SummaryMode::Auto => "Apple Foundation",
        SummaryMode::Backend(backend) => backend.display_name(),
    }
}

#[cfg(test)]
mod tests {
    use super::selected_summary_mode;
    use crate::summary::SummaryMode;
    use clap::Parser;

    #[test]
    fn summary_defaults_to_auto_mode() {
        let cli = crate::Cli::parse_from(["smrze", "summarize", "input.wav"]);
        let crate::cli::Command::Summarize(args) = cli.command else {
            panic!("expected summarize command");
        };
        assert_eq!(selected_summary_mode(&args), SummaryMode::Auto);
    }

    #[test]
    fn explicit_summary_backend_wins() {
        let cli = crate::Cli::parse_from([
            "smrze",
            "summarize",
            "input.wav",
            "--summary-backend",
            "apple-foundation",
        ]);
        let crate::cli::Command::Summarize(args) = cli.command else {
            panic!("expected summarize command");
        };
        assert_eq!(
            selected_summary_mode(&args),
            SummaryMode::Backend(crate::summary_backend::SummaryBackend::AppleFoundation)
        );
    }
}
