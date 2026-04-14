mod download;
mod local;
mod remote;

use color_eyre::Result;
use std::path::PathBuf;
use tracing::debug;

use crate::utils::{file_stem_name, sanitize_name};

pub(crate) use download::{ensure_command, find_downloaded_media};
pub use local::local_file_source_key;
pub(crate) use local::resolve_existing_path;
pub(crate) use remote::fetch_title;
pub use remote::is_url;

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

pub fn resolve_media_input(input: &str) -> Result<ResolvedMediaInput> {
    if is_url(input) {
        debug!("Computing remote cache key for {input}");
        let source_key = remote::normalize_url_source_key(input)?;
        return Ok(ResolvedMediaInput {
            display_name: fallback_remote_display_name(&source_key),
            source_key,
            kind: MediaInputKind::Url {
                url: input.to_owned(),
            },
        });
    }

    debug!("Resolving local media input path {input}");
    let path = resolve_existing_path(input)?;
    Ok(ResolvedMediaInput {
        display_name: sanitize_name(&file_stem_name(&path)?),
        source_key: local_file_source_key(&path)?,
        kind: MediaInputKind::LocalFile { path },
    })
}

fn fallback_remote_display_name(source_key: &str) -> String {
    sanitize_name(&source_key.replace('/', "-"))
}

#[cfg(test)]
mod tests {
    use super::{fallback_remote_display_name, local_file_source_key, remote::youtube_video_id};
    use crate::input::remote::normalize_url_source_key;
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

    #[test]
    fn remote_display_name_uses_source_key_without_title_lookup() {
        assert_eq!(
            fallback_remote_display_name("youtube/WbLPQ7XRpLs"),
            "youtube-wblpq7xrpls"
        );
    }
}
