use std::{env, fs, path::PathBuf, process::Command};

use clap::{Parser, Subcommand, ValueEnum};
use color_eyre::{
    Result,
    eyre::{Context, bail, eyre},
};
use smrze_build_support::{
    blake3_file, build_mlx_metallib, current_runtime_arch_dir, current_xcode_arch,
    ensure_local_mlx_repo, ensure_metal_toolchain, mlx_repo_revision,
};

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
    let metallib_path = build_mlx_metallib(
        &mlx_repo_dir,
        &derived_data_dir,
        current_xcode_arch()?,
        "Release",
    )?;
    let asset_version = mlx_repo_revision(&mlx_repo_dir)?;
    let arch_dir = current_runtime_arch_dir()?;
    let remote_path = format!("mlx/{asset_version}/{arch_dir}/mlx.metallib");
    let blake3 = blake3_file(&metallib_path)?;

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
    println!("BLAKE3 {blake3}");
    Ok(())
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
        .ok_or_else(|| eyre!("Cargo.toml should have a parent directory"))?
        .to_path_buf())
}
