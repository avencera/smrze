mod command;
mod error;
mod hash;
mod mlx;
mod paths;
mod search;

pub use command::{developer_dir, run_checked_command};
pub use error::{BuildSupportError, Result};
pub use hash::blake3_file;
pub use mlx::{
    build_mlx_metallib, ensure_local_mlx_repo, ensure_metal_toolchain, mlx_device_cpp_path,
    mlx_repo_revision, mlx_xcode_project_path,
};
pub use paths::{
    cargo_profile_dir, current_runtime_arch_dir, current_xcode_arch, swift_triple_dir_for_target,
    xcode_arch_for_target,
};
pub use search::find_file_named;

#[cfg(test)]
mod tests {
    use super::{
        blake3_file, cargo_profile_dir, ensure_local_mlx_repo, find_file_named,
        mlx_device_cpp_path, swift_triple_dir_for_target, xcode_arch_for_target,
    };
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("smrze-build-support-{name}-{unique}"))
    }

    #[test]
    fn xcode_arch_matches_supported_targets() {
        assert_eq!(xcode_arch_for_target("aarch64").unwrap(), "arm64");
        assert_eq!(xcode_arch_for_target("x86_64").unwrap(), "x86_64");
    }

    #[test]
    fn swift_triple_matches_supported_targets() {
        assert_eq!(
            swift_triple_dir_for_target("aarch64").unwrap(),
            "aarch64-apple-macosx"
        );
        assert_eq!(
            swift_triple_dir_for_target("x86_64").unwrap(),
            "x86_64-apple-macosx"
        );
    }

    #[test]
    fn cargo_profile_dir_walks_up_from_out_dir() {
        let out_dir = Path::new("/tmp/target/debug/build/smrze/out");
        assert_eq!(
            cargo_profile_dir(out_dir).unwrap(),
            PathBuf::from("/tmp/target/debug")
        );
    }

    #[test]
    fn ensure_local_mlx_repo_requires_submodule_checkout() {
        let root = temp_dir("mlx-checkout");
        fs::create_dir_all(&root).unwrap();
        let error = ensure_local_mlx_repo(&root).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("submodule update --init --recursive")
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn mlx_device_cpp_path_matches_repo_layout() {
        let root = PathBuf::from("/tmp/mlx-swift");
        assert_eq!(
            mlx_device_cpp_path(&root),
            root.join("Source/Cmlx/mlx/mlx/backend/metal/device.cpp")
        );
    }

    #[test]
    fn find_file_named_returns_nested_match() {
        let root = temp_dir("find-file");
        let nested = root.join("a/b");
        fs::create_dir_all(&nested).unwrap();
        let target = nested.join("needle.txt");
        fs::write(&target, "hello").unwrap();

        assert_eq!(find_file_named(&root, "needle.txt"), Some(target));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn blake3_file_hashes_file_contents() {
        let root = temp_dir("blake3");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("file.txt");
        fs::write(&path, "abc").unwrap();

        assert_eq!(
            blake3_file(&path).unwrap(),
            "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85"
        );
        let _ = fs::remove_dir_all(&root);
    }
}
