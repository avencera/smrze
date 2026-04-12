use color_eyre::{
    Result,
    eyre::{Context, eyre},
};
use duct::cmd;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn ensure_command(command: &str) -> Result<()> {
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

pub(crate) fn find_downloaded_media(download_dir: &Path) -> Result<PathBuf> {
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
