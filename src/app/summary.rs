mod generator;
mod input;

use color_eyre::Result;
use std::path::{Path, PathBuf};

use super::transcription::TranscriptionPipeline;
use crate::cli::SummarizeArgs;
use crate::output::{commit_summary, open_path, stage_summary};
use crate::paths::{AppPaths, RunPaths};
use crate::summary::SummaryMode;
use crate::utils::expand_path;

use generator::SummaryGenerator;
pub(crate) use input::SummaryInputResolver;

pub(super) fn run_summarize(
    app_paths: &AppPaths,
    force: bool,
    args: &SummarizeArgs,
    run_paths: Option<&RunPaths>,
) -> Result<()> {
    let transcription = TranscriptionPipeline::new(app_paths, force);
    let summary_input = SummaryInputResolver::new(&transcription).resolve(&args.input)?;
    let summary_mode = selected_summary_mode(args);
    let summary_model_dir = resolve_summary_model_dir(args.summary_model_dir.as_deref())?;
    let generated_summary = SummaryGenerator::new(app_paths, force).summarize(
        &summary_input,
        summary_mode,
        summary_model_dir.as_deref(),
    )?;

    write_summary_output(run_paths, args.open, &generated_summary.markdown)
}

fn write_summary_output(run_paths: Option<&RunPaths>, open: bool, markdown: &str) -> Result<()> {
    if let Some(run_paths) = run_paths {
        let staged_path = stage_summary(&run_paths.scratch_dir, markdown)?;
        commit_summary(&staged_path, &run_paths.summary_path)?;
        println!("{}", run_paths.summary_path.display());
        if open {
            open_path(&run_paths.summary_path)?;
        }
    } else {
        println!("{markdown}");
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::{SummaryInputResolver, selected_summary_mode};
    use crate::paths::AppPaths;
    use crate::summary::SummaryMode;
    use clap::Parser;
    use color_eyre::Result;
    use std::fs;

    use super::super::transcription::TranscriptionPipeline;

    fn test_paths(name: &str) -> AppPaths {
        AppPaths {
            cache_dir: std::env::temp_dir().join(name),
        }
    }

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
    fn structured_transcript_file_is_loaded_without_media_resolution() -> Result<()> {
        let root = std::env::temp_dir().join("smrze-summary-structured-transcript");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root)?;
        let transcript_path = root.join("meeting.txt");
        fs::write(
            &transcript_path,
            "[00:00:01.000-00:00:02.000] Speaker 1: Hello",
        )?;

        let app_paths = test_paths("smrze-summary-structured-transcript-cache");
        let transcription = TranscriptionPipeline::new(&app_paths, false);
        let resolver = SummaryInputResolver::new(&transcription);
        let summary_input = resolver.resolve(transcript_path.to_str().expect("valid path"))?;

        assert_eq!(summary_input.display_name, "meeting");
        assert_eq!(summary_input.turns.len(), 1);
        assert_eq!(summary_input.turns[0].text, "Hello");

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&app_paths.cache_dir);
        Ok(())
    }

    #[test]
    fn plain_text_transcript_file_is_loaded_without_media_resolution() -> Result<()> {
        let root = std::env::temp_dir().join("smrze-summary-plain-transcript");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root)?;
        let transcript_path = root.join("notes.txt");
        fs::write(&transcript_path, "first line\n\nsecond line")?;

        let app_paths = test_paths("smrze-summary-plain-transcript-cache");
        let transcription = TranscriptionPipeline::new(&app_paths, false);
        let resolver = SummaryInputResolver::new(&transcription);
        let summary_input = resolver.resolve(transcript_path.to_str().expect("valid path"))?;

        assert_eq!(summary_input.display_name, "notes");
        assert_eq!(summary_input.turns.len(), 2);
        assert_eq!(summary_input.turns[0].speaker, "Speaker 1");
        assert_eq!(summary_input.turns[1].text, "second line");

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&app_paths.cache_dir);
        Ok(())
    }
}
