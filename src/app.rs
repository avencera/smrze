use color_eyre::{
    Result,
    eyre::{Context, eyre},
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, UNIX_EPOCH};
use tracing::{debug, warn};

use crate::audio::{decode_audio, normalize_audio};
use crate::cache::{
    CacheKind, cache_file_path, clear_cache_entry, ensure_cache_entry_dir, load_manifest,
    spawn_cache_sweeper, write_json_file, write_manifest, write_text_file,
};
use crate::cli::{Cli, Command, SummarizeArgs, TranscriptArgs};
use crate::console;
use crate::input::{is_url, local_file_source_key, materialize_audio, resolve_media_input};
use crate::output::{
    commit_summary, commit_transcript, open_path, remove_path_if_exists, stage_summary,
    stage_transcript,
};
use crate::paths::{AppPaths, RunPaths};
use crate::speakers::{SpeakerTurn, build_turns};
use crate::summary::{GeneratedSummary, SummaryMode, generate_summary};
use crate::transcript::{parse_transcript, render_transcript};
use crate::utils::{expand_path, hash_string, now_millis};
use crate::workers::{DiarizationWorker, TranscriptionWorker};

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

#[derive(Debug, Clone)]
struct CachedTranscript {
    display_name: String,
    source_key: String,
    transcript_hash: String,
    transcript: String,
    turns: Vec<SpeakerTurn>,
}

#[derive(Debug, Clone)]
struct SummaryInput {
    display_name: String,
    source_key: String,
    transcript_hash: String,
    turns: Vec<SpeakerTurn>,
}

pub fn run(cli: Cli) -> Result<()> {
    console::set_quiet(cli.quiet);
    let app_paths = AppPaths::resolve()?;
    spawn_cache_sweeper(app_paths.clone());

    match cli.command {
        Command::Transcript(args) => run_transcript_command(&app_paths, cli.force, args),
        Command::Summarize(args) => run_summarize_command(&app_paths, cli.force, args),
    }
}

fn run_transcript_command(app_paths: &AppPaths, force: bool, args: TranscriptArgs) -> Result<()> {
    if args.open && args.output.is_none() {
        return Err(eyre!("--open requires --output"));
    }

    let run_paths = create_run_paths(app_paths, args.output.as_deref())?;
    let result = run_transcript_inner(app_paths, force, &args, run_paths.as_ref());
    finish_run(run_paths, result)
}

fn run_transcript_inner(
    app_paths: &AppPaths,
    force: bool,
    args: &TranscriptArgs,
    run_paths: Option<&RunPaths>,
) -> Result<()> {
    let resolved_input = resolve_media_input(&args.input)?;
    let transcript = transcribe_media_input(
        app_paths,
        &resolved_input.source_key,
        &resolved_input.display_name,
        &args.input,
        force,
    )?;

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

fn run_summarize_command(app_paths: &AppPaths, force: bool, args: SummarizeArgs) -> Result<()> {
    if args.open && args.output.is_none() {
        return Err(eyre!("--open requires --output"));
    }

    let run_paths = create_run_paths(app_paths, args.output.as_deref())?;
    let result = run_summarize_inner(app_paths, force, &args, run_paths.as_ref());
    finish_run(run_paths, result)
}

fn run_summarize_inner(
    app_paths: &AppPaths,
    force: bool,
    args: &SummarizeArgs,
    run_paths: Option<&RunPaths>,
) -> Result<()> {
    let summary_mode = selected_summary_mode(args);
    let summary_model_dir = resolve_summary_model_dir(args.summary_model_dir.as_deref())?;
    let summary_input = resolve_summary_input(app_paths, &args.input, force)?;
    let generated_summary = summarize_input(
        app_paths,
        &summary_input,
        summary_mode,
        summary_model_dir.as_deref(),
        force,
    )?;

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

fn resolve_summary_input(app_paths: &AppPaths, input: &str, force: bool) -> Result<SummaryInput> {
    if is_url(input) {
        let media_input = resolve_media_input(input)?;
        let transcript = transcribe_media_input(
            app_paths,
            &media_input.source_key,
            &media_input.display_name,
            input,
            force,
        )?;
        return Ok(SummaryInput {
            display_name: transcript.display_name,
            source_key: transcript.source_key,
            transcript_hash: transcript.transcript_hash,
            turns: transcript.turns,
        });
    }

    let path = expand_path(Path::new(input))?
        .canonicalize()
        .with_context(|| format!("failed to resolve {input}"))?;
    if !path.exists() {
        return Err(eyre!("input file not found: {}", path.display()));
    }

    if let Some(transcript_input) = try_load_transcript_file(&path)? {
        return Ok(transcript_input);
    }

    let transcript = transcribe_media_input(
        app_paths,
        &local_file_source_key(&path)?,
        &crate::input::resolve_media_input(input)?.display_name,
        input,
        force,
    )?;
    Ok(SummaryInput {
        display_name: transcript.display_name,
        source_key: transcript.source_key,
        transcript_hash: transcript.transcript_hash,
        turns: transcript.turns,
    })
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
        display_name: crate::utils::sanitize_name(&crate::utils::file_stem_name(path)?),
        source_key: local_file_source_key(path)?,
        transcript_hash: hash_string(&transcript_text),
        turns,
    }))
}

