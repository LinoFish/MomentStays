import SwiftUI
import Observation
import Combine
import Photos
import PhotosUI
import ImageIO
import UniformTypeIdentifiers

// Flow state for the six screens. Conversion goes through the injected `ConversionEngine`
// (via BatchWork); the UI never talks to a concrete engine.

enum MSDirection: Hashable {
    case androidToLive, liveToAndroid
    var title: LocalizedStringKey { self == .androidToLive ? "转成实况照片" : "转成动态照片" }
    var subtitle: LocalizedStringKey { self == .androidToLive ? "将动态照片转换为实况照片" : "转换为对方手机的动态照片" }
    var symbol: String { self == .androidToLive ? "livephoto" : "iphone" }
    /// Home button titles (Camo, msg267). The one-line explanation names both platforms.
    var buttonTitle: LocalizedStringKey { title }
    var homeSymbol: String { self == .androidToLive ? "livephoto" : "play.rectangle" }
}

enum MSSource: Hashable { case photos, files, scan }

enum MSRoute: Hashable {
    case select(MSDirection, MSSource)
    case scan
    case brand
    case converting
    case results
}

enum MSItemStatus: Equatable {
    case waiting, converting, done, failed(String), skipped
}

struct MSItem: Identifiable, Equatable {
    let id: UUID
    let name: String
    var status: MSItemStatus
    var output: ConversionOutput? = nil
}

/// Where a candidate's thumbnail comes from.
enum MSThumbSource: Hashable {
    case asset(String)
    case file(URL)
}

/// One photo the user can select for conversion.
struct MSCandidate: Identifiable {
    let input: BatchInput
    let thumb: MSThumbSource
    var id: UUID { input.id }
    var name: String { input.name }
}

struct MSBrand: Identifiable, Hashable {
    /// `ConversionVendor` raw value.
    let id: String
    let name: LocalizedStringKey
    static func == (lhs: Self, rhs: Self) -> Bool { lhs.id == rhs.id }
    func hash(into hasher: inout Hasher) { hasher.combine(id) }
    static let all: [MSBrand] = [
        MSBrand(id: "google", name: "Google"), MSBrand(id: "xiaomi", name: "小米"), MSBrand(id: "samsung", name: "三星"),
        MSBrand(id: "huawei", name: "华为"), MSBrand(id: "honor", name: "荣耀"), MSBrand(id: "oppo", name: "OPPO"), MSBrand(id: "vivo", name: "vivo")
    ]
    static func named(_ id: String) -> MSBrand { all.first { $0.id == id } ?? all[0] }
}

@Observable @MainActor
final class MSFlow {
    var path: [MSRoute] = []
    var direction: MSDirection = .liveToAndroid
    var source: MSSource = .photos
    var brandID = "samsung"
    var candidates: [MSCandidate] = []
    var selected: Set<UUID> = []
    var items: [MSItem] = []
    var stopping = false
    var runID = UUID()
    var saving = false
    var pickingPhotos = false
    var pickingFiles = false
    /// Alert title and optional detail; set only with the real outcome of an action.
    var notice: String?
    var noticeMessage: String?
    private(set) var thumbnails: [UUID: UIImage] = [:]
    private(set) var outputThumbnails: [UUID: UIImage] = [:]

    @ObservationIgnored private let engine: any ConversionEngine
    @ObservationIgnored private var work: BatchWork?
    @ObservationIgnored private var rowsWatch: AnyCancellable?
    @ObservationIgnored private var startedRun: UUID?
    @ObservationIgnored private var staging: [URL] = []
    @ObservationIgnored private var savedIDs: Set<UUID> = []
    @ObservationIgnored private var exportDelegate: MSExportDelegate?

    init(engine: any ConversionEngine) { self.engine = engine }

    // MARK: Sources

    func open(_ direction: MSDirection, _ source: MSSource) {
        discardSession()
        self.direction = direction; self.source = source
        candidates = []; selected = []; items = []
        switch source {
        case .photos: pickingPhotos = true
        case .files: pickingFiles = true
        case .scan: path.append(.scan)
        }
    }

    func photosPicked(_ picked: [PhotosPickerItem]) async {
        let ids = picked.compactMap(\.itemIdentifier)
        guard !ids.isEmpty else { return }
        // Reading the originals (and their names) needs library access.
        _ = await PHPhotoLibrary.requestAuthorization(for: .readWrite)
        var names: [String: String] = [:]
        PHAsset.fetchAssets(withLocalIdentifiers: ids, options: nil).enumerateObjects { asset, _, _ in
            names[asset.localIdentifier] = PHAssetResource.assetResources(for: asset).first { $0.type == .photo }?.originalFilename
        }
        let live = direction == .liveToAndroid
        candidates = ids.enumerated().map { index, id in
            let name = names[id] ?? String(localized: "照片 \(index + 1)")
            return MSCandidate(input: BatchInput(name: name, source: live ? .livePhoto(id) : .originalPhoto(id)), thumb: .asset(id))
        }
        showSelection(.photos, preselect: true)
    }

