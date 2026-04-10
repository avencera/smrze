use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/foundation_models_bridge.rs");
    println!("cargo:rerun-if-changed=apple-foundation-models/Package.swift");
    println!("cargo:rerun-if-changed=apple-foundation-models/Sources/SmrzeFoundationModels");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }

    generate_swift_bridge();
    compile_swift_library();
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

fn manifest_dir() -> PathBuf {
    PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("missing manifest dir"))
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

fn current_swift_triple_dir() -> &'static str {
    match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("aarch64") => "aarch64-apple-macosx",
        Ok("x86_64") => "x86_64-apple-macosx",
        other => panic!("unsupported macOS arch for swift bridge: {other:?}"),
    }
}

fn is_release_build() -> bool {
    std::env::var("PROFILE").as_deref() == Ok("release")
}