fn transcribe_media_input(
    app_paths: &AppPaths,
    source_key: &str,
    display_name: &str,
    input: &str,
    force: bool,
) -> Result<CachedTranscript> {
    if force {
        clear_cache_entry(app_paths, CacheKind::Transcript, source_key)?;
    } else if let Some(cached_transcript) = load_transcript_cache(app_paths, source_key)? {
        return Ok(cached_transcript);
    }
    clear_cache_entry(app_paths, CacheKind::Transcript, source_key)?;

    let resolved_input = resolve_media_input(input)?;
    let cached_audio = materialize_audio(app_paths, &resolved_input, force)?;
    let normalized_audio = load_normalized_audio(&cached_audio.audio_path)?;
    let built_transcript = build_transcript_from_audio(app_paths, normalized_audio)?;
    store_transcript_cache(
        app_paths,
        source_key,
        display_name,
        &built_transcript.transcript,
        &built_transcript.turns,
    )?;

    Ok(CachedTranscript {
        display_name: cached_audio.display_name,
        source_key: source_key.to_owned(),
        transcript_hash: built_transcript.transcript_hash,
        transcript: built_transcript.transcript,
        turns: built_transcript.turns,
    })
}

fn build_transcript_from_audio(
    app_paths: &AppPaths,
    normalized_audio: Arc<[f32]>,
) -> Result<CachedTranscript> {
    let scriptrs_cache_dir = app_paths.scriptrs_model_cache();
    let speakrs_cache_dir = app_paths.speakrs_model_cache();
    let diarization_worker = DiarizationWorker::spawn(speakrs_cache_dir);
    let transcription_worker = TranscriptionWorker::spawn(scriptrs_cache_dir);

    let result =
        execute_transcription_pipeline(normalized_audio, diarization_worker, transcription_worker);
    match result {
        Ok((transcript, turns)) => Ok(CachedTranscript {
            display_name: String::new(),
            source_key: String::new(),
            transcript_hash: hash_string(&transcript),
            transcript,
            turns,
        }),
        Err(error) => Err(error),
    }
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

fn summarize_input(
    app_paths: &AppPaths,
    summary_input: &SummaryInput,
    summary_mode: SummaryMode,
    summary_model_dir: Option<&Path>,
    force: bool,
) -> Result<GeneratedSummary> {
    let cache_key = summary_cache_key(
        &summary_input.source_key,
        &summary_input.transcript_hash,
        summary_mode,
        summary_model_dir,
    );
    if force {
        clear_cache_entry(app_paths, CacheKind::Summary, &cache_key)?;
    } else if let Some(cached_summary) = load_summary_cache(app_paths, &cache_key)? {
        return Ok(cached_summary);
    }
    clear_cache_entry(app_paths, CacheKind::Summary, &cache_key)?;

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
        app_paths,
    )?;
    debug!(
        "Finished summary in {:.2}s",
        summary_started.elapsed().as_secs_f64()
    );
    store_summary_cache(
        app_paths,
        &cache_key,
        summary_input,
        summary_mode,
        summary_model_dir,
        &generated_summary,
    )?;
    Ok(generated_summary)
}

