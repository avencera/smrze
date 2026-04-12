use color_eyre::{
    Result,
    eyre::{Context, eyre},
};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::utils::expand_path;

pub(crate) fn resolve_existing_path(input: &str) -> Result<PathBuf> {
    let path = expand_path(Path::new(input))?;
    if !path.exists() {
        return Err(eyre!("input file not found: {}", path.display()));
    }

    path.canonicalize()
        .with_context(|| format!("failed to resolve {input}"))
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
