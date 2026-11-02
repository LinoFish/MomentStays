// swift-tools-version: 5.7
import PackageDescription

let package = Package(
    name: "ConversionInterface",
    products: [.library(name: "ConversionInterface", targets: ["ConversionInterface"])],
    targets: [
        .target(name: "ConversionInterface", path: ".", exclude: ["README.md", "Tests", "BuildHelpers"], sources: ["ConversionEngine.swift", "BatchConversionCoordinator.swift", "NativeConversionAdapter.swift"]),
        .testTarget(name: "ConversionInterfaceTests", dependencies: ["ConversionInterface"], path: "Tests")
    ]
)
