// swift-tools-version: 6.0

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
    targets: [
        .target(
            name: "SmrzeFoundationModels"
        ),
        .testTarget(
            name: "SmrzeFoundationModelsTests",
            dependencies: ["SmrzeFoundationModels"]
        ),
    ]
)
