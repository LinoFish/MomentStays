// swift-tools-version: 5.7
import PackageDescription

let package = Package(
    name: "PhotoScan",
    platforms: [.iOS(.v14)],
    products: [.library(name: "PhotoScan", targets: ["PhotoScan"])],
    targets: [
        .target(name: "PhotoScan", path: ".", exclude: ["README.md", "Tests"], sources: ["PhotoScanner.swift", "PhotosLibrarySource.swift"]),
        .testTarget(name: "PhotoScanTests", dependencies: ["PhotoScan"], path: "Tests")
    ]
)
