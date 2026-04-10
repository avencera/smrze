use std::path::PathBuf;

use clap::{Parser, builder::Styles};

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

/// Command line arguments for the smrze CLI
#[derive(Debug, Parser)]
#[command(
    name = "smrze",
    author,
    version,
    long_version = env!("CARGO_PKG_VERSION"),
    arg_required_else_help = true,
    about = "Create a local-only diarized transcript from a media file or URL",
    long_about = "smrze creates a local-only diarized transcript from a YouTube video, direct media URL, or local audio/video file, and can optionally add a local Apple foundation model summary on macOS. By default it prints results to stdout.",
    after_help = "Examples:\n  smrze https://www.youtube.com/watch?v=jNQXAC9IVRw\n  smrze ./meeting.m4a --summary\n  smrze ./call.mp4 -o ~/transcripts/call",
    styles = get_styles()
)]
pub struct Args {
    /// Local media file or remote URL
    pub input: String,
    /// Directory where transcript.txt and summary.md should be written instead of stdout
    #[arg(short, long, value_name = "DIR")]
    pub output: Option<PathBuf>,
    /// Generate summary.md using the local Apple foundation model
    #[arg(long)]
    pub summary: bool,
    /// Open the written output after it is created, requires --output
    #[arg(long)]
    pub open: bool,
}
