// swift-tools-version: 6.1

import PackageDescription

let package = Package(
    name: "smrze-foundation-models",
    platforms: [
        .macOS("26.0"),
    ],
    products: [
        .library(
            name: "SmrzeFoundationModels",
            type: .static,
            targets: ["SmrzeFoundationModels"]
        ),
    ],
    dependencies: [
        .package(url: "https://github.com/ml-explore/mlx-swift.git", .upToNextMinor(from: "0.31.3")),
        .package(url: "https://github.com/ml-explore/mlx-swift-lm.git", branch: "main"),
        .package(url: "https://github.com/huggingface/swift-transformers.git", branch: "main"),
        .package(url: "https://github.com/DePasqualeOrg/swift-hf-api.git", from: "0.2.2"),
    ],
    targets: [
        .target(
            name: "SmrzeFoundationModels",
            dependencies: [
                .product(name: "MLXLLM", package: "mlx-swift-lm"),
                .product(name: "MLXLMCommon", package: "mlx-swift-lm"),
                .product(name: "MLX", package: "mlx-swift"),
                .product(name: "MLXNN", package: "mlx-swift"),
                .product(name: "Tokenizers", package: "swift-transformers"),
                .product(name: "HFAPI", package: "swift-hf-api"),
            ]
        ),
        .testTarget(
            name: "SmrzeFoundationModelsTests",
            dependencies: ["SmrzeFoundationModels"]
        ),
    ]
)
