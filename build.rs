use smrze_build_support::{
    blake3_file, cargo_profile_dir, developer_dir, ensure_local_mlx_repo, ensure_metal_toolchain,
    find_file_named, mlx_repo_revision, run_checked_command, swift_triple_dir_for_target,
    xcode_arch_for_target,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let context = BuildContext::current();
    context.register_rerun_inputs();

    if !context.is_macos_target() {
        return;
    }

    context.run();
}

struct BuildContext {
    manifest_dir: PathBuf,
    out_dir: PathBuf,
    target_arch: String,
    release_build: bool,
    local_mlx_repo_dir: PathBuf,
}

impl BuildContext {
    fn current() -> Self {
        let manifest_dir = required_env_path("CARGO_MANIFEST_DIR");
        let out_dir = required_env_path("OUT_DIR");
        let target_arch = required_env("CARGO_CFG_TARGET_ARCH");
        let local_mlx_repo_dir = manifest_dir
            .parent()
            .unwrap_or_else(|| panic!("smrze manifest dir should have a parent"))
            .join("mlx-swift");
        let release_build = std::env::var("PROFILE").as_deref() == Ok("release");

        Self {
            manifest_dir,
            out_dir,
            target_arch,
            release_build,
            local_mlx_repo_dir,
        }
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

    fn run(&self) {
        self.register_local_mlx_inputs();
        self.ensure_local_mlx_repo();

        self.generate_swift_bridge();
        self.compile_swift_library();
        let metallib_path = self.compile_mlx_metallib();
        copy_file(
            &metallib_path,
            &self.cargo_profile_dir().join("mlx.metallib"),
        );
        self.export_mlx_runtime_metadata(&metallib_path);
        self.link_swift_library();
    }

    fn register_local_mlx_inputs(&self) {
        for path in [
            self.local_mlx_repo_dir.join("Package.swift"),
            self.local_mlx_repo_dir.join("xcode").join("MLX.xcodeproj"),
            self.local_mlx_repo_dir
                .join("xcode")
                .join("xcconfig")
                .join("Cmlx.xcconfig"),
            self.mlx_device_cpp_path(),
        ] {
            if path.exists() {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }

    fn ensure_local_mlx_repo(&self) {
        ensure_local_mlx_repo(&self.local_mlx_repo_dir).unwrap_or_else(|error| panic!("{error}"));
    }

    fn generate_swift_bridge(&self) {
        swift_bridge_build::parse_bridges(vec!["src/foundation_models_bridge.rs"])
            .write_all_concatenated(self.generated_code_dir(), env!("CARGO_PKG_NAME"));
    }

    fn compile_swift_library(&self) {
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

        run_checked_command(&mut command, "xcrun swift build for summary bridge")
            .unwrap_or_else(|error| panic!("{error}"));
    }

    fn compile_mlx_metallib(&self) -> PathBuf {
        self.ensure_metal_toolchain();

        let mut command = Command::new("xcodebuild");
        command
            .arg("build")
            .arg("-project")
            .arg(self.mlx_xcode_project_path())
            .arg("-scheme")
            .arg("Cmlx")
            .arg("-configuration")
            .arg(self.xcode_build_configuration())
            .arg("-destination")
            .arg(format!("platform=macOS,arch={}", self.current_xcode_arch()))
            .arg("-derivedDataPath")
            .arg(self.mlx_derived_data_dir());

        run_checked_command(&mut command, "xcodebuild for MLX metallib")
            .unwrap_or_else(|error| panic!("{error}"));

        find_file_named(&self.mlx_build_products_dir(), "default.metallib").unwrap_or_else(|| {
            panic!(
                "failed to locate default.metallib under {} after building MLX",
                self.mlx_build_products_dir().display()
            )
        })
    }

    fn export_mlx_runtime_metadata(&self, metallib_path: &Path) {
        println!(
            "cargo:rustc-env=SMRZE_MLX_RUNTIME_ASSET_VERSION={}",
            self.mlx_repo_revision()
        );
        println!(
            "cargo:rustc-env=SMRZE_MLX_RUNTIME_ASSET_BLAKE3={}",
            blake3_file(metallib_path).unwrap_or_else(|error| panic!("{error}"))
        );
    }

    fn link_swift_library(&self) {
        println!("cargo:rustc-link-lib=static=SmrzeFoundationModels");
        println!(
            "cargo:rustc-link-search=native={}",
            self.swift_library_dir().display()
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
    }

    fn ensure_metal_toolchain(&self) {
        ensure_metal_toolchain().unwrap_or_else(|error| panic!("{error}"));
    }

    fn cargo_profile_dir(&self) -> PathBuf {
        cargo_profile_dir(&self.out_dir).unwrap_or_else(|error| panic!("{error}"))
    }

    fn apple_foundation_models_dir(&self) -> PathBuf {
        self.manifest_dir.join("apple-foundation-models")
    }

    fn mlx_device_cpp_path(&self) -> PathBuf {
        self.local_mlx_repo_dir
            .join("Source")
            .join("Cmlx")
            .join("mlx")
            .join("mlx")
            .join("backend")
            .join("metal")
            .join("device.cpp")
    }

    fn swift_source_dir(&self) -> PathBuf {
        self.apple_foundation_models_dir()
            .join("Sources")
            .join("SmrzeFoundationModels")
    }

    fn generated_code_dir(&self) -> PathBuf {
        self.swift_source_dir().join("generated")
    }

    fn mlx_xcode_project_path(&self) -> PathBuf {
        self.local_mlx_repo_dir.join("xcode").join("MLX.xcodeproj")
    }

    fn mlx_derived_data_dir(&self) -> PathBuf {
        self.apple_foundation_models_dir()
            .join(".build")
            .join("mlx-derived-data")
    }

    fn mlx_build_products_dir(&self) -> PathBuf {
        self.mlx_derived_data_dir()
            .join("Build")
            .join("Products")
            .join(self.xcode_build_configuration())
    }

    fn swift_library_dir(&self) -> PathBuf {
        let build_mode = if self.release_build {
            "release"
        } else {
            "debug"
        };
        self.apple_foundation_models_dir()
            .join(".build")
            .join(self.current_swift_triple_dir())
            .join(build_mode)
    }

    fn current_xcode_arch(&self) -> &'static str {
        xcode_arch_for_target(&self.target_arch).unwrap_or_else(|error| panic!("{error}"))
    }

    fn current_swift_triple_dir(&self) -> &'static str {
        swift_triple_dir_for_target(&self.target_arch).unwrap_or_else(|error| panic!("{error}"))
    }

    fn xcode_build_configuration(&self) -> &'static str {
        if self.release_build {
            "Release"
        } else {
            "Debug"
        }
    }

    fn mlx_repo_revision(&self) -> String {
        mlx_repo_revision(&self.local_mlx_repo_dir).unwrap_or_else(|error| panic!("{error}"))
    }
}

fn required_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("missing {name}"))
}

fn required_env_path(name: &str) -> PathBuf {
    PathBuf::from(required_env(name))
}

fn copy_file(source_path: &Path, output_path: &Path) {
    fs::copy(source_path, output_path).unwrap_or_else(|error| {
        panic!(
            "failed to copy {} to {}: {error}",
            source_path.display(),
            output_path.display()
        )
    });
}