    func filesPicked(_ result: Result<[URL], Error>) async {
        guard case .success(let urls) = result, !urls.isEmpty else { return }
        if direction == .androidToLive {
            candidates = urls.filter { Self.isPhoto($0) }.map {
                MSCandidate(input: BatchInput(name: $0.lastPathComponent, source: .folderFile($0)), thumb: .file($0))
            }
        } else {
            let staged: (pairs: [(URL, URL)], directories: [URL], unpaired: Int)
            do { staged = try await Task.detached(priority: .userInitiated) { try Self.stagePairs(urls) }.value }
            catch { notice = String(localized: "Couldn't open this file."); noticeMessage = error.localizedDescription; return }
            staging += staged.directories
            candidates = staged.pairs.map { photo, video in
                MSCandidate(input: BatchInput(name: photo.lastPathComponent, source: .livePair(photo, video)), thumb: .file(photo))
            }
            if candidates.isEmpty { notice = String(localized: "没有找到同名的照片和视频。"); return }
            if staged.unpaired > 0 { notice = String(localized: "\(staged.unpaired) 张照片没有同名视频，已略过。") }
        }
        guard !candidates.isEmpty else { return }
        showSelection(.files, preselect: true)
    }

    func showScanResults(_ identifiers: [String], _ identities: [String: PhotoResourceIdentity]) {
        candidates = identifiers.compactMap { id in
            guard let identity = identities[id] else { return nil }
            return MSCandidate(input: BatchInput(name: identity.filename, source: .scannedPhoto(id, identity)), thumb: .asset(id))
        }
        selected = []
        path.append(.select(.androidToLive, .scan))
    }

    private func showSelection(_ source: MSSource, preselect: Bool) {
        self.source = source
        selected = preselect ? Set(candidates.map(\.id)) : []
        path.append(.select(direction, source))
    }

    /// Scans the accessible library for Android Motion Photos with the injected engine.
    func scanLibrary(progress: @escaping @MainActor (PhotoScanProgress) -> Void) async throws -> PhotoScanResult {
        let directory = try PhotoSession.directory()
        defer { PhotoSession.remove(containing: directory) }
        let inspector = MSInspector(engine: engine)
        let scanner = PhotoScanner(source: PhotosLibrarySource()) { resource in
            let url = resource.temporaryFileURL(in: directory)
            try resource.data.write(to: url)
            defer { try? FileManager.default.removeItem(at: url) }
            switch try await inspector.inspect(url).classification {
            case .motion: return true
            case .staticImage: return false
            case .unknown: throw ConversionError(code: .failed, message: String(localized: "Could not determine whether this is a Motion Photo. Try the original file."))
            }
        }
        return try await scanner.scan { value in await MainActor.run { progress(value) } }
    }

    // MARK: Thumbnails

    func thumbnail(for id: UUID) -> UIImage? { outputThumbnails[id] ?? thumbnails[id] }

    func loadThumbnail(_ id: UUID) async {
        guard thumbnails[id] == nil, let candidate = candidates.first(where: { $0.id == id }) else { return }
        let image: UIImage?
        switch candidate.thumb {
        case .asset(let identifier): image = await Self.assetThumbnail(identifier)
        case .file(let url): image = await Self.fileThumbnail(url)
        }
        if let image { thumbnails[id] = image }
    }

    private static func assetThumbnail(_ identifier: String) async -> UIImage? {
        guard let asset = PHAsset.fetchAssets(withLocalIdentifiers: [identifier], options: nil).firstObject else { return nil }
        let options = PHImageRequestOptions()
        options.deliveryMode = .highQualityFormat
        options.resizeMode = .fast
        options.isNetworkAccessAllowed = false
        return await withCheckedContinuation { continuation in
            PHImageManager.default().requestImage(for: asset, targetSize: CGSize(width: 300, height: 300), contentMode: .aspectFill, options: options) { image, _ in
                continuation.resume(returning: image)
            }
        }
    }

