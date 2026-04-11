use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/foundation_models_bridge.rs");
    println!("cargo:rerun-if-changed=apple-foundation-models/Package.swift");
    println!("cargo:rerun-if-changed=apple-foundation-models/Package.resolved");
    println!("cargo:rerun-if-changed=apple-foundation-models/Sources/SmrzeFoundationModels");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }

    register_local_mlx_inputs();
    ensure_local_mlx_repo();

    generate_swift_bridge();
    compile_swift_library();
    let metallib_path = compile_mlx_metallib();
    copy_mlx_metallib(&metallib_path, &cargo_profile_dir().join("mlx.metallib"));
    export_mlx_runtime_metadata(&metallib_path);
    link_swift_library();
}

fn generate_swift_bridge() {
    swift_bridge_build::parse_bridges(vec!["src/foundation_models_bridge.rs"])
        .write_all_concatenated(generated_code_dir(), env!("CARGO_PKG_NAME"));
}

fn compile_swift_library() {
    let package_path = manifest_dir().join("apple-foundation-models");
    let source_dir = swift_source_dir();
    let bridging_header = source_dir.join("bridging-header.h");
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").expect("missing target arch");

    let mut command = Command::new("xcrun");
    command
        .arg("swift")
        .arg("build")
        .arg("--package-path")
        .arg(&package_path)
        .arg("--product")
        .arg("SmrzeFoundationModels")
        .arg("--arch")
        .arg(&arch)
        .arg("-Xswiftc")
        .arg("-static")
        .arg("-Xswiftc")
        .arg("-import-objc-header")
        .arg("-Xswiftc")
        .arg(&bridging_header);

    if is_release_build() {
        command.arg("-c").arg("release");
    }

    let output = command
        .output()
        .expect("failed to run xcrun swift build for summary bridge");
    if output.status.success() {
        return;
    }

    panic!(
        "xcrun swift build failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn link_swift_library() {
    println!("cargo:rustc-link-lib=static=SmrzeFoundationModels");
    println!(
        "cargo:rustc-link-search=native={}",
        swift_library_dir().display()
    );
    println!("cargo:rustc-link-lib=framework=Foundation");
    println!("cargo:rustc-link-lib=framework=FoundationModels");
    println!("cargo:rustc-link-lib=framework=BackgroundAssets");

    let xcode_path = Command::new("xcode-select")
        .arg("--print-path")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|output| output.trim().to_owned())
        .filter(|output| !output.is_empty())
        .unwrap_or_else(|| "/Applications/Xcode.app/Contents/Developer".to_owned());
    let swift_runtime_dir =
        format!("{xcode_path}/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift/macosx");
    println!("cargo:rustc-link-search={swift_runtime_dir}");
    println!("cargo:rustc-link-arg=-Wl,-rpath,{swift_runtime_dir}");
    println!("cargo:rustc-link-search=/usr/lib/swift");
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
}

fn compile_mlx_metallib() -> PathBuf {
    ensure_metal_toolchain();

    let mut command = Command::new("xcodebuild");
    command
        .arg("build")
        .arg("-project")
        .arg(mlx_xcode_project_path())
        .arg("-scheme")
        .arg("Cmlx")
        .arg("-configuration")
        .arg(xcode_build_configuration())
        .arg("-destination")
        .arg(format!("platform=macOS,arch={}", current_xcode_arch()))
        .arg("-derivedDataPath")
        .arg(mlx_derived_data_dir());

    let output = command
        .output()
        .expect("failed to run xcodebuild for MLX metallib");
    if !output.status.success() {
        panic!(
            "xcodebuild failed while building the MLX metallib\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    find_file_named(&mlx_build_products_dir(), "default.metallib").unwrap_or_else(|| {
        panic!(
            "failed to locate default.metallib under {} after building MLX",
            mlx_build_products_dir().display()
        )
    })
}

fn export_mlx_runtime_metadata(metallib_path: &Path) {
    println!(
        "cargo:rustc-env=SMRZE_MLX_RUNTIME_ASSET_VERSION={}",
        mlx_repo_revision()
    );
    println!(
        "cargo:rustc-env=SMRZE_MLX_RUNTIME_ASSET_SHA256={}",
        sha256_file(metallib_path)
    );
}

