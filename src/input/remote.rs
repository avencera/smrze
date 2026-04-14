use color_eyre::{
    Result,
    eyre::{Context, eyre},
};
use duct::cmd;
use std::time::Instant;
use tracing::debug;
use url::Url;

use crate::console;

pub fn is_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

pub(crate) fn fetch_title(url: &str) -> Result<String> {
    debug!("Looking up media title with yt-dlp for {url}");
    let started = Instant::now();
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
    debug!(
        "Finished media title lookup in {:.2}s for {url}",
        started.elapsed().as_secs_f64()
    );
    if title.is_empty() {
        return Err(eyre!("yt-dlp returned an empty title"));
    }

    Ok(title)
}

pub(crate) fn normalize_url_source_key(input: &str) -> Result<String> {
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

pub(crate) fn youtube_video_id(url: &Url) -> Option<String> {
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
