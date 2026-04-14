use color_eyre::Result;

use crate::paths::AppPaths;

use super::support::{CacheSpec, cache_entry_dir, remove_dir_if_exists};

pub(crate) fn clear_cache_entry(
    app_paths: &AppPaths,
    spec: &CacheSpec,
    key_material: &str,
) -> Result<()> {
    remove_dir_if_exists(&cache_entry_dir(app_paths, spec, key_material))
}

pub(crate) fn load_cache_entry<T, F>(
    app_paths: &AppPaths,
    spec: &CacheSpec,
    key_material: &str,
    force: bool,
    load: F,
) -> Result<Option<T>>
where
    F: FnOnce(&AppPaths, &str) -> Result<Option<T>>,
{
    if force {
        clear_cache_entry(app_paths, spec, key_material)?;
        return Ok(None);
    }

    if let Some(entry) = load(app_paths, key_material)? {
        return Ok(Some(entry));
    }

    clear_cache_entry(app_paths, spec, key_material)?;
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::load_cache_entry;
    use crate::cache::{
        AUDIO_CACHE_SPEC, cache_entry_dir, clear_audio_cache_entry, ensure_audio_cache_entry_dir,
    };
    use crate::paths::AppPaths;
    use color_eyre::Result;
    use std::fs;

    fn test_paths(name: &str) -> AppPaths {
        AppPaths {
            cache_dir: std::env::temp_dir().join(name),
        }
    }

    #[test]
    fn clear_cache_entry_ignores_missing_dirs() -> Result<()> {
        let app_paths = test_paths("smrze-cache-sweeper-clear");
        clear_audio_cache_entry(&app_paths, "missing")?;
        let _ = fs::remove_dir_all(&app_paths.cache_dir);
        Ok(())
    }

    #[test]
    fn load_cache_entry_skips_loader_when_force_is_enabled() -> Result<()> {
        let app_paths = test_paths("smrze-cache-load-force");
        let key = "source-key";
        ensure_audio_cache_entry_dir(&app_paths, key)?;
        let mut load_called = false;

        let cached = load_cache_entry(&app_paths, &AUDIO_CACHE_SPEC, key, true, |_, _| {
            load_called = true;
            Ok(Some("cached"))
        })?;

        assert!(cached.is_none());
        assert!(!load_called);
        assert!(!cache_entry_dir(&app_paths, &AUDIO_CACHE_SPEC, key).exists());
        let _ = fs::remove_dir_all(&app_paths.cache_dir);
        Ok(())
    }

    #[test]
    fn load_cache_entry_clears_stale_dir_after_cache_miss() -> Result<()> {
        let app_paths = test_paths("smrze-cache-load-miss");
        let key = "source-key";
        ensure_audio_cache_entry_dir(&app_paths, key)?;

        let cached = load_cache_entry(&app_paths, &AUDIO_CACHE_SPEC, key, false, |_, _| {
            Ok(None::<()>)
        })?;

        assert!(cached.is_none());
        assert!(!cache_entry_dir(&app_paths, &AUDIO_CACHE_SPEC, key).exists());
        let _ = fs::remove_dir_all(&app_paths.cache_dir);
        Ok(())
    }
}
