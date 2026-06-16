use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum, builder::Styles};

use crate::summary_backend::SummaryBackend;

pub fn get_styles() -> Styles {
    Styles::styled()
        .usage(
            anstyle::Style::new()
                .bold()
                .underline()
                .fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Yellow))),
        )
        .header(
            anstyle::Style::new()
                .bold()
                .underline()
                .fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Yellow))),
        )
        .literal(
            anstyle::Style::new().fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Green))),
        )
        .invalid(
            anstyle::Style::new()
                .bold()
                .fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Red))),
        )
        .error(
            anstyle::Style::new()
                .bold()
                .fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Red))),
        )
        .valid(
            anstyle::Style::new()
                .bold()
                .underline()
                .fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Green))),
        )
        .placeholder(
            anstyle::Style::new().fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::White))),
        )
}

#[derive(Debug, Parser)]
#[command(
    name = "smrze",
    author,
    version,
    long_version = env!("CARGO_PKG_VERSION"),
    about = "Create local-only transcripts and summaries from media or transcript files",
    arg_required_else_help = true,
    styles = get_styles()
)]
pub struct Cli {
    /// Suppress non-error logs and downloader progress output
    #[arg(short, long, global = true)]
    pub quiet: bool,
    /// Recompute artifacts instead of reading from the artifact cache
    #[arg(long, global = true)]
    pub force: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Generate a diarized transcript from a media file or URL
    #[command(visible_aliases = ["t", "trans"])]
    Transcript(TranscriptArgs),
    /// Generate a summary from a transcript file, media file, or URL
    #[command(visible_aliases = ["s", "sum"])]
    Summarize(SummarizeArgs),
}

#[derive(Debug, Clone, Args)]
pub struct TranscriptArgs {
    /// Local media file or remote URL
    pub input: String,
    /// Directory where transcript.txt should be written instead of stdout
    #[arg(short, long, value_name = "DIR")]
    pub output: Option<PathBuf>,
    /// Open the written transcript after it is created, requires --output
    #[arg(long)]
    pub open: bool,
    /// Transcription mode used by the ASR pipeline
    #[arg(short = 'm', long = "transcription-mode", value_enum)]
    pub transcription_mode: Option<TranscriptionMode>,
    /// Print only transcript text without timestamps or speaker diarization
    #[arg(long, conflicts_with_all = ["mode", "format"])]
    pub no_timestamps: bool,
    /// Transcript output mode
    #[arg(long, value_enum)]
    pub mode: Option<TranscriptMode>,
    /// Transcript output format
    #[arg(long, value_enum)]
    pub format: Option<TranscriptFormat>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum TranscriptMode {
    #[value(alias = "turns")]
    Transcript,
    #[value(alias = "words")]
    Word,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum TranscriptFormat {
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum TranscriptionMode {
    Fast,
    Vad,
}

impl TranscriptionMode {
    pub const fn cache_key(self) -> &'static str {
        match self {
            Self::Fast => "fast",
            Self::Vad => "vad",
        }
    }
}

#[derive(Debug, Clone, Args)]
pub struct SummarizeArgs {
    /// Local transcript or media file, or a remote media URL
    pub input: String,
    /// Directory where summary.md should be written instead of stdout
    #[arg(short, long, value_name = "DIR")]
    pub output: Option<PathBuf>,
    /// Summary backend to use, defaults to Apple Foundation with Gemma fallback on refusal
    #[arg(short = 'b', long, value_enum)]
    pub summary_backend: Option<SummaryBackend>,
    /// Directory containing local Gemma 4 MLX model directories when using a Gemma backend
    #[arg(short = 'm', long, value_name = "DIR")]
    pub summary_model_dir: Option<PathBuf>,
    /// Open the written summary after it is created, requires --output
    #[arg(long)]
    pub open: bool,
}

#[cfg(test)]
mod tests {
    use super::{Cli, Command, TranscriptFormat, TranscriptMode, TranscriptionMode};
    use clap::{CommandFactory, Parser};

    #[test]
    fn transcript_alias_parses() {
        let cli = Cli::parse_from(["smrze", "t", "input.wav"]);
        assert!(matches!(cli.command, Command::Transcript(_)));
    }

    #[test]
    fn summarize_alias_parses() {
        let cli = Cli::parse_from(["smrze", "s", "transcript.txt"]);
        assert!(matches!(cli.command, Command::Summarize(_)));
    }

    #[test]
    fn extended_aliases_parse() {
        let cli = Cli::parse_from(["smrze", "trans", "input.wav"]);
        assert!(matches!(cli.command, Command::Transcript(_)));

        let cli = Cli::parse_from(["smrze", "sum", "transcript.txt"]);
        assert!(matches!(cli.command, Command::Summarize(_)));
    }

    #[test]
    fn global_flags_parse_before_subcommand() {
        let cli = Cli::parse_from(["smrze", "--quiet", "--force", "transcript", "input.wav"]);
        assert!(cli.quiet);
        assert!(cli.force);
    }

    #[test]
    fn help_shows_visible_aliases() {
        let help = Cli::command().render_help().to_string();
        assert!(help.contains("transcript"));
        assert!(help.contains("[aliases: t, trans]"));
        assert!(help.contains("summarize"));
        assert!(help.contains("[aliases: s, sum]"));
    }

    #[test]
    fn transcript_mode_and_format_parse() {
        let cli = Cli::parse_from([
            "smrze",
            "transcript",
            "input.wav",
            "-m",
            "fast",
            "--mode",
            "word",
            "--format",
            "json",
        ]);
        let Command::Transcript(args) = cli.command else {
            panic!("expected transcript command");
        };
        assert_eq!(args.transcription_mode, Some(TranscriptionMode::Fast));
        assert_eq!(args.mode, Some(TranscriptMode::Word));
        assert_eq!(args.format, Some(TranscriptFormat::Json));
    }

    #[test]
    fn no_timestamps_parses() {
        let cli = Cli::parse_from(["smrze", "transcript", "input.wav", "--no-timestamps"]);
        let Command::Transcript(args) = cli.command else {
            panic!("expected transcript command");
        };
        assert!(args.no_timestamps);
    }

    #[test]
    fn no_timestamps_conflicts_with_structured_output_options() {
        let error = Cli::try_parse_from([
            "smrze",
            "transcript",
            "input.wav",
            "--no-timestamps",
            "--mode",
            "word",
        ])
        .expect_err("--no-timestamps should conflict with --mode");
        assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);

        let error = Cli::try_parse_from([
            "smrze",
            "transcript",
            "input.wav",
            "--no-timestamps",
            "--format",
            "json",
        ])
        .expect_err("--no-timestamps should conflict with --format");
        assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn transcript_mode_aliases_parse() {
        let cli = Cli::parse_from(["smrze", "transcript", "input.wav", "--mode", "words"]);
        let Command::Transcript(args) = cli.command else {
            panic!("expected transcript command");
        };
        assert_eq!(args.mode, Some(TranscriptMode::Word));
    }

    #[test]
    fn transcription_mode_short_flag_parses() {
        let cli = Cli::parse_from(["smrze", "transcript", "input.wav", "-m", "vad"]);
        let Command::Transcript(args) = cli.command else {
            panic!("expected transcript command");
        };
        assert_eq!(args.transcription_mode, Some(TranscriptionMode::Vad));
    }
}
