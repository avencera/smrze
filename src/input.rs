use color_eyre::{
    Result,
    eyre::{Context, eyre},
};
use duct::cmd;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::utils::{ensure_parent_dir, expand_path, file_stem_name, sanitize_name, short_hash};

#[derive(Debug, Clone)]
pub struct ResolvedInput {
    pub display_name: String,
    pub source_identity: String,
    pub media_path: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
struct DownloadManifest {
    display_name: String,
    media_file_name: String,
    #[serde(default)]
    normalized_audio_file_name: Option<String>,
}

pub fn resolve_input(input: &str, downloads_dir: &Path) -> Result<ResolvedInput> {
    if is_url(input) {
        return resolve_url_input(input, downloads_dir);
    }

    let path = expand_path(Path::new(input))?
        .canonicalize()
        .with_context(|| format!("failed to resolve {}", input))?;
    if !path.exists() {
        return Err(eyre!("input file not found: {}", path.display()));
    }

    Ok(ResolvedInput {
        display_name: sanitize_name(&file_stem_name(&path)?),
        source_identity: format!("file:{}", path.display()),
        media_path: path,
    })
}

fn resolve_url_input(input: &str, downloads_dir: &Path) -> Result<ResolvedInput> {
    ensure_command("yt-dlp")?;
    ensure_command("ffmpeg")?;

    let download_dir = downloads_dir.join(short_hash(input));
    fs::create_dir_all(&download_dir)
        .with_context(|| format!("failed to create {}", download_dir.display()))?;

    if let Some(resolved) = load_cached_download(input, &download_dir)? {
        return Ok(resolved);
    }

    let title = fetch_title(input).unwrap_or_else(|_| format!("input-{}", sanitize_name(input)));
    let template = download_dir.join("download.%(ext)s").display().to_string();

    cmd(
        "yt-dlp",
        ["-f", "bestaudio/best", "-o", template.as_str(), input],
    )
    .run()
    .with_context(|| "failed to launch yt-dlp")?;

    let media_path = find_downloaded_media(&download_dir)?;
    let normalized_audio_path = download_dir.join("audio.wav");
    normalize_downloaded_audio(&media_path, &normalized_audio_path)?;
    let display_name = sanitize_name(&title);
    save_download_manifest(
        &download_dir,
        &DownloadManifest {
            display_name: display_name.clone(),
            media_file_name: media_path
                .file_name()
                .and_then(|value| value.to_str())
                .ok_or_else(|| eyre!("downloaded media had no valid file name"))?
                .to_owned(),
            normalized_audio_file_name: Some("audio.wav".to_owned()),
        },
    )?;

    Ok(ResolvedInput {
        display_name,
        source_identity: format!("url:{input}"),
        media_path: normalized_audio_path,
    })
}

fn fetch_title(url: &str) -> Result<String> {
    let title = cmd("yt-dlp", ["--print", "title", "--skip-download", url])
        .read()
        .with_context(|| "failed to launch yt-dlp for title lookup")?
        .trim()
        .to_owned();
    if title.is_empty() {
        return Err(eyre!("yt-dlp returned an empty title"));
    }

    Ok(title)
}

fn ensure_command(command: &str) -> Result<()> {
    let output = cmd("which", [command])
        .stdout_null()
        .stderr_null()
        .unchecked()
        .run()
        .with_context(|| format!("failed to check for {command}"))?;
    if output.status.success() {
        return Ok(());
    }

    Err(eyre!("{command} is required but was not found in PATH"))
}

fn load_cached_download(input: &str, download_dir: &Path) -> Result<Option<ResolvedInput>> {
    let manifest_path = download_manifest_path(download_dir);
    if !manifest_path.exists() {
        return Ok(None);
    }

    let manifest_file = fs::File::open(&manifest_path)
        .with_context(|| format!("failed to open {}", manifest_path.display()))?;
    let manifest: DownloadManifest = serde_json::from_reader(manifest_file)
        .with_context(|| format!("failed to parse {}", manifest_path.display()))?;
    let media_path = download_dir.join(&manifest.media_file_name);
    if !media_path.exists() {
        return Ok(None);
    }

    let normalized_audio_path = manifest
        .normalized_audio_file_name
        .as_deref()
        .map(|value| download_dir.join(value))
        .unwrap_or_else(|| download_dir.join("audio.wav"));
    if !normalized_audio_path.exists() {
        normalize_downloaded_audio(&media_path, &normalized_audio_path)?;
        save_download_manifest(
            download_dir,
            &DownloadManifest {
                display_name: manifest.display_name.clone(),
                media_file_name: manifest.media_file_name.clone(),
                normalized_audio_file_name: Some("audio.wav".to_owned()),
            },
        )?;
    }

    Ok(Some(ResolvedInput {
        display_name: manifest.display_name,
        source_identity: format!("url:{input}"),
        media_path: normalized_audio_path,
    }))
}

fn find_downloaded_media(download_dir: &Path) -> Result<PathBuf> {
    for preferred in ["download.m4a", "download.mp4", "download.aac"] {
        let path = download_dir.join(preferred);
        if path.exists() {
            return Ok(path);
        }
    }

    let mp3_path = download_dir.join("download.mp3");
    if mp3_path.exists() {
        return Ok(mp3_path);
    }

    fs::read_dir(download_dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|value| value.to_str())
                .map(|value| value.starts_with("download."))
                .unwrap_or(false)
        })
        .ok_or_else(|| eyre!("yt-dlp reported success but no downloaded media was found"))
}

fn save_download_manifest(download_dir: &Path, manifest: &DownloadManifest) -> Result<()> {
    let manifest_path = download_manifest_path(download_dir);
    ensure_parent_dir(&manifest_path)?;
    let temp_path = manifest_path.with_extension("json.tmp");
    {
        let file = fs::File::create(&temp_path)
            .with_context(|| format!("failed to create {}", temp_path.display()))?;
        serde_json::to_writer_pretty(file, manifest)
            .with_context(|| format!("failed to write {}", temp_path.display()))?;
    }
    fs::rename(&temp_path, &manifest_path)
        .with_context(|| format!("failed to replace {}", manifest_path.display()))?;
    Ok(())
}

fn download_manifest_path(download_dir: &Path) -> PathBuf {
    download_dir.join("manifest.json")
}

fn normalize_downloaded_audio(media_path: &Path, output_path: &Path) -> Result<()> {
    if output_path.exists() {
        return Ok(());
    }

    let input = media_path.display().to_string();
    let output = output_path.display().to_string();
    let result = cmd(
        "ffmpeg",
        [
            "-y",
            "-i",
            input.as_str(),
            "-vn",
            "-ac",
            "1",
            "-ar",
            "16000",
            "-c:a",
            "pcm_s16le",
            output.as_str(),
        ],
    )
    .stdout_null()
    .stderr_capture()
    .unchecked()
    .run()
    .with_context(|| format!("failed to launch ffmpeg for {}", media_path.display()))?;

    if result.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&result.stderr);
    Err(eyre!(
        "ffmpeg failed to normalize {}: {}",
        media_path.display(),
        stderr.trim()
    ))
}

fn is_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}
