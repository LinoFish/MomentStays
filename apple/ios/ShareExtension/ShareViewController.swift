import SwiftUI
import UniformTypeIdentifiers
import Photos

final class ShareViewController: UIViewController {
    override func viewDidLoad() {
        super.viewDidLoad()
        let host = UIHostingController(rootView: ShareHandoffView(context: extensionContext))
        addChild(host); host.view.frame = view.bounds; host.view.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        view.addSubview(host.view); host.didMove(toParent: self)
    }
}

private struct ShareHandoffView: View {
    let context: NSExtensionContext?
    @State private var status: Status = .loading
    private enum Status { case loading, ready, failed, unavailable, pending }
    var body: some View {
        VStack(spacing: 24) {
            HStack { Button("Cancel") { context?.cancelRequest(withError: CancellationError()) }; Spacer(); Text("Convert with MomentStays").font(.headline); Spacer() }
            Spacer(minLength: 0)
            switch status {
            case .loading: ProgressView("Reading…")
            case .ready:
                Image(systemName: "checkmark.circle").font(.largeTitle)
                Text("Sent to MomentStays. Open MomentStays to continue.").multilineTextAlignment(.center)
                Button("OK") { context?.completeRequest(returningItems: nil) }.buttonStyle(.borderedProminent)
            case .pending:
                Text("A photo is already waiting in MomentStays. Open the app to continue.").multilineTextAlignment(.center)
                Button("OK") { context?.completeRequest(returningItems: nil) }.buttonStyle(.borderedProminent)
            case .unavailable:
                Text("Couldn't prepare this item. Try again in MomentStays.").multilineTextAlignment(.center)
                Button("OK") { context?.completeRequest(returningItems: nil) }.buttonStyle(.borderedProminent)
            case .failed:
                Image(systemName: "exclamationmark.triangle.fill").font(.largeTitle).foregroundStyle(.orange)
                Text("Couldn't read this item. Try selecting the original photo in MomentStays.").multilineTextAlignment(.center)
                Button("OK") { context?.completeRequest(returningItems: nil) }.buttonStyle(.borderedProminent)
            }
            Spacer(minLength: 0)
            Label("Processed on this iPhone", systemImage: "lock.shield").font(.footnote).foregroundStyle(.secondary)
        }.padding(20).tint(.orange).task { await prepare() }
    }
    private func prepare() async {
        guard let provider = (context?.inputItems.first as? NSExtensionItem)?.attachments?.first else { status = .failed; return }
        guard let container = FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: "group.com.app9x3k2q.momentstays") else { status = .unavailable; return }
        guard !FileManager.default.fileExists(atPath: container.appendingPathComponent("pending.json").path) else { status = .pending; return }
        // Recover abandoned staging directories without touching a pending handoff.
        let cutoff = Date().addingTimeInterval(-24 * 60 * 60)
        if let entries = try? FileManager.default.contentsOfDirectory(at: container, includingPropertiesForKeys: [.contentModificationDateKey]) {
            for entry in entries where entry.lastPathComponent.hasPrefix("Handoff-") {
                if let modified = try? entry.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate, modified < cutoff {
                    try? FileManager.default.removeItem(at: entry)
                }
            }
        }
        let isLive = provider.hasItemConformingToTypeIdentifier(UTType.livePhoto.identifier)
        let type = isLive ? UTType.livePhoto.identifier : (provider.hasItemConformingToTypeIdentifier(UTType.heic.identifier) ? UTType.heic.identifier : UTType.jpeg.identifier)
        guard provider.hasItemConformingToTypeIdentifier(type) else { status = .failed; return }
        let directory = container.appendingPathComponent("Handoff-\(UUID().uuidString)", isDirectory: true)
        var published = false
        defer { if !published { try? FileManager.default.removeItem(at: directory) } }
        do {
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
            // The provider's URL is only valid during its callback, so copy there.
            let names: [String]
            if isLive {
                names = try await copyLivePhoto(provider: provider, to: directory)
            } else {
                names = try await withCheckedThrowingContinuation { continuation in
                provider.loadFileRepresentation(forTypeIdentifier: type) { url, error in
                    do {
                        guard let url, error == nil else { throw error ?? CocoaError(.fileReadUnknown) }
                        let candidates: [URL]
                        var isDirectory: ObjCBool = false
                        if FileManager.default.fileExists(atPath: url.path, isDirectory: &isDirectory), isDirectory.boolValue {
                            candidates = try FileManager.default.contentsOfDirectory(at: url, includingPropertiesForKeys: nil)
                        } else { candidates = [url] }
                        let photos = candidates.filter { ["jpg", "jpeg", "heic", "heif"].contains($0.pathExtension.lowercased()) }
                        let movies = candidates.filter { ["mov", "mp4"].contains($0.pathExtension.lowercased()) }
                        guard let photo = photos.first, !isLive || !movies.isEmpty else { throw CocoaError(.fileReadUnsupportedScheme) }
                        let files = [photo] + (isLive ? [movies[0]] : [])
                        var copied: [String] = []
                        for (index, file) in files.enumerated() {
                            let name = "item\(index).\(file.pathExtension)"
                            try FileManager.default.copyItem(at: file, to: directory.appendingPathComponent(name)); copied.append(name)
                        }
                        continuation.resume(returning: copied)
                    } catch { try? FileManager.default.removeItem(at: directory); continuation.resume(throwing: error) }
                }
            }
            }
            try Task.checkCancellation()
            let manifest = ["directory": directory.lastPathComponent, "photo": names[0], "video": names.count > 1 ? names[1] : ""]
            let data = try JSONSerialization.data(withJSONObject: manifest)
            try data.write(to: container.appendingPathComponent("pending.json"), options: .atomic)
            published = true
            status = .ready
        } catch { status = .unavailable }
    }
    private func copyLivePhoto(provider: NSItemProvider, to directory: URL) async throws -> [String] {
        let live: PHLivePhoto = try await withCheckedThrowingContinuation { continuation in
            provider.loadItem(forTypeIdentifier: UTType.livePhoto.identifier, options: nil) { object, error in
                if let object = object as? PHLivePhoto { continuation.resume(returning: object) }
                else { continuation.resume(throwing: error ?? CocoaError(.fileReadUnknown)) }
            }
        }
        let resources = PHAssetResource.assetResources(for: live)
        guard let photo = resources.first(where: { $0.type == .photo }),
              let video = resources.first(where: { $0.type == .pairedVideo }) else { throw CocoaError(.fileReadUnknown) }
        var names: [String] = []
        for (index, resource) in [photo, video].enumerated() {
            let ext = URL(fileURLWithPath: resource.originalFilename).pathExtension
            let name = "item\(index).\(ext.isEmpty ? (index == 0 ? "jpg" : "mov") : ext)"
            let destination = directory.appendingPathComponent(name)
            let options = PHAssetResourceRequestOptions()
            options.isNetworkAccessAllowed = false
            try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
                PHAssetResourceManager.default().writeData(for: resource, toFile: destination, options: options) { error in
                    if let error { continuation.resume(throwing: error) } else { continuation.resume() }
                }
            }
            names.append(name)
        }
        return names
    }

}
