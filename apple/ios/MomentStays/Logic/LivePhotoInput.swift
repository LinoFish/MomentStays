import SwiftUI
import Photos
import PhotosUI

enum LiveInputError: LocalizedError {
    case access, notLive, originalUnavailable
    var errorDescription: String? {
        switch self {
        case .access: return String(localized: "Allow access to this photo in Settings, then try again.")
        case .notLive: return String(localized: "This photo doesn't move. Choose a Live Photo.")
        case .originalUnavailable: return String(localized: "Can't read the original photo or its video. Make sure the originals are downloaded, then try again.")
        }
    }
}
struct LivePhotoInput {
    struct Pair { let photo: URL; let video: URL }
    static func extract(asset: PHAsset) async throws -> Pair {
        let resources = PHAssetResource.assetResources(for: asset)
        guard let still = resources.first(where: { $0.type == .photo }),
              let movie = resources.first(where: { $0.type == .pairedVideo }) else { throw LiveInputError.originalUnavailable }
        let folder = try PhotoSession.directory()
        let stillExtension = URL(fileURLWithPath: still.originalFilename).pathExtension
        let videoExtension = URL(fileURLWithPath: movie.originalFilename).pathExtension
        // Matching basenames help engines identify the still/movie as one pair.
        let photo = folder.appendingPathComponent("LivePhoto").appendingPathExtension(stillExtension.isEmpty ? "heic" : stillExtension)
        let video = folder.appendingPathComponent("LivePhoto").appendingPathExtension(videoExtension.isEmpty ? "mov" : videoExtension)
        do {
            try await write(still, to: photo)
            try await write(movie, to: video)
            return Pair(photo: photo, video: video)
        } catch {
            try? FileManager.default.removeItem(at: folder)
            throw LiveInputError.originalUnavailable
        }
    }
    static func extractOriginalPhoto(asset: PHAsset) async throws -> URL {
        guard let still = PHAssetResource.assetResources(for: asset).first(where: { $0.type == .photo }) else { throw LiveInputError.originalUnavailable }
        let folder = try PhotoSession.directory()
        let ext = URL(fileURLWithPath: still.originalFilename).pathExtension
        let photo = folder.appendingPathComponent("Original").appendingPathExtension(ext.isEmpty ? "jpg" : ext)
        do { try await write(still, to: photo); return photo }
        catch { PhotoSession.remove(containing: photo); throw LiveInputError.originalUnavailable }
    }
    private static func write(_ resource: PHAssetResource, to url: URL) async throws {
        let options = PHAssetResourceRequestOptions()
        // Never silently download cloud-only originals.
        options.isNetworkAccessAllowed = false
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            PHAssetResourceManager.default().writeData(for: resource, toFile: url, options: options) { error in
                if let error { continuation.resume(throwing: error) } else { continuation.resume() }
            }
        }
    }
}

struct LivePhotoPreview: View {
    let photo: URL
    let video: URL
    @State private var livePhoto: PHLivePhoto?
    @State private var failed = false
    var body: some View {
        Group {
            if let livePhoto { LivePhotoPlayer(livePhoto: livePhoto) }
            else if let image = UIImage(contentsOfFile: photo.path) {
                Image(uiImage: image).resizable().scaledToFit().overlay(alignment: .bottom) {
                    if failed { Text("Live Photo preview unavailable").font(.caption).padding(8).background(.regularMaterial, in: Capsule()).padding(8) }
                }
            } else { ProgressView() }
        }.task(id: photo) {
            PHLivePhoto.request(withResourceFileURLs: [photo, video], placeholderImage: nil, targetSize: .zero, contentMode: .aspectFit) { value, info in
                let degraded = (info[PHLivePhotoInfoIsDegradedKey] as? Bool) ?? false
                DispatchQueue.main.async {
                    if let value { livePhoto = value }
                    else if !degraded { failed = true }
                }
            }
        }.accessibilityLabel("Live Photo preview")
    }
}
struct LivePhotoPlayer: UIViewRepresentable {
    let livePhoto: PHLivePhoto
    func makeUIView(context: Context) -> PHLivePhotoView {
        let view = PHLivePhotoView()
        view.contentMode = .scaleAspectFit
        return view
    }
    func updateUIView(_ view: PHLivePhotoView, context: Context) { view.livePhoto = livePhoto }
}

// Present from the active view hierarchy, not an arbitrary connected scene.
struct LimitedLibraryManager: UIViewControllerRepresentable {
    func makeUIViewController(context: Context) -> LimitedPickerController { LimitedPickerController() }
    func updateUIViewController(_ controller: LimitedPickerController, context: Context) {}
    final class LimitedPickerController: UIViewController {
        private var presented = false
        override func viewDidAppear(_ animated: Bool) {
            super.viewDidAppear(animated)
            guard !presented else { return }
            presented = true
            PHPhotoLibrary.shared().presentLimitedLibraryPicker(from: self) { [weak self] _ in
                DispatchQueue.main.async { self?.dismiss(animated: true) }
            }
        }
    }
}
