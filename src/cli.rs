use std::path::PathBuf;

use clap::Parser;

/// Command line arguments for the smrze CLI
#[derive(Debug, Parser)]
#[command(
    name = "smrze",
    author,
    version,
    long_version = env!("CARGO_PKG_VERSION"),
    arg_required_else_help = true,
    about = "Create a local-only diarized transcript from a media file or URL",
    long_about = "smrze creates a local-only diarized transcript from a YouTube video, direct media URL, or local audio/video file",
    after_help = "Examples:\n  smrze https://www.youtube.com/watch?v=jNQXAC9IVRw\n  smrze ./meeting.m4a\n  smrze ./call.mp4 --output-dir ~/transcripts/call"
)]
pub struct Args {
    /// Local media file or remote URL
    pub input: String,
    /// Directory where transcript.txt should be written
    #[arg(long)]
    pub output_dir: Option<PathBuf>,
}
