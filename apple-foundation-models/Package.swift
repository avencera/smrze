// swift-tools-version: 6.0

import PackageDescription

let package = Package(
    name: "smrze-foundation-models",
    platforms: [
        .macOS("26.0"),
    ],
    products: [
        .executable(
            name: "smrze-foundation-models",
            targets: ["SmrzeFoundationModels"]
        ),
    ],
    targets: [
        .executableTarget(
            name: "SmrzeFoundationModels"
        ),
        .testTarget(
            name: "SmrzeFoundationModelsTests",
            dependencies: ["SmrzeFoundationModels"]
        ),
    ]
)
