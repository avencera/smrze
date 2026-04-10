use color_eyre::{
    Result,
    eyre::{Context, eyre},
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn stage_transcript(scratch_dir: &Path, transcript: &str) -> Result<PathBuf> {
    stage_text_file(scratch_dir, "transcript.txt", transcript)
}

pub fn stage_summary(scratch_dir: &Path, summary: &str) -> Result<PathBuf> {
    stage_text_file(scratch_dir, "summary.md", summary)
}

pub fn commit_summary(staged_path: &Path, final_path: &Path) -> Result<()> {
    commit_file(staged_path, final_path)
}

pub fn commit_transcript(staged_path: &Path, final_path: &Path) -> Result<()> {
    commit_file(staged_path, final_path)
}

fn stage_text_file(scratch_dir: &Path, file_name: &str, content: &str) -> Result<PathBuf> {
    let staging_dir = scratch_dir.join("final");
    fs::create_dir_all(&staging_dir)
        .with_context(|| format!("failed to create {}", staging_dir.display()))?;
    let staged_path = staging_dir.join(file_name);
    fs::write(&staged_path, format!("{content}\n"))
        .with_context(|| format!("failed to write {}", staged_path.display()))?;
    Ok(staged_path)
}

fn commit_file(staged_path: &Path, final_path: &Path) -> Result<()> {
    let final_dir = final_path
        .parent()
        .ok_or_else(|| eyre!("final path has no parent: {}", final_path.display()))?;
    fs::create_dir_all(final_dir)
        .with_context(|| format!("failed to create {}", final_dir.display()))?;
    fs::rename(staged_path, final_path)
        .with_context(|| format!("failed to replace {}", final_path.display()))?;
    Ok(())
}

pub fn open_path(path: &Path) -> Result<()> {
    let status = opener_command(path)
        .status()
        .with_context(|| format!("failed to launch opener for {}", path.display()))?;
    if status.success() {
        return Ok(());
    }

    Err(eyre!(
        "opener exited with status {status} for {}",
        path.display()
    ))
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

#[cfg(target_os = "macos")]
fn opener_command(path: &Path) -> Command {
    let mut command = Command::new("open");
    command.arg(path);
    command
}

#[cfg(target_os = "linux")]
fn opener_command(path: &Path) -> Command {
    let mut command = Command::new("xdg-open");
    command.arg(path);
    command
}

#[cfg(target_os = "windows")]
fn opener_command(path: &Path) -> Command {
    let mut command = Command::new("cmd");
    command.args(["/C", "start", ""]).arg(path);
    command
}
