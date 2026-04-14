use std::path::{Path, PathBuf};
use std::process::Command;

use crate::command::{command_output, run_checked_command};
use crate::error::{BuildSupportError, Result};
use crate::search::find_file_named;

pub fn ensure_local_mlx_repo(repo_dir: &Path) -> Result<()> {
    if !repo_dir.exists() {
        return Err(BuildSupportError::new(format!(
            "expected a local mlx-swift checkout at {}\nclone it with: git clone https://github.com/ml-explore/mlx-swift.git {}",
            repo_dir.display(),
            repo_dir.display()
        )));
    }

    if !mlx_device_cpp_path(repo_dir).exists() {
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

pub fn build_mlx_metallib(
    repo_dir: &Path,
    derived_data_dir: &Path,
    arch: &str,
    configuration: &str,
) -> Result<PathBuf> {
    let mut command = Command::new("xcodebuild");
    command
        .arg("build")
        .arg("-project")
        .arg(mlx_xcode_project_path(repo_dir))
        .arg("-scheme")
        .arg("Cmlx")
        .arg("-configuration")
        .arg(configuration)
        .arg("-destination")
        .arg(format!("platform=macOS,arch={arch}"))
        .arg("-derivedDataPath")
        .arg(derived_data_dir);

    run_checked_command(&mut command, "xcodebuild for MLX metallib")?;

    let build_products_dir = derived_data_dir.join("Build/Products").join(configuration);
    find_file_named(&build_products_dir, "default.metallib").ok_or_else(|| {
        BuildSupportError::new(format!(
            "failed to locate default.metallib under {} after building MLX",
            build_products_dir.display()
        ))
    })
}

pub fn mlx_device_cpp_path(repo_dir: &Path) -> PathBuf {
    repo_dir
        .join("Source")
        .join("Cmlx")
        .join("mlx")
        .join("mlx")
        .join("backend")
        .join("metal")
        .join("device.cpp")
}

pub fn mlx_xcode_project_path(repo_dir: &Path) -> PathBuf {
    repo_dir.join("xcode").join("MLX.xcodeproj")
}

pub fn mlx_repo_revision(repo_dir: &Path) -> Result<String> {
    let mut command = Command::new("git");
    command.arg("-C").arg(repo_dir).arg("rev-parse").arg("HEAD");
    let output = run_checked_command(&mut command, "git rev-parse HEAD for mlx-swift")?;
    String::from_utf8(output.stdout)
        .map(|output| output.trim().to_owned())
        .map_err(|_| BuildSupportError::new("local mlx-swift revision should be utf-8"))
}
