use std::{env, fs, path::PathBuf, process::Command};

use clap::{Parser, Subcommand, ValueEnum};
use color_eyre::{
    Result,
    eyre::{Context, bail, eyre},
};

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

    let helper_src = build_summary_helper(&workspace_root)?;

    let home = env::var("HOME").with_context(|| "HOME is not set")?;
    let bin_dir = PathBuf::from(home).join(".local").join("bin");
    fs::create_dir_all(&bin_dir)
        .with_context(|| format!("failed to create {}", bin_dir.display()))?;

    let src = workspace_root.join("target/release/smrze");
    let dest = bin_dir.join("smrze");
    let helper_dest = bin_dir.join("smrze-foundation-models");

    let _ = fs::remove_file(&dest);
    fs::copy(&src, &dest).with_context(|| {
        format!(
            "failed to copy built binary from {} to {}",
            src.display(),
            dest.display()
        )
    })?;
    let _ = fs::remove_file(&helper_dest);
    fs::copy(&helper_src, &helper_dest).with_context(|| {
        format!(
            "failed to copy summary helper from {} to {}",
            helper_src.display(),
            helper_dest.display()
        )
    })?;

    println!("Installed smrze to {}", dest.display());
    println!("Installed summary helper to {}", helper_dest.display());
    Ok(())
}

fn build_summary_helper(workspace_root: &PathBuf) -> Result<PathBuf> {
    let package_path = workspace_root.join("apple-foundation-models");
    let package_path_str = package_path
        .to_str()
        .ok_or_else(|| eyre!("invalid package path: {}", package_path.display()))?;
    let status = Command::new("xcrun")
        .args([
            "swift",
            "build",
            "-c",
            "release",
            "--package-path",
            package_path_str,
            "--product",
            "smrze-foundation-models",
        ])
        .status()
        .with_context(|| "failed to run xcrun swift build for summary helper")?;
    if !status.success() {
        bail!("xcrun swift build for summary helper failed");
    }

    let output = Command::new("xcrun")
        .args([
            "swift",
            "build",
            "-c",
            "release",
            "--package-path",
            package_path_str,
            "--show-bin-path",
        ])
        .output()
        .with_context(|| "failed to resolve summary helper bin path")?;
    if !output.status.success() {
        bail!("xcrun swift build --show-bin-path failed");
    }

    let bin_path = PathBuf::from(
        String::from_utf8(output.stdout)
            .with_context(|| "summary helper bin path was not utf-8")?
            .trim(),
    );
    Ok(bin_path.join("smrze-foundation-models"))
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