    nonisolated private static func fileThumbnail(_ url: URL) async -> UIImage? {
        await Task.detached(priority: .utility) { () -> UIImage? in
            let scoped = url.startAccessingSecurityScopedResource()
            defer { if scoped { url.stopAccessingSecurityScopedResource() } }
            guard let source = CGImageSourceCreateWithURL(url as CFURL, nil) else { return nil }
            let options: [CFString: Any] = [
                kCGImageSourceCreateThumbnailFromImageAlways: true,
                kCGImageSourceCreateThumbnailWithTransform: true,
                kCGImageSourceThumbnailMaxPixelSize: 300
            ]
            guard let image = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary) else { return nil }
            return UIImage(cgImage: image)
        }.value
    }

    // MARK: Conversion

    func startConversion() {
        let chosen = candidates.filter { selected.contains($0.id) }
        guard !chosen.isEmpty else { return }
        discardWork()
        let work = BatchWork(inputs: chosen.map(\.input), engine: engine)
        self.work = work
        items = chosen.map { MSItem(id: $0.id, name: $0.name, status: .waiting) }
        outputThumbnails = [:]; savedIDs = []
        rowsWatch = work.$rows.sink { [weak self] rows in
            MainActor.assumeIsolated { self?.apply(rows) }
        }
        stopping = false; runID = UUID()
        path = [.converting]
    }

    func retryFailed() {
        for index in items.indices where items[index].isFailed {
            items[index].status = .waiting; items[index].output = nil
        }
        stopping = false; runID = UUID()
        path = [.converting]
    }

    /// The current item finishes; the rest become skipped.
    func stop() {
        stopping = true
        work?.stopAfterCurrent()
    }

    func run() async {
        guard let work, startedRun != runID else { return }
        startedRun = runID
        let pending = Set(items.filter { $0.status == .waiting }.map(\.id))
        await work.start(brand: brandID, selected: pending)
        for index in items.indices where items[index].status == .waiting || items[index].status == .converting {
            items[index].status = .skipped
        }
        stopping = false
        path = [.results]
    }

    private func apply(_ rows: [BatchRowResult]) {
        for row in rows {
            guard let index = items.firstIndex(where: { $0.id == row.id }) else { continue }
            switch row.state {
            case .pending: items[index].status = .waiting
            case .converting: items[index].status = .converting
            case .failed(let message): items[index].status = .failed(message)
            case .cancelled: items[index].status = .skipped
            case .converted(let output):
                guard items[index].output?.id != output.id else { continue }
                items[index].output = output
                items[index].status = .done
                if let photo = output.files.first(where: Self.isPhoto) {
                    let id = row.id
                    Task { if let image = await Self.fileThumbnail(photo) { outputThumbnails[id] = image } }
                }
            }
        }
    }

    func finish() {
        discardSession()
        path = []; items = []; candidates = []; selected = []; stopping = false
    }

    private func discardWork() {
        rowsWatch = nil
        work?.discardWhenIdle()
        work = nil
    }

    /// Removes every temporary session directory owned by this flow.
    private func discardSession() {
        discardWork()
        staging.forEach { PhotoSession.remove(containing: $0) }
        staging.removeAll()
        thumbnails = [:]; outputThumbnails = [:]; savedIDs = []
    }

    var processed: Int { items.filter { $0.status == .done || $0.isFailed }.count }
    var succeeded: Int { items.filter { $0.status == .done }.count }
    var failed: Int { items.filter(\.isFailed).count }
    var skipped: Int { items.filter { $0.status == .skipped }.count }
    private var outputFiles: [URL] { items.compactMap(\.output).flatMap(\.files) }

    // MARK: Save and share

    func shareAll() { share(outputFiles) }

    func share(_ item: MSItem) { share(item.output?.files ?? []) }

    private func share(_ urls: [URL]) {
        guard !urls.isEmpty, let top = MSPresenter.top() else { return }
        let controller = UIActivityViewController(activityItems: urls, applicationActivities: nil)
        controller.popoverPresentationController?.sourceView = top.view
        top.present(controller, animated: true)
    }

    func exportToFiles() {
        let urls = outputFiles
        guard !urls.isEmpty, let top = MSPresenter.top() else { return }
        let picker = UIDocumentPickerViewController(forExporting: urls, asCopy: true)
        let delegate = MSExportDelegate { [weak self] in
            self?.notice = String(localized: "已存到文件"); self?.noticeMessage = nil
        }
        exportDelegate = delegate
        picker.delegate = delegate
        top.present(picker, animated: true)
    }

    /// Saves every converted pair as a Live Photo and reports what actually happened.
    func saveAllToPhotos() async {
        guard !saving else { return }
        saving = true
        defer { saving = false }
        let status = await PHPhotoLibrary.requestAuthorization(for: .addOnly)
        guard status == .authorized || status == .limited else {
            notice = String(localized: "Couldn't save to Photos")
            noticeMessage = String(localized: "请在“设置”中允许留刻添加照片。")
            return
        }
        var saved = 0
        var failures: [String] = []
        for item in items where item.status == .done && !savedIDs.contains(item.id) {
            guard let files = item.output?.files,
                  let still = files.first(where: Self.isPhoto),
                  let movie = files.first(where: { ["mov", "mp4"].contains($0.pathExtension.lowercased()) }) else {
                failures.append(String(localized: "Couldn't find converted files.")); continue
            }
            do {
                try await PHPhotoLibrary.shared().performChanges {
                    let request = PHAssetCreationRequest.forAsset()
                    request.addResource(with: .photo, fileURL: still, options: nil)
                    request.addResource(with: .pairedVideo, fileURL: movie, options: nil)
                }
                savedIDs.insert(item.id); saved += 1
            } catch {
                failures.append(error.localizedDescription)
            }
        }
        if failures.isEmpty {
            notice = String(localized: "Saved to Photos")
            noticeMessage = String(localized: "Touch and hold in Photos to play.")
        } else {
            notice = saved > 0 ? String(localized: "已存入 \(saved) 张，\(failures.count) 张没能存入照片") : String(localized: "Couldn't save to Photos")
            noticeMessage = failures.first
        }
    }

    // MARK: Helpers

    nonisolated static func isPhoto(_ url: URL) -> Bool {
        ["jpg", "jpeg", "heic", "heif"].contains(url.pathExtension.lowercased())
    }

    /// Copies each still and its same-named movie into its own session folder.
    nonisolated private static func stagePairs(_ urls: [URL]) throws -> (pairs: [(URL, URL)], directories: [URL], unpaired: Int) {
        func key(_ url: URL) -> String { url.deletingPathExtension().lastPathComponent.lowercased() }
        let stills = urls.filter(isPhoto)
        var videos: [String: URL] = [:]
        for url in urls where ["mov", "mp4"].contains(url.pathExtension.lowercased()) { videos[key(url)] = url }
        var pairs: [(URL, URL)] = []
        var directories: [URL] = []
        var unpaired = 0
        do {
            for still in stills {
                guard let video = videos[key(still)] else { unpaired += 1; continue }
                let directory = try PhotoSession.directory(); directories.append(directory)
                var copies: [URL] = []
                for source in [still, video] {
                    let scoped = source.startAccessingSecurityScopedResource()
                    defer { if scoped { source.stopAccessingSecurityScopedResource() } }
                    let destination = directory.appendingPathComponent(source.lastPathComponent)
                    var coordinationError: NSError?
                    var copyError: Error?
                    NSFileCoordinator().coordinate(readingItemAt: source, options: [], error: &coordinationError) { url in
                        do { try FileManager.default.copyItem(at: url, to: destination) } catch { copyError = error }
                    }
                    if let error = coordinationError ?? copyError { throw error }
                    copies.append(destination)
                }
                pairs.append((copies[0], copies[1]))
            }
        } catch {
            directories.forEach { PhotoSession.remove(containing: $0) }
            throw error
        }
        return (pairs, directories, unpaired)
    }
}

