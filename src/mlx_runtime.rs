use color_eyre::{Result, eyre::Context};
use hf_hub::{Cache, api::sync::ApiBuilder};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::paths::AppPaths;
use crate::utils::ensure_parent_dir;

const HF_REPO: &str = "avencera/smrze-runtime-assets";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MlxRuntimeError {
    UnsupportedArch { message: String },
    DownloadFailure { message: String },
    IntegrityFailure { message: String },
    InstallFailure { message: String },
}

#[derive(Debug, Clone)]
pub struct MlxMetallibAsset {
    install_root: PathBuf,
    huggingface_cache_dir: PathBuf,
    asset_version: &'static str,
    asset_sha256: &'static str,
    arch_dir: &'static str,
}

impl MlxMetallibAsset {
    pub fn from_app_paths(app_paths: &AppPaths) -> Result<Self, MlxRuntimeError> {
        Ok(Self {
            install_root: app_paths.mlx_runtime_cache(),
            huggingface_cache_dir: app_paths.huggingface_cache(),
            asset_version: env!("SMRZE_MLX_RUNTIME_ASSET_VERSION"),
            asset_sha256: env!("SMRZE_MLX_RUNTIME_ASSET_SHA256"),
            arch_dir: current_arch_dir()?,
        })
    }

    pub fn ensure_available(&self) -> Result<PathBuf, MlxRuntimeError> {
        let installed_path = self.installed_path();
        if installed_path.exists() && self.matches_expected_digest(&installed_path)? {
            return Ok(installed_path);
        }

        let downloaded_path = self.download()?;
        self.verify_digest(&downloaded_path)?;
        self.install(&downloaded_path, &installed_path)?;
        Ok(installed_path)
    }

    fn remote_path(&self) -> String {
        format!("mlx/{}/{}/mlx.metallib", self.asset_version, self.arch_dir)
    }

    fn installed_path(&self) -> PathBuf {
        self.install_root
            .join(self.asset_version)
            .join(self.arch_dir)
            .join("mlx.metallib")
    }

    fn download(&self) -> Result<PathBuf, MlxRuntimeError> {
        let api = ApiBuilder::from_cache(Cache::new(self.huggingface_cache_dir.clone()))
            .build()
            .map_err(|error| MlxRuntimeError::DownloadFailure {
                message: error.to_string(),
            })?;
        let repo = api.model(HF_REPO.to_owned());
        repo.get(&self.remote_path())
            .map_err(|error| MlxRuntimeError::DownloadFailure {
                message: error.to_string(),
            })
    }

    fn install(&self, source_path: &Path, target_path: &Path) -> Result<(), MlxRuntimeError> {
        ensure_parent_dir(target_path).map_err(|error| MlxRuntimeError::InstallFailure {
            message: error.to_string(),
        })?;

        let temp_path = target_path.with_extension("tmp");
        let _ = fs::remove_file(&temp_path);
        fs::copy(source_path, &temp_path).map_err(|error| MlxRuntimeError::InstallFailure {
            message: format!(
                "failed to copy {} to {}: {error}",
                source_path.display(),
                temp_path.display()
            ),
        })?;
        self.verify_digest(&temp_path)?;

        let _ = fs::remove_file(target_path);
        fs::rename(&temp_path, target_path).map_err(|error| MlxRuntimeError::InstallFailure {
            message: format!(
                "failed to install {} at {}: {error}",
                source_path.display(),
                target_path.display()
            ),
        })
    }

    fn matches_expected_digest(&self, path: &Path) -> Result<bool, MlxRuntimeError> {
        let actual_digest = file_sha256(path).map_err(|error| MlxRuntimeError::InstallFailure {
            message: error.to_string(),
        })?;
        Ok(actual_digest == self.asset_sha256)
    }

    fn verify_digest(&self, path: &Path) -> Result<(), MlxRuntimeError> {
        let actual_digest = file_sha256(path).map_err(|error| MlxRuntimeError::InstallFailure {
            message: error.to_string(),
        })?;
        if actual_digest == self.asset_sha256 {
            return Ok(());
        }

        Err(MlxRuntimeError::IntegrityFailure {
            message: format!(
                "expected SHA-256 {} for {}, found {}",
                self.asset_sha256,
                path.display(),
                actual_digest
            ),
        })
    }
}

fn current_arch_dir() -> Result<&'static str, MlxRuntimeError> {
    match std::env::consts::ARCH {
        "aarch64" => Ok("macos-arm64"),
        "x86_64" => Ok("macos-x86_64"),
        arch => Err(MlxRuntimeError::UnsupportedArch {
            message: format!("unsupported macOS architecture for MLX metallib: {arch}"),
        }),
    }
}

fn file_sha256(path: &Path) -> Result<String> {
    let mut file =
        fs::File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("failed to read {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::MlxMetallibAsset;
    use std::path::PathBuf;

    fn test_asset() -> MlxMetallibAsset {
        MlxMetallibAsset {
            install_root: PathBuf::from("/tmp/smrze/runtime/mlx"),
            huggingface_cache_dir: PathBuf::from("/tmp/smrze/huggingface"),
            asset_version: "mlx-test-revision",
            asset_sha256: "abc123",
            arch_dir: "macos-arm64",
        }
    }

    #[test]
    fn remote_path_uses_version_and_arch() {
        assert_eq!(
            test_asset().remote_path(),
            "mlx/mlx-test-revision/macos-arm64/mlx.metallib"
        );
    }

    #[test]
    fn installed_path_uses_runtime_cache() {
        assert_eq!(
            test_asset().installed_path(),
            PathBuf::from("/tmp/smrze/runtime/mlx/mlx-test-revision/macos-arm64/mlx.metallib")
        );
    }
}
