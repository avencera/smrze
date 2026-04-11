use color_eyre::{
    Result,
    eyre::{Context, eyre},
};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const SAMPLE_RATE: u32 = 16_000;

pub fn now_millis() -> Result<u128> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| eyre!("system clock before unix epoch: {error}"))?
        .as_millis())
}

pub fn now_millis_u64() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| eyre!("system clock before unix epoch: {error}"))?
        .as_millis()
        .try_into()
        .map_err(|_| eyre!("system time does not fit into u64"))
}

pub fn expand_path(path: &Path) -> Result<PathBuf> {
    let expanded = shellexpand::tilde(&path.to_string_lossy()).into_owned();
    Ok(PathBuf::from(expanded))
}

pub fn sanitize_name(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            ' ' => '-',
            _ if character.is_control() => '-',
            _ => character,
        })
        .collect::<String>()
        .to_lowercase();
    let trimmed = sanitized.trim_matches(['-', '_', '.']);
    if trimmed.is_empty() {
        return "input".to_owned();
    }

    trimmed.chars().take(80).collect()
}

pub fn file_stem_name(path: &Path) -> Result<String> {
    path.file_stem()
        .and_then(|value| value.to_str())
        .map(ToOwned::to_owned)
        .ok_or_else(|| eyre!("path has no valid file stem: {}", path.display()))
}

pub fn ensure_parent_dir(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| eyre!("path has no parent: {}", path.display()))?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("failed to create {}", parent.display()))?;
    Ok(())
}

pub fn short_hash(input: &str) -> String {
    hash_string(input).chars().take(8).collect()
}

pub fn hash_string(input: &str) -> String {
    blake3::hash(input.as_bytes()).to_hex().to_string()
}
