use color_eyre::{
    Result,
    eyre::{Context, eyre},
};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tracing::debug;

use super::transcription::TranscriptionPipeline;
use crate::cache::{
    CacheKind, SummaryCacheEntry, load_cache_entry, load_summary, store_summary, summary_cache_key,
};
use crate::cli::SummarizeArgs;
use crate::console;
use crate::input::{ResolvedMediaInput, is_url, local_file_source_key, resolve_media_input};
use crate::output::{commit_summary, open_path, stage_summary};
use crate::paths::{AppPaths, RunPaths};
use crate::speakers::SpeakerTurn;
use crate::summary::{GeneratedSummary, SummaryMode, generate_summary};
use crate::transcript::parse_transcript;
use crate::utils::{expand_path, file_stem_name, hash_string, sanitize_name};

pub(super) fn run_summarize(
    app_paths: &AppPaths,
    force: bool,
    args: &SummarizeArgs,
    run_paths: Option<&RunPaths>,
) -> Result<()> {
    let transcription = TranscriptionPipeline::new(app_paths, force);
    let input_resolver = SummaryInputResolver::new(&transcription);
    let summary_generator = SummaryGenerator::new(app_paths, force);
    let summary_mode = selected_summary_mode(args);
    let summary_model_dir = resolve_summary_model_dir(args.summary_model_dir.as_deref())?;
    let summary_input = input_resolver.resolve(&args.input)?;
    let generated_summary =
        summary_generator.summarize(&summary_input, summary_mode, summary_model_dir.as_deref())?;

    write_summary_output(run_paths, args.open, &generated_summary.markdown)
}

#[derive(Debug, Clone)]
struct SummaryInput {
    display_name: String,
    source_key: String,
    transcript_hash: String,
    turns: Vec<SpeakerTurn>,
}

struct SummaryInputResolver<'a> {
    transcription: &'a TranscriptionPipeline<'a>,
}

impl<'a> SummaryInputResolver<'a> {
    fn new(transcription: &'a TranscriptionPipeline<'a>) -> Self {
        Self { transcription }
    }

    fn resolve(&self, input: &str) -> Result<SummaryInput> {
        if is_url(input) {
            let resolved_input = resolve_media_input(input)?;
            return self.summary_input_from_media(&resolved_input);
        }

        let path = expand_path(Path::new(input))?
            .canonicalize()
            .with_context(|| format!("failed to resolve {input}"))?;
        if !path.exists() {
            return Err(eyre!("input file not found: {}", path.display()));
        }

        if let Some(summary_input) = self.try_load_transcript_file(&path)? {
            return Ok(summary_input);
        }

        let resolved_input = resolve_media_input(input)?;
        self.summary_input_from_media(&resolved_input)
    }

    fn summary_input_from_media(
        &self,
        resolved_input: &ResolvedMediaInput,
    ) -> Result<SummaryInput> {
        let transcript = self
            .transcription
            .transcribe_resolved_input(resolved_input)?;
        Ok(SummaryInput {
            display_name: transcript.display_name,
            source_key: transcript.source_key,
            transcript_hash: transcript.transcript_hash,
            turns: transcript.turns,
        })
    }

    fn try_load_transcript_file(&self, path: &Path) -> Result<Option<SummaryInput>> {
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
}

struct SummaryGenerator<'a> {
    app_paths: &'a AppPaths,
    force: bool,
}

impl<'a> SummaryGenerator<'a> {
    fn new(app_paths: &'a AppPaths, force: bool) -> Self {
        Self { app_paths, force }
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
        if let Some(cached_summary) = load_cache_entry(
            self.app_paths,
            CacheKind::Summary,
            &cache_key,
            self.force,
            load_summary,
        )? {
            return Ok(GeneratedSummary {
                markdown: cached_summary.markdown,
                backend: cached_summary.backend,
            });
        }

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

fn summary_mode_label(summary_mode: SummaryMode) -> &'static str {
    match summary_mode {
        SummaryMode::Auto => "Apple Foundation",
        SummaryMode::Backend(backend) => backend.display_name(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::transcription::TranscriptionPipeline;
    use super::{SummaryInputResolver, selected_summary_mode};
    use crate::paths::AppPaths;
    use crate::summary::SummaryMode;
    use clap::Parser;
    use color_eyre::Result;
    use std::fs;

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
