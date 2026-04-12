use blake3::Hasher;
use std::env;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub type Result<T> = std::result::Result<T, BuildSupportError>;

#[derive(Debug, Clone)]
pub struct BuildSupportError(String);

impl BuildSupportError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl Display for BuildSupportError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for BuildSupportError {}

pub fn ensure_local_mlx_repo(repo_dir: &Path) -> Result<()> {
    if !repo_dir.exists() {
        return Err(BuildSupportError::new(format!(
            "expected a local mlx-swift checkout at {}\nclone it with: git clone https://github.com/ml-explore/mlx-swift.git {}",
            repo_dir.display(),
            repo_dir.display()
        )));
    }

    let device_cpp = repo_dir
        .join("Source")
        .join("Cmlx")
        .join("mlx")
        .join("mlx")
        .join("backend")
        .join("metal")
        .join("device.cpp");
    if !device_cpp.exists() {
        return Err(BuildSupportError::new(format!(
            "expected mlx-swift submodules to be initialized under {}\nrun: git -C {} submodule update --init --recursive",
            repo_dir.display(),
            repo_dir.display()
        )));
    }

    Ok(())
}

pub fn ensure_metal_toolchain() -> Result<()> {
    let mut command = Command::new("xcrun");
    command.arg("metal").arg("-v");
    let output = command_output(&mut command, "xcrun metal -v")?;
    if output.status.success() {
        return Ok(());
    }

    Err(BuildSupportError::new(format!(
        "the Metal Toolchain is required to build MLX Gemma summaries\nstdout:\n{}\nstderr:\n{}\ninstall it with: xcodebuild -downloadComponent MetalToolchain",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )))
}

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

pub fn mlx_repo_revision(repo_dir: &Path) -> Result<String> {
    let mut command = Command::new("git");
    command.arg("-C").arg(repo_dir).arg("rev-parse").arg("HEAD");
    let output = run_checked_command(&mut command, "git rev-parse HEAD for mlx-swift")?;
    String::from_utf8(output.stdout)
        .map(|output| output.trim().to_owned())
        .map_err(|_| BuildSupportError::new("local mlx-swift revision should be utf-8"))
}

pub fn developer_dir() -> String {
    let mut command = Command::new("xcode-select");
    command.arg("--print-path");
    command_output(&mut command, "xcode-select --print-path")
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|output| output.trim().to_owned())
        .filter(|output| !output.is_empty())
        .unwrap_or_else(|| "/Applications/Xcode.app/Contents/Developer".to_owned())
}

pub fn run_checked_command(command: &mut Command, action: &str) -> Result<Output> {
    let output = command_output(command, action)?;
    if output.status.success() {
        return Ok(output);
    }

    Err(BuildSupportError::new(format!(
        "{action} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )))
}

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

fn command_output(command: &mut Command, action: &str) -> Result<Output> {
    command
        .output()
        .map_err(|error| BuildSupportError::new(format!("{action}: {error}")))
}

#[cfg(test)]
mod tests {
    use super::{
        blake3_file, cargo_profile_dir, ensure_local_mlx_repo, find_file_named,
        swift_triple_dir_for_target, xcode_arch_for_target,
    };
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("smrze-build-support-{name}-{unique}"))
    }

    #[test]
    fn xcode_arch_matches_supported_targets() {
        assert_eq!(xcode_arch_for_target("aarch64").unwrap(), "arm64");
        assert_eq!(xcode_arch_for_target("x86_64").unwrap(), "x86_64");
    }

    #[test]
    fn swift_triple_matches_supported_targets() {
        assert_eq!(
            swift_triple_dir_for_target("aarch64").unwrap(),
            "aarch64-apple-macosx"
        );
        assert_eq!(
            swift_triple_dir_for_target("x86_64").unwrap(),
            "x86_64-apple-macosx"
        );
    }

    #[test]
    fn cargo_profile_dir_walks_up_from_out_dir() {
        let out_dir = Path::new("/tmp/target/debug/build/smrze/out");
        assert_eq!(
            cargo_profile_dir(out_dir).unwrap(),
            PathBuf::from("/tmp/target/debug")
        );
    }

    #[test]
    fn ensure_local_mlx_repo_requires_submodule_checkout() {
        let root = temp_dir("mlx-checkout");
        fs::create_dir_all(&root).unwrap();
        let error = ensure_local_mlx_repo(&root).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("submodule update --init --recursive")
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn find_file_named_returns_nested_match() {
        let root = temp_dir("find-file");
        let nested = root.join("a/b");
        fs::create_dir_all(&nested).unwrap();
        let target = nested.join("needle.txt");
        fs::write(&target, "hello").unwrap();

        assert_eq!(find_file_named(&root, "needle.txt"), Some(target));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn blake3_file_hashes_file_contents() {
        let root = temp_dir("blake3");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("file.txt");
        fs::write(&path, "abc").unwrap();

        assert_eq!(
            blake3_file(&path).unwrap(),
            "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85"
        );
        let _ = fs::remove_dir_all(&root);
    }
}
