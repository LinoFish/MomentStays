#if canImport(Photos)
import Foundation
import Photos

/// Uses read/write authorization and refuses network downloads during scanning.
public struct PhotosLibrarySource: PhotoScanSource {
    public init() {}

    public func requestAccess() async -> PhotoAccess {
        let status = await withCheckedContinuation { continuation in
            PHPhotoLibrary.requestAuthorization(for: .readWrite) { continuation.resume(returning: $0) }
        }
        switch status {
        case .authorized: return .authorized
        case .limited: return .limited
        case .denied: return .denied
        case .restricted: return .restricted
        case .notDetermined: return .notDetermined
        @unknown default: return .restricted
        }
    }

    public func assetIdentifiers() async throws -> [String] {
        try Task.checkCancellation()
        let assets = PHAsset.fetchAssets(with: .image, options: nil)
        var identifiers: [String] = []
        assets.enumerateObjects { asset, _, stop in
            if Task.isCancelled { stop.pointee = true; return }
            identifiers.append(asset.localIdentifier)
        }
        try Task.checkCancellation()
        return identifiers
    }

    public func originalResources(for identifier: String) async throws -> [PhotoOriginalResource] {
        try Task.checkCancellation()
        guard let asset = PHAsset.fetchAssets(withLocalIdentifiers: [identifier], options: nil).firstObject else {
            throw OriginalReadError.unavailable
        }
        // .photo is the original image, unlike .fullSizePhoto (an edited rendering).
        let resources = PHAssetResource.assetResources(for: asset).filter { $0.type == .photo }
        guard !resources.isEmpty else { throw OriginalReadError.unavailable }
        var data: [PhotoOriginalResource] = []
        for resource in resources {
            data.append(PhotoOriginalResource(data: try await read(resource), filename: resource.originalFilename, uniformTypeIdentifier: resource.uniformTypeIdentifier))
        }
        return data
    }

    private func read(_ resource: PHAssetResource) async throws -> Data {
        let request = ResourceRequest()
        return try await withTaskCancellationHandler(operation: {
            try await withCheckedThrowingContinuation { continuation in
                request.start(resource: resource, continuation: continuation)
            }
        }, onCancel: { request.cancel() })
    }
}

private enum OriginalReadError: Error { case unavailable }

/// Photos callbacks may arrive on arbitrary queues. The lock serializes cancellation,
/// request installation, byte accumulation and completion.
private final class ResourceRequest: @unchecked Sendable {
    private let lock = NSLock()
    private let manager = PHAssetResourceManager.default()
    private var requestID: PHAssetResourceDataRequestID?
    private var cancelled = false
    private var bytes = Data()

    func start(resource: PHAssetResource, continuation: CheckedContinuation<Data, Error>) {
        lock.lock()
        if cancelled {
            lock.unlock()
            continuation.resume(throwing: CancellationError())
            return
        }
        lock.unlock()
        let options = PHAssetResourceRequestOptions()
        options.isNetworkAccessAllowed = false
        let id = manager.requestData(for: resource, options: options, dataReceivedHandler: { [self] chunk in
            lock.lock(); bytes.append(chunk); lock.unlock()
        }, completionHandler: { [self] error in
            lock.lock()
            let wasCancelled = cancelled
            let result = bytes
            lock.unlock()
            if wasCancelled { continuation.resume(throwing: CancellationError()) }
            else if let error = error { continuation.resume(throwing: error) }
            else { continuation.resume(returning: result) }
        })
        lock.lock()
        requestID = id
        let shouldCancel = cancelled
        lock.unlock()
        if shouldCancel { manager.cancelDataRequest(id) }
    }

    func cancel() {
        lock.lock()
        cancelled = true
        let id = requestID
        lock.unlock()
        if let id = id { manager.cancelDataRequest(id) }
    }
}
#endif