fn ensure_metal_toolchain() {
    let output = Command::new("xcrun")
        .arg("metal")
        .arg("-v")
        .output()
        .expect("failed to check for the Metal Toolchain with xcrun metal -v");
    if output.status.success() {
        return;
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    panic!(
        "the Metal Toolchain is required to build MLX Gemma summaries\nstdout:\n{}\nstderr:\n{}\ninstall it with: xcodebuild -downloadComponent MetalToolchain",
        stdout, stderr
    );
}

fn ensure_local_mlx_repo() {
    let repo_dir = local_mlx_repo_dir();
    if !repo_dir.exists() {
        panic!(
            "expected a local mlx-swift checkout at {}\nclone it with: git clone https://github.com/ml-explore/mlx-swift.git {}",
            repo_dir.display(),
            repo_dir.display()
        );
    }
    if !mlx_device_cpp_path().exists() {
        panic!(
            "expected mlx-swift submodules to be initialized under {}\nrun: git -C {} submodule update --init --recursive",
            repo_dir.display(),
            repo_dir.display()
        );
    }
}

fn register_local_mlx_inputs() {
    for path in [
        local_mlx_repo_dir().join("Package.swift"),
        local_mlx_repo_dir().join("xcode").join("MLX.xcodeproj"),
        local_mlx_repo_dir()
            .join("xcode")
            .join("xcconfig")
            .join("Cmlx.xcconfig"),
        mlx_device_cpp_path(),
    ] {
        if path.exists() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("missing manifest dir"))
}

fn cargo_profile_dir() -> PathBuf {
    out_dir()
        .ancestors()
        .nth(3)
        .expect("failed to resolve Cargo profile directory from OUT_DIR")
        .to_path_buf()
}

fn out_dir() -> PathBuf {
    PathBuf::from(std::env::var("OUT_DIR").expect("missing OUT_DIR"))
}

fn local_mlx_repo_dir() -> PathBuf {
    manifest_dir()
        .parent()
        .expect("smrze manifest dir should have a parent")
        .join("mlx-swift")
}

fn mlx_device_cpp_path() -> PathBuf {
    local_mlx_repo_dir()
        .join("Source")
        .join("Cmlx")
        .join("mlx")
        .join("mlx")
        .join("backend")
        .join("metal")
        .join("device.cpp")
}

fn swift_source_dir() -> PathBuf {
    manifest_dir()
        .join("apple-foundation-models")
        .join("Sources")
        .join("SmrzeFoundationModels")
}

fn generated_code_dir() -> PathBuf {
    swift_source_dir().join("generated")
}

fn mlx_xcode_project_path() -> PathBuf {
    local_mlx_repo_dir().join("xcode").join("MLX.xcodeproj")
}

fn mlx_derived_data_dir() -> PathBuf {
    manifest_dir()
        .join("apple-foundation-models")
        .join(".build")
        .join("mlx-derived-data")
}

fn mlx_build_products_dir() -> PathBuf {
    mlx_derived_data_dir()
        .join("Build")
        .join("Products")
        .join(xcode_build_configuration())
}

fn swift_library_dir() -> PathBuf {
    let build_mode = if is_release_build() {
        "release"
    } else {
        "debug"
    };
    manifest_dir()
        .join("apple-foundation-models")
        .join(".build")
        .join(current_swift_triple_dir())
        .join(build_mode)
}

fn current_xcode_arch() -> &'static str {
    match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("aarch64") => "arm64",
        Ok("x86_64") => "x86_64",
        other => panic!("unsupported macOS arch for xcodebuild: {other:?}"),
    }
}

fn current_swift_triple_dir() -> &'static str {
    match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("aarch64") => "aarch64-apple-macosx",
        Ok("x86_64") => "x86_64-apple-macosx",
        other => panic!("unsupported macOS arch for swift bridge: {other:?}"),
    }
}

fn xcode_build_configuration() -> &'static str {
    if is_release_build() {
        "Release"
    } else {
        "Debug"
    }
}

fn is_release_build() -> bool {
    std::env::var("PROFILE").as_deref() == Ok("release")
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

fn copy_mlx_metallib(source_path: &Path, output_path: &Path) {
    fs::copy(source_path, output_path).unwrap_or_else(|error| {
        panic!(
            "failed to copy {} to {}: {error}",
            source_path.display(),
            output_path.display()
        )
    });
}

fn mlx_repo_revision() -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(local_mlx_repo_dir())
        .arg("rev-parse")
        .arg("HEAD")
        .output()
        .expect("failed to read local mlx-swift revision");
    if !output.status.success() {
        panic!(
            "git rev-parse HEAD failed for {}\nstdout:\n{}\nstderr:\n{}",
            local_mlx_repo_dir().display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    String::from_utf8(output.stdout)
        .expect("local mlx-swift revision should be utf-8")
        .trim()
        .to_owned()
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
