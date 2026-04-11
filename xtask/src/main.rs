use std::{env, fs, io::Read, path::Path, path::PathBuf, process::Command};

use clap::{Parser, Subcommand, ValueEnum};
use color_eyre::{
    Result,
    eyre::{Context, bail, eyre},
};
use sha2::{Digest, Sha256};

const HF_RUNTIME_REPO: &str = "avencera/smrze-runtime-assets";

#[derive(Parser)]
#[command(name = "xtask")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Build and install smrze
    Release {
        /// Where to install the binary
        #[arg(value_enum)]
        place: Place,
    },
    /// Build and upload the MLX metallib runtime asset
    PublishMlxMetallib,
}

#[derive(Clone, ValueEnum)]
enum Place {
    /// Install to ~/.local/bin/smrze
    Local,
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let cli = Cli::parse();

    match cli.command {
        Commands::Release { place } => release(place),
        Commands::PublishMlxMetallib => publish_mlx_metallib(),
    }
}

fn release(place: Place) -> Result<()> {
    match place {
        Place::Local => release_local(),
    }
}

fn release_local() -> Result<()> {
    let workspace_root = workspace_root()?;

    let status = Command::new("cargo")
        .args(["build", "--release", "--package", "smrze"])
        .current_dir(&workspace_root)
        .status()
        .with_context(|| "failed to run cargo build --release")?;
    if !status.success() {
        bail!("cargo build --release --package smrze failed");
    }

    let home = env::var("HOME").with_context(|| "HOME is not set")?;
    let bin_dir = PathBuf::from(home).join(".local").join("bin");
    fs::create_dir_all(&bin_dir)
        .with_context(|| format!("failed to create {}", bin_dir.display()))?;

    let src = workspace_root.join("target/release/smrze");
    let dest = bin_dir.join("smrze");

    let _ = fs::remove_file(&dest);
    fs::copy(&src, &dest).with_context(|| {
        format!(
            "failed to copy built binary from {} to {}",
            src.display(),
            dest.display()
        )
    })?;

    println!("Installed smrze to {}", dest.display());
    Ok(())
}

fn publish_mlx_metallib() -> Result<()> {
    let workspace_root = workspace_root()?;
    let mlx_repo_dir = workspace_root
        .parent()
        .ok_or_else(|| eyre!("workspace root should have a parent"))?
        .join("mlx-swift");
    ensure_local_mlx_repo(&mlx_repo_dir)?;
    ensure_metal_toolchain()?;

    let derived_data_dir = workspace_root.join("target/mlx-runtime-assets");
    let status = Command::new("xcodebuild")
        .arg("build")
        .arg("-project")
        .arg(mlx_repo_dir.join("xcode/MLX.xcodeproj"))
        .arg("-scheme")
        .arg("Cmlx")
        .arg("-configuration")
        .arg("Release")
        .arg("-destination")
        .arg(format!("platform=macOS,arch={}", current_xcode_arch()?))
        .arg("-derivedDataPath")
        .arg(&derived_data_dir)
        .status()
        .with_context(|| "failed to run xcodebuild for the MLX metallib asset")?;
    if !status.success() {
        bail!("xcodebuild failed while building the MLX metallib asset");
    }

    let metallib_path = find_file_named(
        &derived_data_dir.join("Build/Products/Release"),
        "default.metallib",
    )
    .ok_or_else(|| eyre!("failed to locate default.metallib after xcodebuild"))?;
    let asset_version = mlx_repo_revision(&mlx_repo_dir)?;
    let arch_dir = current_runtime_arch_dir()?;
    let remote_path = format!("mlx/{asset_version}/{arch_dir}/mlx.metallib");
    let sha256 = sha256_file(&metallib_path)?;

    let status = Command::new("hf")
        .args([
            "upload",
            HF_RUNTIME_REPO,
            metallib_path
                .to_str()
                .ok_or_else(|| eyre!("metallib path was not valid utf-8"))?,
            &remote_path,
        ])
        .status()
        .with_context(|| "failed to run hf upload for the MLX metallib asset")?;
    if !status.success() {
        bail!("hf upload failed for the MLX metallib asset");
    }

    println!("Uploaded {remote_path}");
    println!("SHA-256 {sha256}");
    Ok(())
}

fn ensure_local_mlx_repo(mlx_repo_dir: &Path) -> Result<()> {
    if !mlx_repo_dir.exists() {
        bail!(
            "expected a local mlx-swift checkout at {}",
            mlx_repo_dir.display()
        );
    }
    if !mlx_repo_dir
        .join("Source/Cmlx/mlx/mlx/backend/metal/device.cpp")
        .exists()
    {
        bail!(
            "expected mlx-swift submodules to be initialized under {}\nrun: git -C {} submodule update --init --recursive",
            mlx_repo_dir.display(),
            mlx_repo_dir.display()
        );
    }
    Ok(())
}

fn ensure_metal_toolchain() -> Result<()> {
    let status = Command::new("xcrun")
        .args(["metal", "-v"])
        .status()
        .with_context(|| "failed to check the Metal Toolchain")?;
    if status.success() {
        return Ok(());
    }

    bail!(
        "the Metal Toolchain is required; install it with: xcodebuild -downloadComponent MetalToolchain"
    );
}

fn workspace_root() -> Result<PathBuf> {
    let output = Command::new("cargo")
        .args(["locate-project", "--workspace", "--message-format=plain"])
        .output()
        .with_context(|| "failed to locate workspace root")?;
    if !output.status.success() {
        bail!("cargo locate-project --workspace failed");
    }

    let cargo_toml = PathBuf::from(
        String::from_utf8(output.stdout)
            .with_context(|| "cargo locate-project returned non-utf8 output")?
            .trim(),
    );

    Ok(cargo_toml
        .parent()
        .expect("Cargo.toml should have a parent directory")
        .to_path_buf())
}

fn current_xcode_arch() -> Result<&'static str> {
    match env::consts::ARCH {
        "aarch64" => Ok("arm64"),
        "x86_64" => Ok("x86_64"),
        arch => bail!("unsupported macOS architecture for xcodebuild: {arch}"),
    }
}

fn current_runtime_arch_dir() -> Result<&'static str> {
    match env::consts::ARCH {
        "aarch64" => Ok("macos-arm64"),
        "x86_64" => Ok("macos-x86_64"),
        arch => bail!("unsupported macOS architecture for runtime assets: {arch}"),
    }
}

fn mlx_repo_revision(mlx_repo_dir: &Path) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(mlx_repo_dir)
        .arg("rev-parse")
        .arg("HEAD")
        .output()
        .with_context(|| {
            format!(
                "failed to read the mlx-swift revision from {}",
                mlx_repo_dir.display()
            )
        })?;
    if !output.status.success() {
        bail!("git rev-parse HEAD failed for {}", mlx_repo_dir.display());
    }

    Ok(String::from_utf8(output.stdout)
        .with_context(|| "git rev-parse returned non-utf8 output")?
        .trim()
        .to_owned())
}

fn find_file_named(root: &Path, file_name: &str) -> Option<PathBuf> {
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

fn sha256_file(path: &Path) -> Result<String> {
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
