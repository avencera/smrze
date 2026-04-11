use color_eyre::{
    Result,
    eyre::{Context, eyre},
};
use duct::cmd;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use url::Url;

use crate::cache::{
    CacheKind, cache_file_path, clear_cache_entry, ensure_cache_entry_dir, load_manifest,
    write_manifest,
};
use crate::console;
use crate::paths::AppPaths;
use crate::utils::{expand_path, file_stem_name, sanitize_name};

#[derive(Debug, Clone)]
pub struct CachedAudio {
    pub display_name: String,
    pub audio_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ResolvedMediaInput {
    pub display_name: String,
    pub source_key: String,
    pub kind: MediaInputKind,
}

#[derive(Debug, Clone)]
pub enum MediaInputKind {
    Url { url: String },
    LocalFile { path: PathBuf },
}

#[derive(Debug, Serialize, Deserialize)]
struct AudioManifest {
    created_at_ms: u64,
    source_key: String,
    display_name: String,
    audio_file_name: String,
    #[serde(default)]
    media_file_name: Option<String>,
}

pub fn resolve_media_input(input: &str) -> Result<ResolvedMediaInput> {
    if is_url(input) {
        let source_key = normalize_url_source_key(input)?;
        return Ok(ResolvedMediaInput {
            display_name: sanitize_name(
                &fetch_title(input).unwrap_or_else(|_| format!("input-{source_key}")),
            ),
            source_key,
            kind: MediaInputKind::Url {
                url: input.to_owned(),
            },
        });
    }

    let path = expand_path(Path::new(input))?
        .canonicalize()
        .with_context(|| format!("failed to resolve {input}"))?;
    if !path.exists() {
        return Err(eyre!("input file not found: {}", path.display()));
    }

    Ok(ResolvedMediaInput {
        display_name: sanitize_name(&file_stem_name(&path)?),
        source_key: local_file_source_key(&path)?,
        kind: MediaInputKind::LocalFile { path },
    })
}

pub fn materialize_audio(
    app_paths: &AppPaths,
    resolved_input: &ResolvedMediaInput,
    force: bool,
) -> Result<CachedAudio> {
    if force {
        clear_cache_entry(app_paths, CacheKind::Audio, &resolved_input.source_key)?;
    } else if let Some(manifest) =
        load_manifest::<AudioManifest>(app_paths, CacheKind::Audio, &resolved_input.source_key)?
    {
        let audio_path = cache_file_path(
            app_paths,
            CacheKind::Audio,
            &resolved_input.source_key,
            &manifest.audio_file_name,
        );
        if audio_path.exists() {
            return Ok(CachedAudio {
                display_name: manifest.display_name,
                audio_path,
            });
        }
    }
    clear_cache_entry(app_paths, CacheKind::Audio, &resolved_input.source_key)?;

    let entry_dir =
        ensure_cache_entry_dir(app_paths, CacheKind::Audio, &resolved_input.source_key)?;
    let audio_path = entry_dir.join("audio.wav");
    let mut media_file_name = None;

    match &resolved_input.kind {
        MediaInputKind::Url { url } => {
            ensure_command("yt-dlp")?;
            ensure_command("ffmpeg")?;
            let template = entry_dir.join("download.%(ext)s").display().to_string();
            let mut args = vec!["-f", "bestaudio/best"];
            if console::is_quiet() {
                args.extend(["--quiet", "--no-warnings"]);
            }
            args.extend(["-o", template.as_str(), url]);

            let download = cmd("yt-dlp", args).stdout_null();
            let download = if console::is_quiet() {
                download.stderr_null()
            } else {
                download
            };
            download.run().with_context(|| "failed to launch yt-dlp")?;

            let media_path = find_downloaded_media(&entry_dir)?;
            media_file_name = media_path
                .file_name()
                .and_then(|value| value.to_str())
                .map(ToOwned::to_owned);
            convert_to_cached_audio(&media_path, &audio_path)?;
        }
        MediaInputKind::LocalFile { path } => {
            ensure_command("ffmpeg")?;
            convert_to_cached_audio(path, &audio_path)?;
        }
    }

    write_manifest(
        &entry_dir.join("manifest.json"),
        &AudioManifest {
            created_at_ms: now_millis_u64()?,
            source_key: resolved_input.source_key.clone(),
            display_name: resolved_input.display_name.clone(),
            audio_file_name: "audio.wav".to_owned(),
            media_file_name,
        },
    )?;

    Ok(CachedAudio {
        display_name: resolved_input.display_name.clone(),
        audio_path,
    })
}

pub fn is_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

pub fn local_file_source_key(path: &Path) -> Result<String> {
    let metadata = fs::metadata(path)
        .with_context(|| format!("failed to read metadata for {}", path.display()))?;
    let modified_ms = metadata
        .modified()
        .with_context(|| format!("failed to read modified time for {}", path.display()))?
        .duration_since(UNIX_EPOCH)
        .map_err(|error| eyre!("{} has an invalid modified time: {error}", path.display()))?
        .as_millis();
    Ok(format!(
        "file:{}:{}:{}",
        path.display(),
        metadata.len(),
        modified_ms
    ))
}

fn fetch_title(url: &str) -> Result<String> {
    let mut args = vec!["--print", "title", "--skip-download"];
    if console::is_quiet() {
        args.extend(["--quiet", "--no-warnings"]);
    }
    args.push(url);

    let title_lookup = cmd("yt-dlp", args);
    let title_lookup = if console::is_quiet() {
        title_lookup.stderr_null()
    } else {
        title_lookup
    };
    let title = title_lookup
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

fn normalize_url_source_key(input: &str) -> Result<String> {
    let url = Url::parse(input).with_context(|| format!("failed to parse URL {input}"))?;
    if let Some(video_id) = youtube_video_id(&url) {
        return Ok(format!("youtube/{video_id}"));
    }

    let host = url
        .host_str()
        .ok_or_else(|| eyre!("URL is missing a host: {input}"))?
        .to_ascii_lowercase();
    let path = if url.path().is_empty() {
        "/"
    } else {
        url.path()
    };
    let port = match (url.port(), default_port(url.scheme())) {
        (Some(port), Some(default_port)) if port != default_port => format!(":{port}"),
        (Some(port), None) => format!(":{port}"),
        _ => String::new(),
    };

    Ok(format!("{host}{port}{path}"))
}

fn youtube_video_id(url: &Url) -> Option<String> {
    let host = url.host_str()?.to_ascii_lowercase();
    if host == "youtu.be" {
        return url
            .path_segments()?
            .find(|segment| !segment.is_empty())
            .map(ToOwned::to_owned);
    }

    if !host.ends_with("youtube.com") {
        return None;
    }

    let mut segments = url.path_segments()?;
    match segments.next()? {
        "watch" => url
            .query_pairs()
            .find_map(|(key, value)| (key == "v").then(|| value.into_owned())),
        "shorts" | "embed" => segments
            .find(|segment| !segment.is_empty())
            .map(ToOwned::to_owned),
        _ => None,
    }
}

fn default_port(scheme: &str) -> Option<u16> {
    match scheme {
        "http" => Some(80),
        "https" => Some(443),
        _ => None,
    }
}

fn find_downloaded_media(download_dir: &Path) -> Result<PathBuf> {
    for preferred in [
        "download.m4a",
        "download.mp4",
        "download.aac",
        "download.mp3",
    ] {
        let path = download_dir.join(preferred);
        if path.exists() {
            return Ok(path);
        }
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

fn convert_to_cached_audio(media_path: &Path, output_path: &Path) -> Result<()> {
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

fn now_millis_u64() -> Result<u64> {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| eyre!("system clock before unix epoch: {error}"))?
        .as_millis()
        .try_into()
        .map_err(|_| eyre!("system time does not fit into u64"))
}

#[cfg(test)]
mod tests {
    use super::{local_file_source_key, normalize_url_source_key, youtube_video_id};
    use color_eyre::Result;
    use std::fs;
    use std::path::PathBuf;
    use url::Url;

    #[test]
    fn normalizes_generic_url_keys() -> Result<()> {
        assert_eq!(
            normalize_url_source_key("https://Example.com/path/to/file?x=1#frag")?,
            "example.com/path/to/file"
        );
        Ok(())
    }

    #[test]
    fn extracts_youtube_video_ids() -> Result<()> {
        let url = Url::parse("https://www.youtube.com/watch?v=jNQXAC9IVRw")?;
        assert_eq!(youtube_video_id(&url).as_deref(), Some("jNQXAC9IVRw"));
        Ok(())
    }

    #[test]
    fn local_file_key_changes_with_metadata() -> Result<()> {
        let root = std::env::temp_dir().join("smrze-local-file-key");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root)?;
        let path = root.join("input.txt");
        fs::write(&path, "first")?;
        let first_key = local_file_source_key(&path)?;
        std::thread::sleep(std::time::Duration::from_millis(5));
        fs::write(&path, "second")?;
        let second_key = local_file_source_key(&path)?;
        assert_ne!(first_key, second_key);
        let _ = fs::remove_dir_all(PathBuf::from(&root));
        Ok(())
    }
}
