use smrze_build_support::{
    Result, blake3_file, build_mlx_metallib, cargo_profile_dir, developer_dir,
    ensure_local_mlx_repo, ensure_metal_toolchain, mlx_device_cpp_path, mlx_repo_revision,
    mlx_xcode_project_path, run_checked_command, swift_triple_dir_for_target,
    xcode_arch_for_target,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    if let Err(error) = try_main() {
        panic!("{error}");
    }
}

fn try_main() -> Result<()> {
    let context = BuildContext::current()?;
    context.register_rerun_inputs();

    if !context.is_macos_target() {
        return Ok(());
    }

    context.run()
}

struct BuildContext {
    manifest_dir: PathBuf,
    out_dir: PathBuf,
    target_arch: String,
    release_build: bool,
    local_mlx_repo_dir: PathBuf,
}

impl BuildContext {
    fn current() -> Result<Self> {
        let manifest_dir = required_env_path("CARGO_MANIFEST_DIR");
        let out_dir = required_env_path("OUT_DIR");
        let target_arch = required_env("CARGO_CFG_TARGET_ARCH");
        let local_mlx_repo_dir = manifest_dir
            .parent()
            .expect("smrze manifest dir should have a parent")
            .join("mlx-swift");
        let release_build = std::env::var("PROFILE").as_deref() == Ok("release");

        Ok(Self {
            manifest_dir,
            out_dir,
            target_arch,
            release_build,
            local_mlx_repo_dir,
        })
    }

    fn register_rerun_inputs(&self) {
        for path in [
            "build.rs",
            "src/foundation_models_bridge.rs",
            "apple-foundation-models/Package.swift",
            "apple-foundation-models/Package.resolved",
            "apple-foundation-models/Sources/SmrzeFoundationModels",
        ] {
            println!("cargo:rerun-if-changed={path}");
        }
    }

    fn is_macos_target(&self) -> bool {
        std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos")
    }

    fn run(&self) -> Result<()> {
        self.register_local_mlx_inputs();
        self.ensure_local_mlx_repo()?;

        self.generate_swift_bridge();
        self.compile_swift_library()?;
        let metallib_path = self.compile_mlx_metallib()?;
        copy_file(
            &metallib_path,
            &self.cargo_profile_dir()?.join("mlx.metallib"),
        )?;
        self.export_mlx_runtime_metadata(&metallib_path)?;
        self.link_swift_library()?;
        Ok(())
    }

