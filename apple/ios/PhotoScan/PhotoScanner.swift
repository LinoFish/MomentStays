import Foundation
#if canImport(UniformTypeIdentifiers)
import UniformTypeIdentifiers
#endif

/// Exact bytes and Photos metadata for one original still resource.
public struct PhotoOriginalResource: Sendable, Equatable {
    public let data: Data
    public let filename: String
    public let uniformTypeIdentifier: String
    public init(data: Data, filename: String, uniformTypeIdentifier: String) {
        self.data = data; self.filename = filename; self.uniformTypeIdentifier = uniformTypeIdentifier
    }

    /// Unique local file name, retaining the original extension (or deriving it from the UTI).
    public func temporaryFileURL(in directory: URL = FileManager.default.temporaryDirectory) -> URL {
        let name = (filename as NSString).lastPathComponent
        var safeName = name.isEmpty || name == "." || name == ".." ? "original" : name
        #if canImport(UniformTypeIdentifiers)
        if (safeName as NSString).pathExtension.isEmpty,
           let ext = UTType(uniformTypeIdentifier)?.preferredFilenameExtension {
            safeName += "." + ext
        }
        #endif
        return directory.appendingPathComponent(UUID().uuidString + "-" + safeName)
    }
}

public enum PhotoAccess: Sendable, Equatable {
    case authorized, limited, denied, restricted, notDetermined
}

/// The source must return unmodified original still-image resources, not rendered images.
public protocol PhotoScanSource: Sendable {
    func requestAccess() async -> PhotoAccess
    func assetIdentifiers() async throws -> [String]
    func originalResources(for identifier: String) async throws -> [PhotoOriginalResource]
}

/// Return true only when the bytes are positively recognized as an Android Motion Photo.
/// Throw for an inconclusive or failed inspection; never turn an error into a match.
public typealias MotionPhotoRecognizer = @Sendable (PhotoOriginalResource) async throws -> Bool

public enum PhotoScanIssue: Sendable, Equatable {
    /// Original could not be read locally (including an iCloud-only original).
    case originalUnavailable
    case recognitionFailed
}

public struct PhotoResourceIdentity: Sendable, Equatable {
    public let filename: String
    public let uniformTypeIdentifier: String
    public let resourceIndex: Int
    public init(filename: String, uniformTypeIdentifier: String, resourceIndex: Int) {
        self.filename = filename; self.uniformTypeIdentifier = uniformTypeIdentifier; self.resourceIndex = resourceIndex
    }
}

public struct PhotoScanProgress: Sendable, Equatable {
    public let access: PhotoAccess
    /// Number of assets processed, including those that could not be inspected.
    public let completed: Int
    public let total: Int
    public let motionAssetIdentifiers: [String]
    public let issues: [String: PhotoScanIssue]
    /// The original filename and UTI of the specific resource that matched.
    public let matchedResources: [String: PhotoResourceIdentity]
}

public struct PhotoScanResult: Sendable {
    public let access: PhotoAccess
    public let motionAssetIdentifiers: [String]
    public let issues: [String: PhotoScanIssue]
    /// The original filename and UTI of the specific resource that matched.
    public let matchedResources: [String: PhotoResourceIdentity]
}

public struct PhotoScanner: Sendable {
    private let source: any PhotoScanSource
    private let recognizer: MotionPhotoRecognizer

    public init(source: any PhotoScanSource, recognizer: @escaping MotionPhotoRecognizer) {
        self.source = source
        self.recognizer = recognizer
    }

    /// Progress is invoked on a background executor; consumers must hop to the main actor for UI.
    /// Cancelling the caller cancels the scan and throws CancellationError instead of partial results.
    public func scan(progress: @escaping @Sendable (PhotoScanProgress) async -> Void = { _ in }) async throws -> PhotoScanResult {
        let worker = Task.detached(priority: .utility) {
            try await runScan(progress: progress)
        }
        return try await withTaskCancellationHandler(operation: {
            try await worker.value
        }, onCancel: {
            worker.cancel()
        })
    }

    private func runScan(progress: @Sendable (PhotoScanProgress) async -> Void) async throws -> PhotoScanResult {
        try Task.checkCancellation()
        let access = await source.requestAccess()
        try Task.checkCancellation()
        guard access == .authorized || access == .limited else {
            return PhotoScanResult(access: access, motionAssetIdentifiers: [], issues: [:], matchedResources: [:])
        }
        let identifiers = try await source.assetIdentifiers()
        var matches: [String] = []
        var issues: [String: PhotoScanIssue] = [:]
        var matchedResources: [String: PhotoResourceIdentity] = [:]
        await progress(PhotoScanProgress(access: access, completed: 0, total: identifiers.count, motionAssetIdentifiers: [], issues: [:], matchedResources: [:]))
        for (index, identifier) in identifiers.enumerated() {
            try Task.checkCancellation()
            do {
                let resources = try await source.originalResources(for: identifier)
                try Task.checkCancellation()
                if resources.isEmpty {
                    issues[identifier] = .originalUnavailable
                } else {
                    var matched: (Int, PhotoOriginalResource)?
                    var failed = false
                    for (index, resource) in resources.enumerated() {
                        try Task.checkCancellation()
                        do {
                            if try await recognizer(resource) { matched = (index, resource); break }
                        } catch is CancellationError { throw CancellationError() }
                        catch { failed = true }
                    }
                    try Task.checkCancellation()
                    if let matched {
                        matches.append(identifier)
                        matchedResources[identifier] = PhotoResourceIdentity(filename: matched.1.filename, uniformTypeIdentifier: matched.1.uniformTypeIdentifier, resourceIndex: matched.0)
                    }
                    else if failed { issues[identifier] = .recognitionFailed }
                }
            } catch is CancellationError { throw CancellationError() }
            catch { issues[identifier] = .originalUnavailable }
            try Task.checkCancellation()
            await progress(PhotoScanProgress(access: access, completed: index + 1, total: identifiers.count, motionAssetIdentifiers: matches, issues: issues, matchedResources: matchedResources))
        }
        return PhotoScanResult(access: access, motionAssetIdentifiers: matches, issues: issues, matchedResources: matchedResources)
    }
}
