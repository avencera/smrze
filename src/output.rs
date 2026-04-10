use color_eyre::{
    Result,
    eyre::{Context, eyre},
};
use std::fs;
use std::path::{Path, PathBuf};

pub fn stage_transcript(scratch_dir: &Path, transcript: &str) -> Result<PathBuf> {
    let staging_dir = scratch_dir.join("final");
    fs::create_dir_all(&staging_dir)
        .with_context(|| format!("failed to create {}", staging_dir.display()))?;
    let staged_path = staging_dir.join("transcript.txt");
    fs::write(&staged_path, format!("{transcript}\n"))
        .with_context(|| format!("failed to write {}", staged_path.display()))?;
    Ok(staged_path)
}

pub fn commit_transcript(staged_path: &Path, final_path: &Path) -> Result<()> {
    let final_dir = final_path
        .parent()
        .ok_or_else(|| eyre!("final path has no parent: {}", final_path.display()))?;
    fs::create_dir_all(final_dir)
        .with_context(|| format!("failed to create {}", final_dir.display()))?;
    fs::rename(staged_path, final_path)
        .with_context(|| format!("failed to replace {}", final_path.display()))?;
    Ok(())
}

pub fn remove_path_if_exists(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }

    if path.is_dir() {
        fs::remove_dir_all(path).with_context(|| format!("failed to remove {}", path.display()))?;
    } else {
        fs::remove_file(path).with_context(|| format!("failed to remove {}", path.display()))?;
    }
    Ok(())
}
