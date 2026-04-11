use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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
        if !self.local_mlx_repo_dir.exists() {
            panic!(
                "expected a local mlx-swift checkout at {}\nclone it with: git clone https://github.com/ml-explore/mlx-swift.git {}",
                self.local_mlx_repo_dir.display(),
                self.local_mlx_repo_dir.display()
            );
        }
        if !self.mlx_device_cpp_path().exists() {
            panic!(
                "expected mlx-swift submodules to be initialized under {}\nrun: git -C {} submodule update --init --recursive",
                self.local_mlx_repo_dir.display(),
                self.local_mlx_repo_dir.display()
            );
        }
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

        run_checked_command(&mut command, "xcrun swift build for summary bridge");
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

        run_checked_command(&mut command, "xcodebuild for MLX metallib");

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
            "cargo:rustc-env=SMRZE_MLX_RUNTIME_ASSET_SHA256={}",
            sha256_file(metallib_path)
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
        let mut command = Command::new("xcrun");
        command.arg("metal").arg("-v");
        let output = command_output(&mut command, "xcrun metal -v")
            .unwrap_or_else(|error| panic!("failed to run xcrun metal -v: {error}"));
        if output.status.success() {
            return;
        }

        panic!(
            "the Metal Toolchain is required to build MLX Gemma summaries\nstdout:\n{}\nstderr:\n{}\ninstall it with: xcodebuild -downloadComponent MetalToolchain",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    fn cargo_profile_dir(&self) -> PathBuf {
        self.out_dir
            .ancestors()
            .nth(3)
            .unwrap_or_else(|| panic!("failed to resolve Cargo profile directory from OUT_DIR"))
            .to_path_buf()
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
        match self.target_arch.as_str() {
            "aarch64" => "arm64",
            "x86_64" => "x86_64",
            other => panic!("unsupported macOS arch for xcodebuild: {other}"),
        }
    }

    fn current_swift_triple_dir(&self) -> &'static str {
        match self.target_arch.as_str() {
            "aarch64" => "aarch64-apple-macosx",
            "x86_64" => "x86_64-apple-macosx",
            other => panic!("unsupported macOS arch for swift bridge: {other}"),
        }
    }

    fn xcode_build_configuration(&self) -> &'static str {
        if self.release_build {
            "Release"
        } else {
            "Debug"
        }
    }

    fn mlx_repo_revision(&self) -> String {
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(&self.local_mlx_repo_dir)
            .arg("rev-parse")
            .arg("HEAD");
        let output = run_checked_command(&mut command, "git rev-parse HEAD for mlx-swift");

        String::from_utf8(output.stdout)
            .unwrap_or_else(|_| panic!("local mlx-swift revision should be utf-8"))
            .trim()
            .to_owned()
    }
}

fn required_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("missing {name}"))
}

fn required_env_path(name: &str) -> PathBuf {
    PathBuf::from(required_env(name))
}

fn developer_dir() -> String {
    let mut command = Command::new("xcode-select");
    command.arg("--print-path");
    command_output(&mut command, "xcode-select --print-path")
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|output| output.trim().to_owned())
        .filter(|output| !output.is_empty())
        .unwrap_or_else(|| "/Applications/Xcode.app/Contents/Developer".to_owned())
}

fn run_checked_command(command: &mut Command, action: &str) -> Output {
    let output = command_output(command, action)
        .unwrap_or_else(|error| panic!("failed to run {action}: {error}"));
    if output.status.success() {
        return output;
    }

    panic!(
        "{action} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn command_output(command: &mut Command, action: &str) -> Result<Output, std::io::Error> {
    command
        .output()
        .map_err(|error| std::io::Error::new(error.kind(), format!("{action}: {error}")))
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

fn copy_file(source_path: &Path, output_path: &Path) {
    fs::copy(source_path, output_path).unwrap_or_else(|error| {
        panic!(
            "failed to copy {} to {}: {error}",
            source_path.display(),
            output_path.display()
        )
    });
}

fn sha256_file(path: &Path) -> String {
    let mut file = fs::File::open(path)
        .unwrap_or_else(|error| panic!("failed to open {}: {error}", path.display()));
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    format!("{:x}", hasher.finalize())
}