    fn register_local_mlx_inputs(&self) {
        for path in [
            self.local_mlx_repo_dir.join("Package.swift"),
            mlx_xcode_project_path(&self.local_mlx_repo_dir),
            self.local_mlx_repo_dir
                .join("xcode")
                .join("xcconfig")
                .join("Cmlx.xcconfig"),
            mlx_device_cpp_path(&self.local_mlx_repo_dir),
        ] {
            if path.exists() {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }

    fn ensure_local_mlx_repo(&self) -> Result<()> {
        ensure_local_mlx_repo(&self.local_mlx_repo_dir)
    }

    fn generate_swift_bridge(&self) {
        swift_bridge_build::parse_bridges(vec!["src/foundation_models_bridge.rs"])
            .write_all_concatenated(self.generated_code_dir(), env!("CARGO_PKG_NAME"));
    }

    fn compile_swift_library(&self) -> Result<()> {
        let mut command = Command::new("xcrun");
        command
            .arg("swift")
            .arg("build")
            .arg("--package-path")
            .arg(self.apple_foundation_models_dir())
            .arg("--product")
            .arg("SmrzeFoundationModels")
            .arg("--arch")
            .arg(&self.target_arch)
            .arg("-Xswiftc")
            .arg("-static")
            .arg("-Xswiftc")
            .arg("-import-objc-header")
            .arg("-Xswiftc")
            .arg(self.swift_source_dir().join("bridging-header.h"));

        if self.release_build {
            command.arg("-c").arg("release");
        }

        run_checked_command(&mut command, "xcrun swift build for summary bridge")?;
        Ok(())
    }

    fn compile_mlx_metallib(&self) -> Result<PathBuf> {
        self.ensure_metal_toolchain()?;
        build_mlx_metallib(
            &self.local_mlx_repo_dir,
            &self.mlx_derived_data_dir(),
            self.current_xcode_arch()?,
            self.xcode_build_configuration(),
        )
    }

    fn export_mlx_runtime_metadata(&self, metallib_path: &Path) -> Result<()> {
        println!(
            "cargo:rustc-env=SMRZE_MLX_RUNTIME_ASSET_VERSION={}",
            self.mlx_repo_revision()?
        );
        println!(
            "cargo:rustc-env=SMRZE_MLX_RUNTIME_ASSET_BLAKE3={}",
            blake3_file(metallib_path)?
        );
        Ok(())
    }

    fn link_swift_library(&self) -> Result<()> {
        println!("cargo:rustc-link-lib=static=SmrzeFoundationModels");
        println!(
            "cargo:rustc-link-search=native={}",
            self.swift_library_dir()?.display()
        );
        println!("cargo:rustc-link-lib=framework=Foundation");
        println!("cargo:rustc-link-lib=framework=FoundationModels");
        println!("cargo:rustc-link-lib=framework=BackgroundAssets");

        let swift_runtime_dir = format!(
            "{}/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift/macosx",
            developer_dir()
        );
        println!("cargo:rustc-link-search={swift_runtime_dir}");
        println!("cargo:rustc-link-arg=-Wl,-rpath,{swift_runtime_dir}");
        println!("cargo:rustc-link-search=/usr/lib/swift");
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
        Ok(())
    }

    fn ensure_metal_toolchain(&self) -> Result<()> {
        ensure_metal_toolchain()
    }

    fn cargo_profile_dir(&self) -> Result<PathBuf> {
        cargo_profile_dir(&self.out_dir)
    }

    fn apple_foundation_models_dir(&self) -> PathBuf {
        self.manifest_dir.join("apple-foundation-models")
    }

    fn swift_source_dir(&self) -> PathBuf {
        self.apple_foundation_models_dir()
            .join("Sources")
            .join("SmrzeFoundationModels")
    }

    fn generated_code_dir(&self) -> PathBuf {
        self.swift_source_dir().join("generated")
    }

    fn mlx_derived_data_dir(&self) -> PathBuf {
        self.apple_foundation_models_dir()
            .join(".build")
            .join("mlx-derived-data")
    }

    fn swift_library_dir(&self) -> Result<PathBuf> {
        let build_mode = if self.release_build {
            "release"
        } else {
            "debug"
        };
        Ok(self
            .apple_foundation_models_dir()
            .join(".build")
            .join(self.current_swift_triple_dir()?)
            .join(build_mode))
    }

    fn current_xcode_arch(&self) -> Result<&'static str> {
        xcode_arch_for_target(&self.target_arch)
    }

    fn current_swift_triple_dir(&self) -> Result<&'static str> {
        swift_triple_dir_for_target(&self.target_arch)
    }

    fn xcode_build_configuration(&self) -> &'static str {
        if self.release_build {
            "Release"
        } else {
            "Debug"
        }
    }

    fn mlx_repo_revision(&self) -> Result<String> {
        mlx_repo_revision(&self.local_mlx_repo_dir)
    }
}

fn required_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("missing {name}"))
}

fn required_env_path(name: &str) -> PathBuf {
    PathBuf::from(required_env(name))
}

fn copy_file(source_path: &Path, output_path: &Path) -> Result<()> {
    fs::copy(source_path, output_path).map_err(|error| {
        smrze_build_support::BuildSupportError::new(format!(
            "failed to copy {} to {}: {error}",
            source_path.display(),
            output_path.display()
        ))
    })?;
    Ok(())
}
