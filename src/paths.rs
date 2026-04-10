use color_eyre::{
    Result,
    eyre::{Context, eyre},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::utils::{ensure_parent_dir, expand_path, sanitize_name};

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub data_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct RunPaths {
    pub scratch_dir: PathBuf,
    pub final_dir: PathBuf,
    pub final_path: PathBuf,
    pub user_provided_output_dir: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct OutputIndex {
    entries: Vec<OutputIndexEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OutputIndexEntry {
    logical_key: String,
    dir_name: String,
}

impl AppPaths {
    pub fn resolve() -> Result<Self> {
        let config_dir = base_dir("XDG_CONFIG_HOME", ".config")?.join("smrze");
        let cache_dir = base_dir("XDG_CACHE_HOME", ".cache")?.join("smrze");
        let data_dir = base_dir("XDG_DATA_HOME", ".local/share")?.join("smrze");

        for dir in [&config_dir, &cache_dir, &data_dir] {
            fs::create_dir_all(dir)
                .with_context(|| format!("failed to create {}", dir.display()))?;
        }

        Ok(Self {
            config_dir,
            cache_dir,
            data_dir,
        })
    }

    pub fn output_root(&self) -> PathBuf {
        self.data_dir.join("outputs")
    }

    pub fn output_index_path(&self) -> PathBuf {
        self.config_dir.join("output_index.json")
    }

    pub fn scriptrs_model_cache(&self) -> PathBuf {
        self.cache_dir.join("models").join("scriptrs")
    }

    pub fn speakrs_model_cache(&self) -> PathBuf {
        self.cache_dir.join("models").join("speakrs")
    }

    pub fn create_run(
        &self,
        preferred_name: &str,
        logical_key: &str,
        output_dir: Option<&Path>,
        run_id: &str,
    ) -> Result<RunPaths> {
        let scratch_dir = self.cache_dir.join("runs").join(run_id);
        fs::create_dir_all(&scratch_dir)
            .with_context(|| format!("failed to create {}", scratch_dir.display()))?;

        let (final_dir, user_provided_output_dir) = match output_dir {
            Some(dir) => {
                let expanded = expand_path(dir)?;
                fs::create_dir_all(&expanded)
                    .with_context(|| format!("failed to create {}", expanded.display()))?;
                (expanded, true)
            }
            None => {
                let dir_name = self.resolve_output_dir_name(preferred_name, logical_key)?;
                let final_dir = self.output_root().join(dir_name);
                fs::create_dir_all(&final_dir)
                    .with_context(|| format!("failed to create {}", final_dir.display()))?;
                (final_dir, false)
            }
        };
        let final_path = final_dir.join("transcript.txt");
        Ok(RunPaths {
            scratch_dir,
            final_dir,
            final_path,
            user_provided_output_dir,
        })
    }

    fn resolve_output_dir_name(&self, preferred_name: &str, logical_key: &str) -> Result<String> {
        let index_path = self.output_index_path();
        let mut index = load_index(&index_path)?;
        if let Some(entry) = index
            .entries
            .iter()
            .find(|entry| entry.logical_key == logical_key)
        {
            return Ok(entry.dir_name.clone());
        }

        let base_name = sanitize_name(preferred_name);
        let mut candidate = base_name.clone();
        let existing_by_dir = index
            .entries
            .iter()
            .map(|entry| (entry.dir_name.as_str(), entry.logical_key.as_str()))
            .collect::<HashMap<_, _>>();
        if let Some(existing_key) = existing_by_dir.get(candidate.as_str())
            && *existing_key != logical_key
        {
            candidate = format!("{base_name}-{}", crate::utils::short_hash(logical_key));
        }

        index.entries.push(OutputIndexEntry {
            logical_key: logical_key.to_owned(),
            dir_name: candidate.clone(),
        });
        save_index(&index_path, &index)?;
        Ok(candidate)
    }
}

fn base_dir(env_name: &str, home_suffix: &str) -> Result<PathBuf> {
    if let Some(path) = std::env::var_os(env_name) {
        return Ok(PathBuf::from(path));
    }

    let home = std::env::var_os("HOME").ok_or_else(|| eyre!("HOME is not set"))?;
    Ok(PathBuf::from(home).join(home_suffix))
}

fn load_index(path: &Path) -> Result<OutputIndex> {
    if !path.exists() {
        return Ok(OutputIndex::default());
    }

    let file =
        fs::File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    serde_json::from_reader(file).with_context(|| format!("failed to parse {}", path.display()))
}

fn save_index(path: &Path, index: &OutputIndex) -> Result<()> {
    ensure_parent_dir(path)?;
    let temp_path = path.with_extension("json.tmp");
    {
        let file = fs::File::create(&temp_path)
            .with_context(|| format!("failed to create {}", temp_path.display()))?;
        serde_json::to_writer_pretty(file, index)
            .with_context(|| format!("failed to write {}", temp_path.display()))?;
    }
    fs::rename(&temp_path, path)
        .with_context(|| format!("failed to replace {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{AppPaths, RunPaths};
    use crate::utils::short_hash;
    use color_eyre::Result;
    use std::fs;
    use std::path::Path;

    fn test_paths(root: &Path) -> AppPaths {
        AppPaths {
            config_dir: root.join("config"),
            cache_dir: root.join("cache"),
            data_dir: root.join("data"),
        }
    }

    fn run_paths(
        app_paths: &AppPaths,
        logical_key: &str,
        preferred_name: &str,
    ) -> Result<RunPaths> {
        app_paths.create_run(preferred_name, logical_key, None, "run-1")
    }

    #[test]
    fn same_logical_input_reuses_output_dir() -> Result<()> {
        let root = std::env::temp_dir().join(format!("smrze-test-{}", short_hash("same-dir")));
        let _ = fs::remove_dir_all(&root);
        let app_paths = test_paths(&root);
        for dir in [
            &app_paths.config_dir,
            &app_paths.cache_dir,
            &app_paths.data_dir,
        ] {
            fs::create_dir_all(dir)?;
        }

        let first = run_paths(&app_paths, "key-a", "Interview")?;
        let second = run_paths(&app_paths, "key-a", "Interview")?;

        assert_eq!(first.final_dir, second.final_dir);
        let _ = fs::remove_dir_all(&root);
        Ok(())
    }

    #[test]
    fn different_logical_inputs_get_collision_suffix() -> Result<()> {
        let root = std::env::temp_dir().join(format!("smrze-test-{}", short_hash("collision")));
        let _ = fs::remove_dir_all(&root);
        let app_paths = test_paths(&root);
        for dir in [
            &app_paths.config_dir,
            &app_paths.cache_dir,
            &app_paths.data_dir,
        ] {
            fs::create_dir_all(dir)?;
        }

        let first = run_paths(&app_paths, "key-a", "Interview")?;
        let second = run_paths(&app_paths, "key-b", "Interview")?;

        assert_ne!(first.final_dir, second.final_dir);
        assert!(second.final_dir.to_string_lossy().contains("interview-"));
        let _ = fs::remove_dir_all(&root);
        Ok(())
    }
}
