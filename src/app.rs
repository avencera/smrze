mod summary;
mod transcription;

use color_eyre::{Result, eyre::eyre};
use std::path::Path;
use tracing::warn;

use crate::cache::spawn_cache_sweeper;
use crate::cli::{Cli, Command, SummarizeArgs, TranscriptArgs};
use crate::console;
use crate::output::remove_path_if_exists;
use crate::paths::{AppPaths, RunPaths};
use crate::utils::now_millis;

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
    validate_open_flag(args.open, args.output.as_deref())?;

    let run_paths = create_run_paths(app_paths, args.output.as_deref())?;
    let result = transcription::run_transcript(app_paths, force, &args, run_paths.as_ref());
    finish_run(run_paths, result)
}

fn run_summarize_command(app_paths: &AppPaths, force: bool, args: SummarizeArgs) -> Result<()> {
    validate_open_flag(args.open, args.output.as_deref())?;

    let run_paths = create_run_paths(app_paths, args.output.as_deref())?;
    let result = summary::run_summarize(app_paths, force, &args, run_paths.as_ref());
    finish_run(run_paths, result)
}

fn validate_open_flag(open: bool, output_dir: Option<&Path>) -> Result<()> {
    if open && output_dir.is_none() {
        return Err(eyre!("--open requires --output"));
    }

    Ok(())
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
