use color_eyre::{
    Result,
    eyre::{Context, eyre},
};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, warn};

use crate::audio::{decode_audio, normalize_audio};
use crate::cli::Args;
use crate::console;
use crate::input::resolve_input;
use crate::output::{
    commit_summary, commit_transcript, open_path, remove_path_if_exists, stage_summary,
    stage_transcript,
};
use crate::paths::{AppPaths, RunPaths};
use crate::speakers::build_turns;
use crate::summary::{generate_summary, render_markdown};
use crate::transcript::render_transcript;
use crate::utils::now_millis;
use crate::workers::{DiarizationWorker, TranscriptionWorker};

pub fn run(args: Args) -> Result<()> {
    console::set_quiet(args.quiet);
    if args.open && args.output.is_none() {
        return Err(eyre!("--open requires --output"));
    }
    let app_paths = AppPaths::resolve()?;
    let run_token = format!("run-{}", now_millis()?);
    let initial_scratch_dir = app_paths.cache_dir.join("runs").join(&run_token);
    std::fs::create_dir_all(&initial_scratch_dir)
        .with_context(|| format!("failed to create {}", initial_scratch_dir.display()))?;

    let result = run_inner(&app_paths, &args, &run_token, &initial_scratch_dir);
    if let Err(error) = remove_path_if_exists(&initial_scratch_dir) {
        warn!(
            "Failed to clean scratch dir {}: {error:#}",
            initial_scratch_dir.display()
        );
    }
    result
}

fn run_inner(
    app_paths: &AppPaths,
    args: &Args,
    run_id: &str,
    _initial_scratch_dir: &std::path::Path,
) -> Result<()> {
    let scriptrs_cache_dir = app_paths.scriptrs_model_cache();
    let speakrs_cache_dir = app_paths.speakrs_model_cache();
    let diarization_worker = DiarizationWorker::spawn(speakrs_cache_dir);
    let transcription_worker = TranscriptionWorker::spawn(scriptrs_cache_dir);
    let resolved_input = match resolve_input(&args.input, &app_paths.cache_dir.join("downloads")) {
        Ok(resolved_input) => resolved_input,
        Err(error) => {
            cancel_workers(diarization_worker, transcription_worker);
            return Err(error);
        }
    };

    let decode_started = Instant::now();
    console::info("Decoding audio");
    let decoded_audio = match decode_audio(&resolved_input.media_path) {
        Ok(decoded_audio) => decoded_audio,
        Err(error) => {
            cancel_workers(diarization_worker, transcription_worker);
            return Err(error);
        }
    };
    let normalized_audio = normalize_audio(&decoded_audio);
    if normalized_audio.is_empty() {
        cancel_workers(diarization_worker, transcription_worker);
        return Err(eyre!("decoded audio was empty"));
    }
    let normalized_audio: Arc<[f32]> = normalized_audio.into();
    debug!(
        "Decoded and normalized audio in {:.2}s",
        decode_started.elapsed().as_secs_f64()
    );
    debug!("normalized {} samples", normalized_audio.len());

    let run_paths = match args.output.as_deref() {
        Some(output_dir) => match app_paths.create_run(output_dir, run_id) {
            Ok(run_paths) => Some(run_paths),
            Err(error) => {
                cancel_workers(diarization_worker, transcription_worker);
                return Err(error);
            }
        },
        None => None,
    };

    let result = execute_pipeline(
        run_paths.as_ref(),
        &resolved_input.display_name,
        normalized_audio,
        diarization_worker,
        transcription_worker,
        args.summary,
        args.open,
    );
    if result.is_err()
        && let Some(run_paths) = run_paths.as_ref()
        && !run_paths.final_path.exists()
    {
        cleanup_failed_output(run_paths)?;
    }
    result
}

fn execute_pipeline(
    run_paths: Option<&RunPaths>,
    title: &str,
    normalized_audio: Arc<[f32]>,
    diarization_worker: DiarizationWorker,
    transcription_worker: TranscriptionWorker,
    generate_summary_file: bool,
    open_output: bool,
) -> Result<()> {
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
    let summary_markdown = if generate_summary_file {
        console::info("Generating summary");
        let summary_started = Instant::now();
        let summary = match generate_summary(title, &turns) {
            Ok(summary) => summary,
            Err(error) => {
                if let Some(run_paths) = run_paths {
                    remove_path_if_exists(&run_paths.summary_path)?;
                }
                return Err(error);
            }
        };
        let summary_markdown = render_markdown(&summary);
        debug!(
            "Finished summary in {:.2}s",
            summary_started.elapsed().as_secs_f64()
        );
        Some(summary_markdown)
    } else {
        None
    };

    if let Some(run_paths) = run_paths {
        let staged_path = stage_transcript(&run_paths.scratch_dir, &transcript)?;
        commit_transcript(&staged_path, &run_paths.final_path)?;
        if let Some(summary_markdown) = summary_markdown.as_deref() {
            let staged_summary_path = stage_summary(&run_paths.scratch_dir, summary_markdown)?;
            commit_summary(&staged_summary_path, &run_paths.summary_path)?;
        }

        println!("{}", run_paths.final_path.display());
        if summary_markdown.is_some() {
            println!("{}", run_paths.summary_path.display());
        }
        if open_output {
            let path_to_open = if summary_markdown.is_some() {
                &run_paths.summary_path
            } else {
                &run_paths.final_path
            };
            open_path(path_to_open)?;
        }
    } else {
        print_stdout_output(&transcript, summary_markdown.as_deref());
    }
    Ok(())
}

fn print_stdout_output(transcript: &str, summary_markdown: Option<&str>) {
    println!("{transcript}");
    if let Some(summary_markdown) = summary_markdown {
        if !transcript.is_empty() {
            println!();
        }
        println!("{summary_markdown}");
    }
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

fn cancel_workers(
    diarization_worker: DiarizationWorker,
    transcription_worker: TranscriptionWorker,
) {
    if let Err(error) = diarization_worker.cancel() {
        warn!("Failed to stop diarization worker: {error:#}");
    }
    if let Err(error) = transcription_worker.cancel() {
        warn!("Failed to stop transcription worker: {error:#}");
    }
}
