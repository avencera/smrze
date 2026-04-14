use color_eyre::{Result, eyre::eyre};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, warn};

use crate::audio::{convert_media_to_wav, decode_audio, normalize_audio};
use crate::cache::{
    AudioCacheEntry, CachedAudio, ensure_audio_cache_entry_dir, load_cached_audio, store_audio,
};
use crate::console;
use crate::input::{
    MediaInputKind, ResolvedMediaInput, ensure_command, fetch_title, find_downloaded_media,
};
use crate::paths::AppPaths;
use crate::utils::sanitize_name;

pub(super) struct AudioMaterializer<'a> {
    app_paths: &'a AppPaths,
    force: bool,
}

impl<'a> AudioMaterializer<'a> {
    pub(super) fn new(app_paths: &'a AppPaths, force: bool) -> Self {
        Self { app_paths, force }
    }

    pub(super) fn materialize(&self, resolved_input: &ResolvedMediaInput) -> Result<CachedAudio> {
        debug!(
            "Checking audio cache for source key {}",
            resolved_input.source_key
        );
        if let Some(cached_audio) =
            load_cached_audio(self.app_paths, &resolved_input.source_key, self.force)?
        {
            debug!(
                "Audio cache hit for source key {}",
                resolved_input.source_key
            );
            return Ok(cached_audio);
        }
        debug!(
            "Audio cache miss for source key {}",
            resolved_input.source_key
        );

        let entry_dir = ensure_audio_cache_entry_dir(self.app_paths, &resolved_input.source_key)?;
        let audio_path = entry_dir.join("audio.wav");
        let display_name = self.resolve_display_name(resolved_input);
        let media_file_name = self.write_audio_file(resolved_input, &entry_dir, &audio_path)?;

        store_audio(
            self.app_paths,
            AudioCacheEntry {
                source_key: &resolved_input.source_key,
                display_name: &display_name,
                audio_file_name: "audio.wav",
                media_file_name: media_file_name.as_deref(),
            },
        )?;

        Ok(CachedAudio {
            display_name,
            audio_path,
        })
    }

    fn resolve_display_name(&self, resolved_input: &ResolvedMediaInput) -> String {
        match &resolved_input.kind {
            MediaInputKind::Url { url } => fetch_title(url)
                .map(|title: String| sanitize_name(&title))
                .unwrap_or_else(|_| resolved_input.display_name.clone()),
            MediaInputKind::LocalFile { .. } => resolved_input.display_name.clone(),
        }
    }

    fn write_audio_file(
        &self,
        resolved_input: &ResolvedMediaInput,
        entry_dir: &Path,
        audio_path: &Path,
    ) -> Result<Option<String>> {
        match &resolved_input.kind {
            MediaInputKind::Url { url } => self.download_remote_audio(url, entry_dir, audio_path),
            MediaInputKind::LocalFile { path } => {
                debug!("Converting local media file {} to wav", path.display());
                ensure_command("ffmpeg")?;
                convert_media_to_wav(path, audio_path)?;
                Ok(None)
            }
        }
    }

    fn download_remote_audio(
        &self,
        url: &str,
        entry_dir: &Path,
        audio_path: &Path,
    ) -> Result<Option<String>> {
        debug!("Downloading remote media from {url}");
        let download_started = Instant::now();
        ensure_command("yt-dlp")?;
        ensure_command("ffmpeg")?;
        console::info(format!("Downloading media from {url}"));
        let template = entry_dir.join("download.%(ext)s").display().to_string();
        let mut args = vec!["-f", "bestaudio/best"];
        if console::is_quiet() {
            args.extend(["--quiet", "--no-warnings"]);
        }
        args.extend(["-o", template.as_str(), url]);

        let download = duct::cmd("yt-dlp", args).stdout_null();
        let download = if console::is_quiet() {
            download.stderr_null()
        } else {
            download
        };
        download
            .run()
            .map_err(|error| eyre!("failed to launch yt-dlp: {error}"))?;
        debug!(
            "Finished remote media download in {:.2}s for {url}",
            download_started.elapsed().as_secs_f64()
        );

        let media_path = find_downloaded_media(entry_dir)?;
        let media_file_name = media_path
            .file_name()
            .and_then(|value| value.to_str())
            .map(ToOwned::to_owned);
        let conversion_started = Instant::now();
        convert_media_to_wav(&media_path, audio_path)?;
        debug!(
            "Converted downloaded media to wav in {:.2}s",
            conversion_started.elapsed().as_secs_f64()
        );
        if let Err(error) = remove_downloaded_media(&media_path) {
            warn!(
                "Failed to remove downloaded media {}: {error:#}",
                media_path.display()
            );
        }
        Ok(media_file_name)
    }
}

fn remove_downloaded_media(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn load_normalized_audio(audio_path: &Path) -> Result<Arc<[f32]>> {
    let decode_started = Instant::now();
    console::info("Decoding audio");
    let decoded_audio = decode_audio(audio_path)?;
    let normalized_audio = normalize_audio(&decoded_audio);
    if normalized_audio.is_empty() {
        return Err(eyre!("decoded audio was empty"));
    }

    debug!(
        "Decoded and normalized audio in {:.2}s",
        decode_started.elapsed().as_secs_f64()
    );
    debug!("normalized {} samples", normalized_audio.len());
    Ok(Arc::<[f32]>::from(normalized_audio))
}