fn load_transcript_cache(
    app_paths: &AppPaths,
    source_key: &str,
) -> Result<Option<CachedTranscript>> {
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

fn store_transcript_cache(
    app_paths: &AppPaths,
    source_key: &str,
    display_name: &str,
    transcript: &str,
    turns: &[SpeakerTurn],
) -> Result<()> {
    let entry_dir = ensure_cache_entry_dir(app_paths, CacheKind::Transcript, source_key)?;
    let transcript_path = entry_dir.join("transcript.txt");
    let turns_path = entry_dir.join("turns.json");
    write_text_file(&transcript_path, transcript)?;
    write_json_file(&turns_path, &turns)?;
    write_manifest(
        &entry_dir.join("manifest.json"),
        &TranscriptManifest {
            created_at_ms: now_millis_u64()?,
            source_key: source_key.to_owned(),
            display_name: display_name.to_owned(),
            transcript_hash: hash_string(transcript),
            transcript_file_name: "transcript.txt".to_owned(),
            turns_file_name: "turns.json".to_owned(),
        },
    )?;
    Ok(())
}

fn load_summary_cache(app_paths: &AppPaths, cache_key: &str) -> Result<Option<GeneratedSummary>> {
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
    let backend = parse_summary_backend(&manifest.actual_backend)?;
    Ok(Some(GeneratedSummary { markdown, backend }))
}

fn store_summary_cache(
    app_paths: &AppPaths,
    cache_key: &str,
    summary_input: &SummaryInput,
    summary_mode: SummaryMode,
    summary_model_dir: Option<&Path>,
    generated_summary: &GeneratedSummary,
) -> Result<()> {
    let entry_dir = ensure_cache_entry_dir(app_paths, CacheKind::Summary, cache_key)?;
    let summary_path = entry_dir.join("summary.md");
    write_text_file(&summary_path, &generated_summary.markdown)?;
    write_manifest(
        &entry_dir.join("manifest.json"),
        &SummaryManifest {
            created_at_ms: now_millis_u64()?,
            source_key: summary_input.source_key.clone(),
            display_name: summary_input.display_name.clone(),
            transcript_hash: summary_input.transcript_hash.clone(),
            requested_mode: summary_mode.requested_key().to_owned(),
            actual_backend: generated_summary.backend.cache_key().to_owned(),
            summary_model_dir: summary_model_dir.map(|path| path.display().to_string()),
            summary_file_name: "summary.md".to_owned(),
        },
    )?;
    Ok(())
}

fn parse_summary_backend(value: &str) -> Result<crate::summary_backend::SummaryBackend> {
    match value {
        "apple-foundation" => Ok(crate::summary_backend::SummaryBackend::AppleFoundation),
        "gemma4-e2b" => Ok(crate::summary_backend::SummaryBackend::Gemma4E2b),
        "gemma4-e4b" => Ok(crate::summary_backend::SummaryBackend::Gemma4E4b),
        _ => Err(eyre!("unknown summary backend {value}")),
    }
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

fn summary_cache_key(
    source_key: &str,
    transcript_hash: &str,
    summary_mode: SummaryMode,
    summary_model_dir: Option<&Path>,
) -> String {
    format!(
        "{source_key}\n{transcript_hash}\n{}\n{}",
        summary_mode.requested_key(),
        summary_model_dir
            .map(|path| path.display().to_string())
            .unwrap_or_default()
    )
}

fn create_run_paths(app_paths: &AppPaths, output_dir: Option<&Path>) -> Result<Option<RunPaths>> {
    let Some(output_dir) = output_dir else {
        return Ok(None);
    };
    let run_id = format!("run-{}", now_millis()?);
    Ok(Some(app_paths.create_run(output_dir, &run_id)?))
}

fn finish_run(run_paths: Option<RunPaths>, result: Result<()>) -> Result<()> {
    let had_error = result.is_err();
    if let Some(run_paths) = run_paths.as_ref() {
        if had_error && !run_paths.final_path.exists() && !run_paths.summary_path.exists() {
            cleanup_failed_output(run_paths)?;
        }
        if let Err(error) = remove_path_if_exists(&run_paths.scratch_dir) {
            warn!(
                "Failed to clean scratch dir {}: {error:#}",
                run_paths.scratch_dir.display()
            );
        }
    }
    result
}

fn cleanup_failed_output(run_paths: &RunPaths) -> Result<()> {
    remove_path_if_exists(&run_paths.final_path)?;
    remove_path_if_exists(&run_paths.summary_path)?;

    if is_dir_empty(&run_paths.final_dir)? {
        remove_path_if_exists(&run_paths.final_dir)?;
    }
    Ok(())
}

fn is_dir_empty(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(true);
    }
    Ok(std::fs::read_dir(path)?.next().is_none())
}

fn now_millis_u64() -> Result<u64> {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| eyre!("system clock before unix epoch: {error}"))?
        .as_millis()
        .try_into()
        .map_err(|_| eyre!("system time does not fit into u64"))
}

#[cfg(test)]
mod tests {
    use super::{selected_summary_mode, summary_cache_key};
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

    #[test]
    fn summary_cache_key_depends_on_requested_mode() {
        let auto_key = summary_cache_key("source", "hash", SummaryMode::Auto, None);
        let strict_key = summary_cache_key(
            "source",
            "hash",
            SummaryMode::Backend(crate::summary_backend::SummaryBackend::AppleFoundation),
            None,
        );
        assert_ne!(auto_key, strict_key);
    }
}