extension MSItem {
    var isFailed: Bool { if case .failed = status { return true }; return false }
}

/// Lets the background scanner call the main-queue engine.
private final class MSInspector: @unchecked Sendable {
    private let engine: any ConversionEngine
    init(engine: any ConversionEngine) { self.engine = engine }
    func inspect(_ url: URL) async throws -> InspectionResult {
        let cancellation = InspectionCancellation()
        return try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { continuation in
                DispatchQueue.main.async {
                    self.engine.inspect(input: url, cancellation: cancellation) { continuation.resume(with: $0) }
                }
            }
        } onCancel: { cancellation.cancel() }
    }
}

final class MSExportDelegate: NSObject, UIDocumentPickerDelegate {
    private let done: @MainActor () -> Void
    init(done: @escaping @MainActor () -> Void) { self.done = done }
    func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) {
        MainActor.assumeIsolated { done() }
    }
}

@MainActor enum MSPresenter {
    /// Top-most controller of the foreground scene's key window.
    static func top() -> UIViewController? {
        let scenes = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
        let scene = scenes.first { $0.activationState == .foregroundActive } ?? scenes.first
        var controller = scene?.keyWindow?.rootViewController
        while let presented = controller?.presentedViewController, !presented.isBeingDismissed { controller = presented }
        return controller
    }
}

#if DEBUG
extension MSFlow {
    /// Preview-only sample data; never used by the app.
    static func preview(_ direction: MSDirection, converting: Bool = false) -> MSFlow {
        let flow = MSFlow(engine: UnavailableEngine())
        flow.direction = direction
        flow.source = .files
        flow.candidates = (1...8).map {
            let url = URL(fileURLWithPath: "/preview/IMG_\(2040 + $0).jpg")
            return MSCandidate(input: BatchInput(name: url.lastPathComponent, source: .folderFile(url)), thumb: .file(url))
        }
        flow.items = flow.candidates.enumerated().map { index, candidate in
            MSItem(id: candidate.id, name: candidate.name, status: [2, 6].contains(index) ? .failed("未找到配对视频") : .done)
        }
        if converting {
            flow.items[3].status = .converting
            for index in 4..<8 { flow.items[index].status = .waiting }
        }
        flow.selected = [flow.candidates[0].id, flow.candidates[3].id]
        return flow
    }
}
#endif
