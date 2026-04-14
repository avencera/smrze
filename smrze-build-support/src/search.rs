use std::fs;
use std::path::{Path, PathBuf};

pub fn find_file_named(root: &Path, file_name: &str) -> Option<PathBuf> {
    if !root.exists() {
        return None;
    }

    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_file_named(&path, file_name) {
                return Some(found);
            }
            continue;
        }

        if path.file_name().and_then(|name| name.to_str()) == Some(file_name) {
            return Some(path);
        }
    }

    None
}
