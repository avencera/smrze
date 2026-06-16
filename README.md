# smrze

`smrze` is a work-in-progress CLI for creating local-only transcripts and summaries from media or transcript files.

The project is still early. Commands, setup, output shape, and model requirements may change as the tool is developed.

## What it can do

- Generate diarized transcripts from local media files
- Download remote media URLs with `yt-dlp` and process them locally
- Convert and decode media with `ffmpeg`
- Run transcription and speaker diarization locally
- Print results to stdout or write files into an output directory
- Produce transcript output as plain text, diarized text, speaker turn JSON, word timing JSON, or timestamped word text
- Summarize existing transcript files, media files, or remote media URLs
- Use Apple Foundation Models for summaries by default
- Fall back to Gemma 4 E2B when Apple Foundation Models refuse a transcript
- Select Gemma 4 E2B or Gemma 4 E4B explicitly for summaries
- Cache downloaded audio, transcripts, summaries, models, and MLX runtime assets under the app cache directory

## Examples

Generate a transcript from a local media file:

```sh
smrze transcript ./meeting.mp4
```

Generate a transcript from a URL and write `transcript.txt` into an output directory:

```sh
smrze transcript "https://example.com/video" --output ./out
```

Generate a plain transcript without timestamps or speaker diarization:

```sh
smrze transcript ./meeting.wav --no-timestamps
```

Write word-level timing output as JSON:

```sh
smrze transcript ./meeting.wav --mode word --format json
```

Summarize an existing transcript:

```sh
smrze summarize ./transcript.txt
```

Summarize a URL with an explicit Gemma backend:

```sh
smrze summarize "https://example.com/video" --summary-backend gemma4-e2b
```

Use short aliases:

```sh
smrze t ./meeting.mp4
smrze s ./transcript.txt
```

## Output

Without `--output`, commands print their result to stdout.

With `--output DIR`, `smrze transcript` writes one of:

- `transcript.txt`
- `turns.json`
- `words.json`
- `words.txt`

With `--output DIR`, `smrze summarize` writes:

- `summary.md`

Use `--open` with `--output` to open the written file after creation.

## Requirements

- Rust toolchain
- `ffmpeg` for media conversion and audio fallback decoding
- `yt-dlp` for remote URL inputs
- macOS with Xcode for Apple Foundation Models and MLX-backed summaries
- SwiftPM network access on the first macOS build so it can fetch `mlx-swift`
- Metal Toolchain for MLX Gemma support

Install the Metal Toolchain before using MLX-backed Gemma summaries:

```sh
xcodebuild -downloadComponent MetalToolchain
```

## Building

```sh
cargo build
```

## Development

```sh
just fmt
just clippy
just test
```
