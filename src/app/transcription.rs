mod audio;
mod pipeline;

use color_eyre::Result;

use crate::cli::TranscriptArgs;
use crate::input::resolve_media_input;
use crate::output::{commit_transcript, open_path, stage_transcript};
use crate::paths::{AppPaths, RunPaths};

pub(crate) use pipeline::TranscriptionPipeline;

pub(super) fn run_transcript(
    app_paths: &AppPaths,
    force: bool,
    args: &TranscriptArgs,
    run_paths: Option<&RunPaths>,
) -> Result<()> {
    let resolved_input = resolve_media_input(&args.input)?;
    let pipeline = TranscriptionPipeline::new(app_paths, force);
    let transcript = pipeline.transcribe_resolved_input(&resolved_input)?;

    write_transcript_output(run_paths, args.open, &transcript.transcript)
}

fn write_transcript_output(
    run_paths: Option<&RunPaths>,
    open: bool,
    transcript: &str,
) -> Result<()> {
    if let Some(run_paths) = run_paths {
        let staged_path = stage_transcript(&run_paths.scratch_dir, transcript)?;
        commit_transcript(&staged_path, &run_paths.final_path)?;
        println!("{}", run_paths.final_path.display());
        if open {
            open_path(&run_paths.final_path)?;
        }
    } else {
        println!("{transcript}");
    }
    Ok(())
}
