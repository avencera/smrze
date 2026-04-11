use color_eyre::{
    Result,
    eyre::{Context, eyre},
};
use std::fs;
use std::path::{Path, PathBuf};

use crate::utils::expand_path;

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub cache_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct RunPaths {
    pub scratch_dir: PathBuf,
    pub final_dir: PathBuf,
    pub final_path: PathBuf,
    pub summary_path: PathBuf,
}

impl AppPaths {
    pub fn resolve() -> Result<Self> {
        let cache_dir = base_dir("XDG_CACHE_HOME", ".cache")?.join("smrze");
        fs::create_dir_all(&cache_dir)
            .with_context(|| format!("failed to create {}", cache_dir.display()))?;

        Ok(Self { cache_dir })
    }

    pub fn scriptrs_model_cache(&self) -> PathBuf {
        self.cache_dir.join("models").join("scriptrs")
    }

    pub fn speakrs_model_cache(&self) -> PathBuf {
        self.cache_dir.join("models").join("speakrs")
    }

    pub fn huggingface_cache(&self) -> PathBuf {
        self.cache_dir.join("huggingface")
    }

    pub fn mlx_runtime_cache(&self) -> PathBuf {
        self.cache_dir.join("runtime").join("mlx")
    }

    pub fn create_run(&self, output_dir: &Path, run_id: &str) -> Result<RunPaths> {
        let scratch_dir = self.cache_dir.join("runs").join(run_id);
        fs::create_dir_all(&scratch_dir)
            .with_context(|| format!("failed to create {}", scratch_dir.display()))?;

        let final_dir = expand_path(output_dir)?;
        fs::create_dir_all(&final_dir)
            .with_context(|| format!("failed to create {}", final_dir.display()))?;
        let final_path = final_dir.join("transcript.txt");
        let summary_path = final_dir.join("summary.md");
        Ok(RunPaths {
            scratch_dir,
            final_dir,
            final_path,
            summary_path,
        })
    }
}

fn base_dir(env_name: &str, home_suffix: &str) -> Result<PathBuf> {
    if let Some(path) = std::env::var_os(env_name) {
        return Ok(PathBuf::from(path));
    }

    let home = std::env::var_os("HOME").ok_or_else(|| eyre!("HOME is not set"))?;
    Ok(PathBuf::from(home).join(home_suffix))
}

#[cfg(test)]
mod tests {
    use super::{AppPaths, RunPaths};
    use color_eyre::Result;
    use std::fs;
    use std::path::Path;

    fn test_paths(root: &Path) -> AppPaths {
        AppPaths {
            cache_dir: root.join("cache"),
        }
    }

    fn run_paths(app_paths: &AppPaths, output_dir: &Path) -> Result<RunPaths> {
        app_paths.create_run(output_dir, "run-1")
    }

    #[test]
    fn create_run_uses_explicit_output_dir() -> Result<()> {
        let root = std::env::temp_dir().join("smrze-test-explicit-output");
        let _ = fs::remove_dir_all(&root);
        let app_paths = test_paths(&root);
        for dir in [&app_paths.cache_dir] {
            fs::create_dir_all(dir)?;
        }

        let output_dir = root.join("custom-output");
        let run_paths = run_paths(&app_paths, &output_dir)?;

        assert_eq!(run_paths.final_dir, output_dir);
        assert_eq!(run_paths.final_path, output_dir.join("transcript.txt"));
        assert_eq!(run_paths.summary_path, output_dir.join("summary.md"));
        let _ = fs::remove_dir_all(&root);
        Ok(())
    }
}
