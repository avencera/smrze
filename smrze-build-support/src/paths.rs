use std::env;
use std::path::{Path, PathBuf};

use crate::error::{BuildSupportError, Result};

pub fn xcode_arch_for_target(target_arch: &str) -> Result<&'static str> {
    match target_arch {
        "aarch64" => Ok("arm64"),
        "x86_64" => Ok("x86_64"),
        other => Err(BuildSupportError::new(format!(
            "unsupported macOS arch for xcodebuild: {other}"
        ))),
    }
}

pub fn current_xcode_arch() -> Result<&'static str> {
    xcode_arch_for_target(env::consts::ARCH)
}

pub fn swift_triple_dir_for_target(target_arch: &str) -> Result<&'static str> {
    match target_arch {
        "aarch64" => Ok("aarch64-apple-macosx"),
        "x86_64" => Ok("x86_64-apple-macosx"),
        other => Err(BuildSupportError::new(format!(
            "unsupported macOS arch for swift bridge: {other}"
        ))),
    }
}

pub fn current_runtime_arch_dir() -> Result<&'static str> {
    match env::consts::ARCH {
        "aarch64" => Ok("macos-arm64"),
        "x86_64" => Ok("macos-x86_64"),
        arch => Err(BuildSupportError::new(format!(
            "unsupported macOS architecture for runtime assets: {arch}"
        ))),
    }
}

pub fn cargo_profile_dir(out_dir: &Path) -> Result<PathBuf> {
    out_dir
        .ancestors()
        .nth(3)
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            BuildSupportError::new("failed to resolve Cargo profile directory from OUT_DIR")
        })
}
