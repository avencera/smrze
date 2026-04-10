use std::{env, fs, path::PathBuf, process::Command};

use clap::{Parser, Subcommand, ValueEnum};
use color_eyre::{
    Result,
    eyre::{Context, bail},
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
