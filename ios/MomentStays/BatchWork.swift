import SwiftUI
import Photos

/// Owns access to one user-selected folder until the browser and its work are finished.
final class BatchFolderAccess {
    let url: URL
    private let scoped: Bool

    init(url: URL) {
        self.url = url
        scoped = url.startAccessingSecurityScopedResource()
    }
    deinit { if scoped { url.stopAccessingSecurityScopedResource() } }

    /// Only immediate regular image files are offered; subfolders and symbolic links are excluded.
    func files() throws -> [URL] {
        let keys: Set<URLResourceKey> = [.isRegularFileKey, .isSymbolicLinkKey]
        return try FileManager.default.contentsOfDirectory(at: url, includingPropertiesForKeys: Array(keys), options: [.skipsHiddenFiles])
            .filter { item in
                guard let values = try? item.resourceValues(forKeys: keys), values.isRegularFile == true, values.isSymbolicLink != true else { return false }
                return ["jpg", "jpeg", "heic", "heif"].contains(item.pathExtension.lowercased())
            }.sorted { $0.lastPathComponent.localizedStandardCompare($1.lastPathComponent) == .orderedAscending }
    }
}

/// One row remains identifiable even if preparation or conversion fails.
struct BatchInput: Identifiable {
    enum Source {
        case folderFile(URL)
        case scannedPhoto(String, PhotoResourceIdentity)
        case livePhoto(String)
        case originalPhoto(String)
        case livePair(URL, URL)
    }
    let id: UUID
    let name: String
    let source: Source
    init(id: UUID = UUID(), name: String, source: Source) {
        self.id = id; self.name = name; self.source = source
    }
}

struct BatchRowResult: Identifiable {
    enum State {
        case pending
        case converting
        case failed(String)
        case converted(ConversionOutput)
        case cancelled
    }
    let id: UUID
    let name: String
    var state: State
}

/// Owns staged inputs and outputs until explicitly discarded, and survives in-flight callbacks.
@MainActor final class BatchWork: ObservableObject {
    @Published private(set) var rows: [BatchRowResult]
    @Published private(set) var completed = 0
    @Published private(set) var running = false
    @Published private(set) var checking = false
    @Published private(set) var eligibility: [UUID: String] = [:]
    private var prepared: [UUID: ([URL], Direction)] = [:]
    private let inputs: [BatchInput]
    private let engine: any ConversionEngine
    private let folder: BatchFolderAccess?
    private var directories: [URL] = []
    private var discardRequested = false
    private var stopRequested = false

    init(inputs: [BatchInput], engine: any ConversionEngine, folder: BatchFolderAccess? = nil) {
        self.inputs = inputs; self.engine = engine; self.folder = folder
        rows = inputs.map { BatchRowResult(id: $0.id, name: $0.name, state: .pending) }
    }

    func checkInputs() async {
        guard !checking, !running else { return }
        checking = true
        for item in inputs {
            if Task.isCancelled || discardRequested { break }
            do { prepared[item.id] = try await prepare(item) }
            catch { eligibility[item.id] = error.localizedDescription }
        }
        checking = false
        if Task.isCancelled || discardRequested { discard() }
    }

    /// Cancellation only prevents later conversion requests from starting.
    func stopAfterCurrent() { stopRequested = true }

    func start(brand: String, selected: Set<UUID>? = nil) async {
        guard !running else { return }
        guard Set(inputs.map(\.id)).count == inputs.count else {
            for index in rows.indices { rows[index].state = .failed(String(localized: "Batch item IDs must be unique")) }
            completed = inputs.count
            return
        }
        let chosen = inputs.filter { selected?.contains($0.id) ?? true }
        rows = chosen.map { BatchRowResult(id: $0.id, name: $0.name, state: .pending) }
        running = true; completed = 0
        for item in chosen {
            if stopRequested { update(item.id, .cancelled); continue }
            do {
                if let message = eligibility[item.id] { throw ConversionError(code: .failed, message: message) }
                let files: [URL]
                let direction: Direction
                if let cached = prepared[item.id] { (files, direction) = cached }
                else { (files, direction) = try await prepare(item) }
                let directory = try PhotoSession.directory(); directories.append(directory)
                let vendor: ConversionVendor = direction == .androidToLive ? .apple : (ConversionVendor(rawValue: brand.lowercased()) ?? .google)
                let request = ConversionRequest(inputs: files, outputDirectory: directory, targetVendor: vendor)
                // Preparation belongs to the current item: a stop during preparation lets
                // that one item finish, but no subsequent item is started.
                update(item.id, .converting)
                let outcome: Result<ConversionResult, ConversionError> = await withCheckedContinuation { continuation in
                    engine.convert(request: request, progress: { _ in }, completion: { continuation.resume(returning: $0) })
                }
                switch outcome {
                case .failure(let error): update(item.id, .failed(error.localizedDescription))
                case .success(let value):
                    do { update(item.id, .converted(try output(value, direction: direction, brand: brand))) }
                    catch { update(item.id, .failed(error.localizedDescription)) }
                }
                completed += 1
            } catch {
                update(item.id, .failed(error.localizedDescription)); completed += 1
            }
        }
        stopRequested = false; running = false
        if discardRequested { discard() }
    }

