use blake3::Hasher;
use std::fs;
use std::io::Read;
use std::path::Path;

use crate::error::{BuildSupportError, Result};

pub fn blake3_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path).map_err(|error| {
        BuildSupportError::new(format!("failed to open {}: {error}", path.display()))
    })?;
    let mut hasher = Hasher::new();
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| {
            BuildSupportError::new(format!("failed to read {}: {error}", path.display()))
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}
