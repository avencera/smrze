use clap::Parser;
use color_eyre::{
    Result,
    eyre::{Context, eyre},
};
use std::time::Instant;
use tracing::{debug, info};

use crate::audio::{audio_fingerprint, decode_audio, normalize_audio};
use crate::cli::Args;
use crate::input::resolve_input;
use crate::models::{build_diarization_pipeline, build_transcription_pipeline, ensure_models};
use crate::output::{commit_transcript, open_path, remove_path_if_exists, stage_transcript};
use crate::paths::{AppPaths, RunPaths};
use crate::speakers::build_turns;
use crate::transcript::render_transcript;
use crate::utils::now_millis;
use speakrs::BatchInput;

pub fn run() -> Result<()> {
    let args = Args::parse();
    let app_paths = AppPaths::resolve()?;
    let run_token = format!("run-{}", now_millis()?);
    let initial_scratch_dir = app_paths.cache_dir.join("runs").join(&run_token);
    std::fs::create_dir_all(&initial_scratch_dir)
        .with_context(|| format!("failed to create {}", initial_scratch_dir.display()))?;

    let result = run_inner(&app_paths, &args, &run_token, &initial_scratch_dir);
    if let Err(error) = remove_path_if_exists(&initial_scratch_dir) {
        eprintln!(
            "failed to clean scratch dir {}: {error:#}",
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
    let resolved_input = resolve_input(&args.input, &app_paths.cache_dir.join("downloads"))?;
    let scriptrs_cache_dir = app_paths.scriptrs_model_cache();
    let speakrs_cache_dir = app_paths.speakrs_model_cache();
    let model_prefetch =
        std::thread::spawn(move || ensure_models(&scriptrs_cache_dir, &speakrs_cache_dir));

    let decode_started = Instant::now();
    info!("decoding audio");
    let decoded_audio = decode_audio(&resolved_input.media_path)?;
    let normalized_audio = normalize_audio(&decoded_audio);
    if normalized_audio.is_empty() {
        return Err(eyre!("decoded audio was empty"));
    }
    info!(
        "Decoded and normalized audio in {:.2}s",
        decode_started.elapsed().as_secs_f64()
    );
    debug!("normalized {} samples", normalized_audio.len());

    let logical_key = logical_key(&resolved_input.source_identity, &normalized_audio);
    let run_paths = app_paths.create_run(
        &resolved_input.display_name,
        &logical_key,
        args.output_dir.as_deref(),
        run_id,
    )?;

    debug!("waiting for model prefetch to finish");
    let prefetched_models = model_prefetch
        .join()
        .map_err(|_| eyre!("model prefetch thread panicked"))??;

    let result = execute_pipeline(&run_paths, &normalized_audio, prefetched_models, args.open);
    if result.is_err() {
        cleanup_failed_output(&run_paths)?;
    }
    result
}

fn execute_pipeline(
    run_paths: &RunPaths,
    normalized_audio: &[f32],
    models: crate::models::PrefetchedModels,
    open_transcript: bool,
) -> Result<()> {
    let diarization_build_started = Instant::now();
    let mut diarization_pipeline = build_diarization_pipeline(models.diarization)?;
    info!(
        "Built diarization stage in {:.2}s",
        diarization_build_started.elapsed().as_secs_f64()
    );

    info!("running diarization");
    let diarization_started = Instant::now();
    let diarization_results = diarization_pipeline.run_batch(&[BatchInput {
        audio: normalized_audio,
        file_id: "input",
    }])?;
    let diarization = diarization_results
        .into_iter()
        .next()
        .ok_or_else(|| eyre!("diarization returned no results"))?;
    info!(
        "Finished diarization in {:.2}s",
        diarization_started.elapsed().as_secs_f64()
    );
    debug!(
        "diarization produced {} segments",
        diarization.segments.len()
    );

    drop(diarization_pipeline);

    let transcription_build_started = Instant::now();
    let transcription_pipeline = build_transcription_pipeline(models.transcription)?;
    info!(
        "Built transcription stage in {:.2}s",
        transcription_build_started.elapsed().as_secs_f64()
    );

    info!("running transcription");
    let transcription_started = Instant::now();
    let transcription = transcription_pipeline.run(normalized_audio)?;
    info!(
        "Finished transcription in {:.2}s",
        transcription_started.elapsed().as_secs_f64()
    );
    debug!(
        "transcription produced {} timed tokens",
        transcription.tokens.len()
    );

    let turns = build_turns(&transcription.tokens, &diarization);
    let transcript = render_transcript(&turns);
    let staged_path = stage_transcript(&run_paths.scratch_dir, &transcript)?;
    commit_transcript(&staged_path, &run_paths.final_path)?;
    if open_transcript {
        open_path(&run_paths.final_path)?;
    }

    println!("{}", run_paths.final_path.display());
    Ok(())
}

fn logical_key(source_identity: &str, normalized_audio: &[f32]) -> String {
    let audio_hash = audio_fingerprint(normalized_audio);
    blake3::hash(format!("v1\0{source_identity}\0{audio_hash}").as_bytes())
        .to_hex()
        .to_string()
}

fn cleanup_failed_output(run_paths: &RunPaths) -> Result<()> {
    remove_path_if_exists(&run_paths.final_path)?;
    if run_paths.user_provided_output_dir {
        return Ok(());
    }

    if is_dir_empty(&run_paths.final_dir)? {
        remove_path_if_exists(&run_paths.final_dir)?;
    }
    Ok(())
}

fn is_dir_empty(path: &std::path::Path) -> Result<bool> {
    if !path.exists() {
        return Ok(true);
    }
    Ok(std::fs::read_dir(path)?.next().is_none())
}
