// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "SourceDictation",
    platforms: [.macOS(.v15)],
    dependencies: [
        .package(url: "https://github.com/FluidInference/FluidAudio.git", from: "0.14.1"),
    ],
    targets: [
        .executableTarget(
            name: "SourceDictation",
            dependencies: [.product(name: "FluidAudio", package: "FluidAudio")],
            path: "Sources/SourceDictation",
            exclude: ["engine_stub.swift"]
        ),
    ]
)