    func thumbnailURL(for id: UUID) -> URL? { prepared[id]?.0.first }

    func discardWhenIdle() {
        discardRequested = true
        if !checking && !running { discard() }
    }

    /// Call only after conversion has completed and result sharing/saving has finished.
    func discard() {
        guard !running else { return }
        directories.forEach { PhotoSession.remove(containing: $0) }
        directories.removeAll()
    }

    private func update(_ id: UUID, _ state: BatchRowResult.State) {
        if let index = rows.firstIndex(where: { $0.id == id }) { rows[index].state = state }
    }

    private func prepare(_ item: BatchInput) async throws -> ([URL], Direction) {
        switch item.source {
        case .folderFile(let source):
            if let folder {
                guard source.deletingLastPathComponent().standardizedFileURL == folder.url.standardizedFileURL else { throw ConversionFailure.unreadable }
            }
            let scoped = folder == nil && source.startAccessingSecurityScopedResource()
            defer { if scoped { source.stopAccessingSecurityScopedResource() } }
            let directory = try PhotoSession.directory(); directories.append(directory)
            let destination = directory.appendingPathComponent(source.lastPathComponent)
            try await Task.detached(priority: .utility) {
                let values = try source.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey])
                guard values.isRegularFile == true, values.isSymbolicLink != true else { throw ConversionFailure.unreadable }
                var coordinationError: NSError?
                var copyError: Error?
                NSFileCoordinator().coordinate(readingItemAt: source, options: [], error: &coordinationError) { url in
                    do { try FileManager.default.copyItem(at: url, to: destination) } catch { copyError = error }
                }
                if let error = coordinationError ?? (copyError as NSError?) { throw error }
            }.value
            try await confirmMotion(destination)
            return ([destination], .androidToLive)
        case .scannedPhoto(let id, let identity):
            let resources = try await PhotosLibrarySource().originalResources(for: id)
            guard resources.indices.contains(identity.resourceIndex) else { throw ConversionFailure.unreadable }
            let resource = resources[identity.resourceIndex]
            guard resource.filename == identity.filename, resource.uniformTypeIdentifier == identity.uniformTypeIdentifier else { throw ConversionFailure.unreadable }
            let directory = try PhotoSession.directory(); directories.append(directory)
            let url = resource.temporaryFileURL(in: directory)
            try await Task.detached(priority: .utility) { try resource.data.write(to: url) }.value
            try await confirmMotion(url)
            return ([url], .androidToLive)
        case .originalPhoto(let id):
            guard let asset = PHAsset.fetchAssets(withLocalIdentifiers: [id], options: nil).firstObject else { throw LiveInputError.access }
            let url = try await LivePhotoInput.extractOriginalPhoto(asset: asset)
            directories.append(url)
            try await confirmMotion(url)
            return ([url], .androidToLive)
        case .livePair(let photo, let video):
            guard FileManager.default.fileExists(atPath: photo.path), FileManager.default.fileExists(atPath: video.path) else { throw ConversionFailure.unreadable }
            return ([photo, video], .liveToAndroid)
        case .livePhoto(let id):
            guard let asset = PHAsset.fetchAssets(withLocalIdentifiers: [id], options: nil).firstObject else { throw LiveInputError.access }
            guard asset.mediaSubtypes.contains(.photoLive) else { throw LiveInputError.notLive }
            let pair = try await LivePhotoInput.extract(asset: asset)
            directories.append(pair.photo)
            return ([pair.photo, pair.video], .liveToAndroid)
        }
    }

    private func confirmMotion(_ url: URL) async throws {
        let result: InspectionResult = try await withCheckedThrowingContinuation { continuation in
            engine.inspect(input: url, cancellation: InspectionCancellation()) { continuation.resume(with: $0) }
        }
        guard case .motion = result.classification else { throw ConversionFailure.unreadable }
    }

    private func output(_ result: ConversionResult, direction: Direction, brand: String) throws -> ConversionOutput {
        guard result.items.count == 1, let item = result.items.first,
              item.status == .converted || item.status == .resumed, let url = item.output else {
            throw ConversionError(code: .failed, message: result.items.first?.detail ?? String(localized: "Couldn't convert this photo"))
        }
        let all = (try? FileManager.default.contentsOfDirectory(at: url, includingPropertiesForKeys: nil)) ?? [url]
        let photos = all.filter { ["jpg", "jpeg", "heic", "heif"].contains($0.pathExtension.lowercased()) }
        let videos = all.filter { ["mov", "mp4"].contains($0.pathExtension.lowercased()) }
        let files = direction == .androidToLive ? Array(photos.prefix(1)) + Array(videos.prefix(1)) : Array(photos.prefix(1))
        guard files.count == (direction == .androidToLive ? 2 : 1) else { throw ConversionError(code: .failed, message: String(localized: "Couldn't find converted files.")) }
        return ConversionOutput(direction: direction, files: files, brand: brand, image: photos.first.flatMap { UIImage(contentsOfFile: $0.path) })
    }
}
