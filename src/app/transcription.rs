mod audio;
mod pipeline;

use color_eyre::Result;
use tracing::debug;

use crate::cache::CachedTranscript;
use crate::cli::{TranscriptArgs, TranscriptFormat, TranscriptMode};
use crate::input::resolve_media_input;
use crate::output::{commit_output, open_path, stage_named_output};
use crate::paths::{AppPaths, RunPaths};
use crate::transcript::{build_word_timings, render_word_lines, transcript_turns_json};

pub(crate) use pipeline::TranscriptionPipeline;

pub(super) fn run_transcript(
    app_paths: &AppPaths,
    force: bool,
    args: &TranscriptArgs,
    run_paths: Option<&RunPaths>,
) -> Result<()> {
    debug!("Starting transcript command for {}", args.input);
    let resolved_input = resolve_media_input(&args.input)?;
    let pipeline = TranscriptionPipeline::new(app_paths, force);
    let transcript = pipeline.transcribe_resolved_input(&resolved_input)?;
    let output = render_transcript_output(selected_transcript_output(args), &transcript)?;

    write_transcript_output(run_paths, args.open, &output)
}

fn write_transcript_output(
    run_paths: Option<&RunPaths>,
    open: bool,
    output: &RenderedTranscriptOutput,
) -> Result<()> {
    if let Some(run_paths) = run_paths {
        let final_path = run_paths.final_dir.join(output.file_name);
        let staged_path =
            stage_named_output(&run_paths.scratch_dir, output.file_name, &output.content)?;
        commit_output(&staged_path, &final_path)?;
        println!("{}", final_path.display());
        if open {
            open_path(&final_path)?;
        }
    } else {
        println!("{}", output.content);
    }
    Ok(())
}

fn selected_transcript_output(args: &TranscriptArgs) -> SelectedTranscriptOutput {
    let mode = args.mode.unwrap_or(TranscriptMode::Transcript);
    let format = args.format.unwrap_or(match mode {
        TranscriptMode::Transcript => TranscriptFormat::Text,
        TranscriptMode::Word => TranscriptFormat::Json,
    });
    SelectedTranscriptOutput { mode, format }
}

fn render_transcript_output(
    output: SelectedTranscriptOutput,
    transcript: &CachedTranscript,
) -> Result<RenderedTranscriptOutput> {
    match (output.mode, output.format) {
        (TranscriptMode::Transcript, TranscriptFormat::Text) => Ok(RenderedTranscriptOutput {
            file_name: "transcript.txt",
            content: transcript.transcript.clone(),
        }),
        (TranscriptMode::Transcript, TranscriptFormat::Json) => Ok(RenderedTranscriptOutput {
            file_name: "turns.json",
            content: serde_json::to_string_pretty(&transcript_turns_json(&transcript.turns))?,
        }),
        (TranscriptMode::Word, TranscriptFormat::Json) => Ok(RenderedTranscriptOutput {
            file_name: "words.json",
            content: serde_json::to_string_pretty(&build_word_timings(&transcript.tokens))?,
        }),
        (TranscriptMode::Word, TranscriptFormat::Text) => {
            let words = build_word_timings(&transcript.tokens);
            Ok(RenderedTranscriptOutput {
                file_name: "words.txt",
                content: render_word_lines(&words),
            })
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SelectedTranscriptOutput {
    mode: TranscriptMode,
    format: TranscriptFormat,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RenderedTranscriptOutput {
    file_name: &'static str,
    content: String,
}

#[cfg(test)]
mod tests {
    use super::{render_transcript_output, selected_transcript_output};
    use crate::cache::CachedTranscript;
    use crate::cli::{Command, TranscriptFormat, TranscriptMode};
    use crate::speakers::SpeakerTurn;
    use crate::transcript::TranscriptToken;
    use clap::Parser;

    fn sample_transcript() -> CachedTranscript {
        CachedTranscript {
            display_name: "track".to_owned(),
            source_key: "source-key".to_owned(),
            transcript_hash: "hash".to_owned(),
            transcript: "[00:00:01.000-00:00:02.000] Speaker 1: Hello".to_owned(),
            turns: vec![SpeakerTurn {
                start: 1.0,
                end: 2.0,
                speaker: "Speaker 1".to_owned(),
                text: "Hello".to_owned(),
            }],
            tokens: vec![TranscriptToken {
                text: " hello".to_owned(),
                start: 1.0,
                end: 1.3,
            }],
        }
    }

    #[test]
    fn transcript_defaults_to_text_mode() {
        let cli = crate::Cli::parse_from(["smrze", "transcript", "input.wav"]);
        let Command::Transcript(args) = cli.command else {
            panic!("expected transcript command");
        };
        let selected = selected_transcript_output(&args);
        assert_eq!(selected.mode, TranscriptMode::Transcript);
        assert_eq!(selected.format, TranscriptFormat::Text);
    }

    #[test]
    fn word_mode_defaults_to_json() {
        let cli = crate::Cli::parse_from(["smrze", "transcript", "input.wav", "--mode", "word"]);
        let Command::Transcript(args) = cli.command else {
            panic!("expected transcript command");
        };
        let selected = selected_transcript_output(&args);
        assert_eq!(selected.mode, TranscriptMode::Word);
        assert_eq!(selected.format, TranscriptFormat::Json);
    }

    #[test]
    fn transcript_json_uses_turns_filename() {
        let rendered = render_transcript_output(
            super::SelectedTranscriptOutput {
                mode: TranscriptMode::Transcript,
                format: TranscriptFormat::Json,
            },
            &sample_transcript(),
        )
        .expect("transcript json should render");
        assert_eq!(rendered.file_name, "turns.json");
        assert!(rendered.content.contains("\"speaker\": \"Speaker 1\""));
        assert!(rendered.content.contains("\"start_ms\": 1000"));
    }

    #[test]
    fn word_text_uses_timestamped_lines_file() {
        let rendered = render_transcript_output(
            super::SelectedTranscriptOutput {
                mode: TranscriptMode::Word,
                format: TranscriptFormat::Text,
            },
            &sample_transcript(),
        )
        .expect("word text should render");
        assert_eq!(rendered.file_name, "words.txt");
        assert_eq!(rendered.content, "[00:00:01.000-00:00:01.300] hello");
    }
}
