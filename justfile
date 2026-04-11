fmt:
    cargo fmt

clippy:
    cargo clippy

check:
    cargo check

test:
    cargo test

fix:
    cargo clippy --fix --allow-dirty

release place="local":
    cargo xtask release {{place}}

publish-mlx-metallib:
    cargo xtask publish-mlx-metallib
