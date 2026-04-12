use color_eyre::{Result, eyre::eyre};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use tracing::debug;

use crate::audio::{convert_media_to_wav, decode_audio, normalize_audio};
use crate::cache::{
    AudioCacheEntry, CacheKind, CachedAudio, ensure_cache_entry_dir, load_audio, load_cache_entry,
    store_audio,
};
use crate::console;
use crate::input::{MediaInputKind, ResolvedMediaInput, ensure_command, find_downloaded_media};
use crate::paths::AppPaths;

pub(super) struct AudioMaterializer<'a> {
    app_paths: &'a AppPaths,
    force: bool,
}

impl<'a> AudioMaterializer<'a> {
    pub(super) fn new(app_paths: &'a AppPaths, force: bool) -> Self {
        Self { app_paths, force }
    }

    pub(super) fn materialize(&self, resolved_input: &ResolvedMediaInput) -> Result<CachedAudio> {
        if let Some(cached_audio) = load_cache_entry(
            self.app_paths,
            CacheKind::Audio,
            &resolved_input.source_key,
            self.force,
            load_audio,
        )? {
            return Ok(cached_audio);
        }

        let entry_dir =
            ensure_cache_entry_dir(self.app_paths, CacheKind::Audio, &resolved_input.source_key)?;
        let audio_path = entry_dir.join("audio.wav");
        let media_file_name = self.write_audio_file(resolved_input, &entry_dir, &audio_path)?;

        store_audio(
            self.app_paths,
            AudioCacheEntry {
                source_key: &resolved_input.source_key,
                display_name: &resolved_input.display_name,
                audio_file_name: "audio.wav",
                media_file_name: media_file_name.as_deref(),
            },
        )?;

        Ok(CachedAudio {
            display_name: resolved_input.display_name.clone(),
            audio_path,
        })
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
        ensure_command("yt-dlp")?;
        ensure_command("ffmpeg")?;
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

        let media_path = find_downloaded_media(entry_dir)?;
        let media_file_name = media_path
            .file_name()
            .and_then(|value| value.to_str())
            .map(ToOwned::to_owned);
        convert_media_to_wav(&media_path, audio_path)?;
        Ok(media_file_name)
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
