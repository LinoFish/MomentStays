import SwiftUI
import PhotosUI
import Photos
import UniformTypeIdentifiers

// The interface owns presentation only. The conversion implementation is injected at the app boundary.
struct ConversionClient {
    let engine: any ConversionEngine
    func convert(_ input: InputPhoto, _ brand: String) async throws -> ConversionOutput {
        guard let file = input.file else { throw ConversionFailure.unavailable }
        if input.direction == .liveToAndroid && input.pairedVideo == nil { throw ConversionFailure.unreadable }
        let directory = try PhotoSession.directory()
        var succeeded = false
        defer { if !succeeded { PhotoSession.remove(containing: directory) } }
        let vendor = input.direction == .androidToLive ? ConversionVendor.apple : (ConversionVendor(rawValue: brand.lowercased()) ?? .google)
        let request = ConversionRequest(inputs: [file] + (input.pairedVideo.map { [$0] } ?? []), outputDirectory: directory, targetVendor: vendor)
        let result: ConversionResult = try await withCheckedThrowingContinuation { continuation in
            engine.convert(request: request, progress: { _ in }) { continuation.resume(with: $0) }
        }
        guard let item = result.items.first, item.status == .converted || item.status == .resumed, let output = item.output else {
            throw ConversionError(code: .failed, message: result.items.first?.detail ?? String(localized: "Couldn't convert this photo"))
        }
        let all = (try? FileManager.default.contentsOfDirectory(at: output, includingPropertiesForKeys: nil)) ?? [output]
        let photos = all.filter { ["jpg", "jpeg", "heic", "heif"].contains($0.pathExtension.lowercased()) }
        let videos = all.filter { ["mov", "mp4"].contains($0.pathExtension.lowercased()) }
        let files = input.direction == .androidToLive ? Array(photos.prefix(1)) + Array(videos.prefix(1)) : Array(photos.prefix(1))
        guard files.count == (input.direction == .androidToLive ? 2 : 1) else { throw ConversionError(code: .failed, message: String(localized: "Couldn't find converted files.")) }
        succeeded = true
        return ConversionOutput(direction: input.direction, files: files, brand: brand, image: photos.first.flatMap { UIImage(contentsOfFile: $0.path) })
    }
}
struct UnavailableEngine: ConversionEngine {
    func inspect(input: URL, cancellation: InspectionCancellation, completion: @escaping (Result<InspectionResult, ConversionError>) -> Void) {
        completion(.failure(ConversionError(code: .unavailable, message: String(localized: "Conversion is not available in this build."))))
    }
    func convert(request: ConversionRequest, progress: @escaping (ConversionProgress) -> Void, completion: @escaping (Result<ConversionResult, ConversionError>) -> Void) {
        completion(.failure(ConversionError(code: .unavailable, message: String(localized: "Conversion is not available in this build."))))
    }
}
enum ConversionFailure: LocalizedError {
    case unavailable, unreadable
    var errorDescription: String? {
        switch self {
        case .unavailable: return String(localized: "Conversion is not available in this build.")
        case .unreadable: return String(localized: "Couldn't open this file.")
        }
    }
}
enum Direction { case androidToLive, liveToAndroid }
struct InputPhoto: Identifiable, Hashable {
    static func == (lhs: Self, rhs: Self) -> Bool { lhs.id == rhs.id }
    func hash(into hasher: inout Hasher) { hasher.combine(id) }
    let id = UUID()
    let direction: Direction
    let file: URL?
    let assetID: String?
    var pairedVideo: URL? = nil
    let name: String
    let image: UIImage?
    var originalCertain = true
}
struct ConversionOutput: Identifiable, Hashable {
    static func == (lhs: Self, rhs: Self) -> Bool { lhs.id == rhs.id }
    func hash(into hasher: inout Hasher) { hasher.combine(id) }
    let id = UUID()
    let direction: Direction
    let files: [URL]
    let brand: String
    let image: UIImage?
}

struct BatchRoute: Identifiable, Hashable {
    static func == (lhs: Self, rhs: Self) -> Bool { lhs.id == rhs.id }
    func hash(into hasher: inout Hasher) { hasher.combine(id) }
    let id = UUID()
    let inputs: [BatchInput]
    let direction: Direction
    let folder: BatchFolderAccess?
}

struct HomeView: View {
    let engine: ConversionClient
    @Environment(\.scenePhase) private var scenePhase
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize
    @State private var photo: PhotosPickerItem?
    @State private var photoDirection: Direction = .androidToLive
    @State private var showPhotos = false
    @State private var showFiles = false
    @State private var showSource = false
    @State private var showScan = false
    @State private var about = false
    @State private var loading = false
    @State private var error: String?
    @State private var input: InputPhoto?
    @State private var output: ConversionOutput?
    @State private var limitedPicker = false
    @State private var needsLimitedSelection = false
    @State private var batchRoute: BatchRoute?
    @State private var batchPhotos: [PhotosPickerItem] = []
    @State private var showBatchPhotos = false
    @State private var showBatchFiles = false
    @State private var showBatchFolder = false
    @State private var batchDirection: Direction = .androidToLive

    var body: some View {
        NavigationStack {
            AnyView(AnyView(homeContent
            .background(Color(uiColor: .systemGroupedBackground))
            .navigationTitle("MomentStays")
            .toolbar { Button { about = true } label: { Image(systemName: "info.circle") }.accessibilityLabel("About") }
            .sheet(isPresented: $about) { AboutView() }
            .confirmationDialog("Choose Photo Source", isPresented: $showSource) {
                Button("Find in Photos") { showScan = true }
                Button("Choose from Photos") { photoDirection = .androidToLive; showPhotos = true }
                Button("Choose from Files") { showFiles = true }
                Button("Select Multiple from Photos") { batchDirection = .androidToLive; showBatchPhotos = true }
                Button("Select Multiple from Files") { showBatchFiles = true }
                Button("Choose a Folder") { showBatchFolder = true }
                Button("Cancel", role: .cancel) {}
            }
            .navigationDestination(isPresented: $showScan) { ScanView(engine: engine.engine, chooseFiles: { showScan = false; showFiles = true }, onSelect: { url in showScan = false; importFile(url) }, onBatch: { _ in }) }
            .photosPicker(isPresented: $showBatchPhotos, selection: $batchPhotos, maxSelectionCount: 0, matching: batchDirection == .liveToAndroid ? .livePhotos : .images, preferredItemEncoding: .current, photoLibrary: .shared())
            .onChange(of: batchPhotos) { _, items in
                guard !items.isEmpty else { return }
                let direction = batchDirection
                batchPhotos = []
                Task { await importBatchPhotos(items, direction: direction) }
            }
            .fileImporter(isPresented: $showBatchFiles, allowedContentTypes: [.jpeg, .heic], allowsMultipleSelection: true) { result in
                switch result {
                case .success(let urls): batchRoute = BatchRoute(inputs: urls.map { BatchInput(name: $0.lastPathComponent, source: .folderFile($0)) }, direction: .androidToLive, folder: nil)
                case .failure(let failure): error = failure.localizedDescription
                }
            }
            .fileImporter(isPresented: $showBatchFolder, allowedContentTypes: [.folder]) { result in
                do {
                    let folder = BatchFolderAccess(url: try result.get())
                    Task {
                        do {
                            let files = try await Task.detached(priority: .utility) { try folder.files() }.value
                            batchRoute = BatchRoute(inputs: files.map { BatchInput(name: $0.lastPathComponent, source: .folderFile($0)) }, direction: .androidToLive, folder: folder)
                        } catch { self.error = error.localizedDescription }
                    }
                } catch { self.error = error.localizedDescription }
            }
            .navigationDestination(item: $batchRoute) { route in
                BatchSelectionView(inputs: route.inputs, direction: route.direction, engine: engine.engine, folder: route.folder, chooseFiles: { batchRoute = nil; showBatchFiles = true })
            })
            .photosPicker(isPresented: $showPhotos, selection: $photo, matching: photoDirection == .liveToAndroid ? .livePhotos : .images, preferredItemEncoding: .current, photoLibrary: .shared())
            .onChange(of: photo) { _, item in if let item { Task { await importPhoto(item) } } }
            .fileImporter(isPresented: $showFiles, allowedContentTypes: [.jpeg, .heic]) { result in
                switch result { case .success(let url): importFile(url); case .failure: error = String(localized: "Couldn't open this file.") }
            }
            .onOpenURL { importFile($0) }
            .onAppear { PhotoSession.removeExpired(); consumeHandoff() })
            .onChange(of: scenePhase) { _, phase in if phase == .active { consumeHandoff() } }
            .navigationDestination(item: $input) { value in
                ConvertView(input: value, engine: engine) { output = $0 }
                    .navigationDestination(item: $output) { result in ResultView(result: result) { output = nil; input = nil } }
            }
            .onChange(of: input) { old, new in if new == nil { PhotoSession.remove(containing: old?.file) } }
            .onChange(of: output) { old, new in if new == nil { PhotoSession.remove(containing: old?.files.first) } }
            .sheet(isPresented: $limitedPicker) { LimitedLibraryManager() }
            .alert("Allow access to the selected photo", isPresented: $needsLimitedSelection) {
                Button("Manage Photo Access") { limitedPicker = true }
                Button("Cancel", role: .cancel) {}
            } message: { Text("Add this photo to the photos available to MomentStays, then select it again.") }
            .alert("Couldn't open this file", isPresented: Binding(get: { error != nil }, set: { if !$0 { error = nil } })) { Button("OK", role: .cancel) {} } message: { Text(error ?? "") }
            .overlay { if loading { ProgressView("Reading photo…").padding(24).background(.regularMaterial, in: RoundedRectangle(cornerRadius: 20)) } }
            .disabled(loading)
        }
    }
    private var homeContent: some View {
        GeometryReader { geometry in
                if geometry.size.width >= 700 && !dynamicTypeSize.isAccessibilitySize {
                    if #available(iOS 27.1, *) {
                        ArrangementView {
                            ScrollView { VStack(alignment: .leading, spacing: 24) { homeHeader; privacyNote }.padding(20) }
                                .splitArrangementLayoutRatio(0.35)
                        } secondary: {
                            ScrollView { VStack(alignment: .leading, spacing: 12) { androidCard; liveCard }.frame(maxWidth: 480).padding(20) }
                        }.arrangementViewStyle(.split)
                    } else {
                        ScrollView {
                            HStack(alignment: .top, spacing: 24) {
                                VStack(alignment: .leading, spacing: 24) { homeHeader; privacyNote }.frame(width: min(geometry.size.width * 0.35, 360))
                                VStack(alignment: .leading, spacing: 12) { androidCard; liveCard }.frame(maxWidth: 480)
                            }.frame(maxWidth: .infinity).padding(20)
                        }
                    }
                } else {
                    ScrollView {
                        VStack(spacing: 24) { homeHeader; VStack(spacing: 12) { androidCard; liveCard }; privacyNote }.padding(20)
                    }
                }
            }
    }
    private var homeHeader: some View {
        VStack(spacing: 12) {
            MomentMark(size: 64)
            Text("Keep moving photos moving, on any phone.").font(.title2.weight(.semibold)).multilineTextAlignment(.center)
            Text("Live Photos and Motion Photos, together.").font(.subheadline).foregroundStyle(.secondary).multilineTextAlignment(.center)
        }.frame(maxWidth: .infinity).padding(.top, 8)
    }
    private var privacyNote: some View {
        VStack(alignment: .leading, spacing: 24) {
            Label("Photos are processed on this iPhone and never uploaded.", systemImage: "lock.shield")
                .font(.footnote).foregroundStyle(.secondary).frame(maxWidth: .infinity, alignment: .leading)
            Text("You can also open a Motion Photo from Files.").font(.footnote).foregroundStyle(.secondary).frame(maxWidth: .infinity, alignment: .leading)
        }
    }
    private var androidCard: some View {
        Button { showSource = true } label: {
            DirectionCard(symbol: "livephoto", title: "Android → Live Photo", subtitle: "Turn a Motion Photo from an Android friend into a Live Photo.")
        }.buttonStyle(.plain)
    }
    private var liveCard: some View {
        VStack(spacing: 8) {
            Button { photoDirection = .liveToAndroid; showPhotos = true } label: {
                DirectionCard(symbol: "square.and.arrow.up.on.square", title: "Live Photo → Android", subtitle: "Send a Live Photo to Android so it still moves there.")
            }.buttonStyle(.plain)
            Button("Select Multiple Live Photos") { batchDirection = .liveToAndroid; showBatchPhotos = true }
        }
    }
    private func consumeHandoff() {
        guard input == nil, !loading, let group = FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: "group.com.app9x3k2q.momentstays") else { return }
        let manifest = group.appendingPathComponent("pending.json")
        guard FileManager.default.fileExists(atPath: manifest.path) else { return }
        guard let data = try? Data(contentsOf: manifest),
              let object = (try? JSONSerialization.jsonObject(with: data)) as? [String: String],
              let directoryName = object["directory"], directoryName.hasPrefix("Handoff-"), directoryName == URL(fileURLWithPath: directoryName).lastPathComponent,
              let photoName = object["photo"], photoName == URL(fileURLWithPath: photoName).lastPathComponent else {
            try? FileManager.default.removeItem(at: manifest)
            error = String(localized: "Couldn't open this file.")
            return
        }
        let directory = group.appendingPathComponent(directoryName, isDirectory: true)
        let source = directory.appendingPathComponent(photoName)
        let videoName = object["video"] ?? ""
        guard FileManager.default.fileExists(atPath: source.path),
              videoName.isEmpty || (videoName == URL(fileURLWithPath: videoName).lastPathComponent && FileManager.default.fileExists(atPath: directory.appendingPathComponent(videoName).path)) else {
            try? FileManager.default.removeItem(at: manifest)
            try? FileManager.default.removeItem(at: directory)
            error = String(localized: "Couldn't open this file.")
            return
        }
        var staging: URL?
        do {
            let folder = try PhotoSession.directory(); staging = folder
            let photo = folder.appendingPathComponent("LivePhoto").appendingPathExtension(source.pathExtension)
            try FileManager.default.copyItem(at: source, to: photo)
            var video: URL?
            if let name = object["video"], !name.isEmpty {
                guard name == URL(fileURLWithPath: name).lastPathComponent else { throw ConversionFailure.unreadable }
                let copy = folder.appendingPathComponent("LivePhoto").appendingPathExtension(URL(fileURLWithPath: name).pathExtension)
                try FileManager.default.copyItem(at: directory.appendingPathComponent(name), to: copy)
                video = copy
            }
            // Publish a complete private copy before acknowledging the shared handoff.
            try FileManager.default.removeItem(at: manifest)
            try? FileManager.default.removeItem(at: directory)
            let candidate = InputPhoto(direction: video == nil ? .androidToLive : .liveToAndroid, file: photo, assetID: nil, pairedVideo: video, name: photoName, image: UIImage(contentsOfFile: photo.path), originalCertain: video != nil)
            if video == nil { Task { await inspect(candidate) } } else { input = candidate }
        } catch {
            PhotoSession.remove(containing: staging)
            // Keep a valid handoff for retry if copying or acknowledgement fails.
            self.error = String(localized: "Couldn't open this file.")
        }
    }
    private func importFile(_ url: URL) {
        let access = url.startAccessingSecurityScopedResource()
        defer { if access { url.stopAccessingSecurityScopedResource() } }
        var directory: URL?
        do {
            let staging = try PhotoSession.directory(); directory = staging
            let destination = staging.appendingPathComponent(url.lastPathComponent)
            try FileManager.default.copyItem(at: url, to: destination)
            let candidate = InputPhoto(direction: .androidToLive, file: destination, assetID: nil, name: url.lastPathComponent, image: UIImage(contentsOfFile: destination.path))
            Task { await inspect(candidate) }
        } catch { PhotoSession.remove(containing: directory); self.error = String(localized: "Couldn't open this file.") }
    }
    @MainActor private func inspect(_ candidate: InputPhoto) async {
        guard let url = candidate.file else { return }
        loading = true; defer { loading = false }
        do {
            let inspected: InspectionResult = try await withCheckedThrowingContinuation { continuation in
                engine.engine.inspect(input: url, cancellation: InspectionCancellation()) { continuation.resume(with: $0) }
            }
            switch inspected.classification {
            case .motion: input = candidate
            case .staticImage:
                PhotoSession.remove(containing: candidate.file)
                error = candidate.originalCertain ? String(localized: "This file isn't a Motion Photo, so it can't be converted.") : String(localized: "Could not determine whether this is a Motion Photo. Try the original file.")
            case .unknown: PhotoSession.remove(containing: candidate.file); error = String(localized: "Could not determine whether this is a Motion Photo. Try the original file.")
            }
        } catch { PhotoSession.remove(containing: candidate.file); self.error = error.localizedDescription }
    }
    @MainActor private func importBatchPhotos(_ items: [PhotosPickerItem], direction: Direction) async {
        loading = true
        defer { loading = false }
        if PHPhotoLibrary.authorizationStatus(for: .readWrite) == .notDetermined {
            _ = await PHPhotoLibrary.requestAuthorization(for: .readWrite)
        }
        let status = PHPhotoLibrary.authorizationStatus(for: .readWrite)
        let ids = items.compactMap(\.itemIdentifier)
        let accessible = PHAsset.fetchAssets(withLocalIdentifiers: ids, options: nil)
        var allowed = Set<String>()
        accessible.enumerateObjects { asset, _, _ in allowed.insert(asset.localIdentifier) }
        guard ids.count == items.count, ids.allSatisfy({ allowed.contains($0) }) else {
            if status == .limited { needsLimitedSelection = true }
            else { error = LiveInputError.access.localizedDescription }
            return
        }
        let inputs = ids.enumerated().map { index, id in
            BatchInput(name: String(localized: "Photo \(index + 1)"), source: direction == .liveToAndroid ? .livePhoto(id) : .originalPhoto(id))
        }
        batchRoute = BatchRoute(inputs: inputs, direction: direction, folder: nil)
    }
    @MainActor private func importPhoto(_ item: PhotosPickerItem) async {
        loading = true; defer { loading = false; photo = nil }
        do {
            guard let id = item.itemIdentifier else { throw LiveInputError.originalUnavailable }
            var asset = PHAsset.fetchAssets(withLocalIdentifiers: [id], options: nil).firstObject
            if asset == nil, PHPhotoLibrary.authorizationStatus(for: .readWrite) == .notDetermined {
                _ = await PHPhotoLibrary.requestAuthorization(for: .readWrite)
                asset = PHAsset.fetchAssets(withLocalIdentifiers: [id], options: nil).firstObject
            }
            guard let asset else {
                if PHPhotoLibrary.authorizationStatus(for: .readWrite) == .limited { needsLimitedSelection = true; return }
                throw LiveInputError.access
            }
            if photoDirection == .liveToAndroid {
                guard asset.mediaSubtypes.contains(.photoLive) else { throw LiveInputError.notLive }
                let pair = try await LivePhotoInput.extract(asset: asset)
                input = InputPhoto(direction: .liveToAndroid, file: pair.photo, assetID: id, pairedVideo: pair.video, name: pair.photo.lastPathComponent, image: UIImage(contentsOfFile: pair.photo.path))
            } else {
                let url = try await LivePhotoInput.extractOriginalPhoto(asset: asset)
                let candidate = InputPhoto(direction: .androidToLive, file: url, assetID: id, name: url.lastPathComponent, image: UIImage(contentsOfFile: url.path))
                await inspect(candidate)
            }
        } catch { self.error = error.localizedDescription }
    }
}

struct DirectionCard: View {
    let symbol: String
    let title: LocalizedStringKey
    let subtitle: LocalizedStringKey
    var body: some View {
        HStack(spacing: 16) {
            Image(systemName: symbol).font(.title2.weight(.semibold)).foregroundStyle(.orange).frame(width: 52, height: 52).background(.orange.opacity(0.15), in: RoundedRectangle(cornerRadius: 14))
            VStack(alignment: .leading, spacing: 6) { Text(title).font(.headline).foregroundStyle(.primary); Text(subtitle).font(.subheadline).foregroundStyle(.secondary).multilineTextAlignment(.leading) }
            Spacer(minLength: 0)
            Image(systemName: "chevron.right").font(.footnote.weight(.semibold)).foregroundStyle(.tertiary)
        }.padding(20).frame(maxWidth: .infinity, alignment: .leading).background(Color(uiColor: .secondarySystemGroupedBackground), in: RoundedRectangle(cornerRadius: 22))
    }
}
struct MomentMark: View {
    let size: CGFloat
    var body: some View {
        ZStack {
            Circle().trim(from: 0, to: 0.75).stroke(.primary, style: StrokeStyle(lineWidth: size * 0.022, lineCap: .round, dash: [0.1, size * 0.08]))
            Circle().stroke(.primary, lineWidth: size * 0.048).frame(width: size * 0.6, height: size * 0.6)
            Circle().fill(Color(red: 0.94, green: 0.64, blue: 0.24)).frame(width: size * 0.23, height: size * 0.23)
        }.frame(width: size, height: size).accessibilityHidden(true)
    }
}
struct AboutView: View {
    @Environment(\.dismiss) private var dismiss
    var body: some View {
        NavigationStack { Form {
            Section { LabeledContent("Version", value: Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "1.0")
                Text("Photos are processed on this iPhone and never uploaded.")
                Link("Open source · GPL-3.0", destination: URL(string: "https://github.com/LinoFish/MomentStays")!) }
            Section { MadeWithAirBuild() }
        }.navigationTitle("About").toolbar { Button { dismiss() } label: { Label("Done", systemImage: "checkmark") } } }
    }
}

struct ConvertView: View {
    let input: InputPhoto
    let engine: ConversionClient
    let onComplete: (ConversionOutput) -> Void
    @AppStorage("lastBrand") private var brand = "Google"
    @State private var converting = false
    @State private var failure: String?
    private let brands = ["Google", "Xiaomi", "Samsung", "HUAWEI", "HONOR", "OPPO", "vivo"]
    var body: some View {
        AdaptiveWorkspace {
            VStack(alignment: .leading, spacing: 24) {
                Text(input.name).font(.caption.monospacedDigit()).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                if input.direction == .liveToAndroid {
                    VStack(alignment: .leading, spacing: 12) {
                        Text("What phone will they use?").font(.footnote.weight(.semibold)).foregroundStyle(.secondary)
                        LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 8), count: 2), spacing: 8) {
                            ForEach(brands, id: \.self) { value in
                                Button { brand = value } label: {
                                    HStack { if brand == value { Image(systemName: "checkmark") }; Text(value).multilineTextAlignment(.center) }
                                        .font(.subheadline.weight(.medium)).frame(maxWidth: .infinity, minHeight: 44)
                                        .foregroundStyle(brand == value ? Color(red: 0.12, green: 0.16, blue: 0.2) : .primary)
                                        .background(brand == value ? Color.orange : Color(uiColor: .tertiarySystemFill), in: Capsule())
                                }.buttonStyle(.plain).accessibilityAddTraits(brand == value ? .isSelected : [])
                            }
                        }.disabled(converting)
                        Text("Choose the recipient's phone brand.").font(.caption).foregroundStyle(.secondary)
                    }
                } else {
                    Label("The result will be a paired photo and video.", systemImage: "info.circle").font(.subheadline).foregroundStyle(.secondary).padding(16).frame(maxWidth: .infinity, alignment: .leading).background(Color(uiColor: .secondarySystemGroupedBackground), in: RoundedRectangle(cornerRadius: 22))
                }
                if let failure {
                    Label { VStack(alignment: .leading, spacing: 4) { Text("Couldn't convert this photo").font(.headline); Text(failure).font(.subheadline).foregroundStyle(.secondary) } } icon: { Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.red) }.padding(16).frame(maxWidth: .infinity, alignment: .leading).background(Color(uiColor: .secondarySystemGroupedBackground), in: RoundedRectangle(cornerRadius: 22))
                }
                Button { Task { await convert() } } label: {
                    Group { if converting { ProgressView("Converting…") } else { Label(failure == nil ? "Convert" : "Try Again", systemImage: "arrow.triangle.2.circlepath") } }.font(.body.weight(.semibold)).frame(maxWidth: .infinity, minHeight: 54)
                }.buttonStyle(.glassProminent).buttonBorderShape(.capsule).foregroundStyle(Color(red: 0.12, green: 0.16, blue: 0.2)).disabled(converting)
            }
        } content: {
            ZStack(alignment: .topLeading) {
                Group {
                    if input.direction == .liveToAndroid, let photo = input.file, let video = input.pairedVideo {
                        LivePhotoPreview(photo: photo, video: video).frame(minHeight: 220, maxHeight: 420)
                    } else if let image = input.image { Image(uiImage: image).resizable().scaledToFit() }
                    else { Image(systemName: "livephoto").font(.system(size: 70)).foregroundStyle(.secondary).frame(maxWidth: .infinity, minHeight: 220) }
                }.frame(maxWidth: .infinity).background(Color(uiColor: .secondarySystemGroupedBackground)).clipShape(RoundedRectangle(cornerRadius: 28))
                Label(input.direction == .liveToAndroid ? "Live Photo" : "Motion Photo", systemImage: input.direction == .liveToAndroid ? "livephoto" : "play.circle")
                    .font(.caption.weight(.semibold)).padding(8).background(.ultraThinMaterial, in: Capsule()).padding(12)
            }
        }.background(Color(uiColor: .systemGroupedBackground))
            .navigationTitle("Convert").navigationBarTitleDisplayMode(.inline).navigationBarBackButtonHidden(converting)
    }
    @MainActor private func convert() async {
        converting = true; failure = nil
        do { let result = try await engine.convert(input, brand); converting = false; onComplete(result) }
        catch { converting = false; failure = error.localizedDescription }
    }
}
struct ResultView: View {
    let result: ConversionOutput
    let finish: () -> Void
    @Environment(\.openURL) private var openURL
    @State private var sharing = false
    @State private var saving = false
    @State private var saved = false
    @State private var saveError = false
    @State private var exporting = false
    var body: some View {
        AdaptiveWorkspace {
            VStack(alignment: .leading, spacing: 16) {
                VStack(alignment: .leading, spacing: 12) { ForEach(result.files, id: \.self) { file in Label(file.lastPathComponent, systemImage: "doc").lineLimit(1).truncationMode(.middle) } }.padding(16).frame(maxWidth: .infinity, alignment: .leading).background(Color(uiColor: .secondarySystemGroupedBackground), in: RoundedRectangle(cornerRadius: 22))
                if result.direction == .androidToLive {
                    Button { Task { await save() } } label: { Label(saved ? "Saved to Photos" : "Save to Photos", systemImage: saved ? "checkmark" : "photo.badge.plus").frame(maxWidth: .infinity, minHeight: 54) }.buttonStyle(.glassProminent).buttonBorderShape(.capsule).disabled(saved || saving)
                    if saved { Text("Touch and hold in Photos to play.").font(.caption).foregroundStyle(.secondary) }
                }
                Button { sharing = true } label: { Label("Share", systemImage: "square.and.arrow.up").frame(maxWidth: .infinity, minHeight: 54) }.buttonStyle(.glass).buttonBorderShape(.capsule)
                if result.direction == .liveToAndroid, !result.files.isEmpty {
                    Button { exporting = true } label: { Label("Save to Files", systemImage: "folder").frame(maxWidth: .infinity, minHeight: 54) }.buttonStyle(.glass).buttonBorderShape(.capsule)
                }
                Button("Convert Another", action: finish).frame(minHeight: 44)
            }
        } content: {
            VStack(spacing: 24) {
                Image(systemName: "checkmark.circle.fill").font(.system(size: 56)).foregroundStyle(.green).accessibilityHidden(true)
                Text("Your photo is ready").font(.title2.weight(.semibold))
                if result.direction == .liveToAndroid { Text("For \(result.brand). Share it as a file to avoid flattening it into a still.").font(.subheadline).foregroundStyle(.secondary).multilineTextAlignment(.center) }
                else { Text("Converted to a Live Photo.").font(.subheadline).foregroundStyle(.secondary) }
                if let image = result.image { Image(uiImage: image).resizable().scaledToFit().frame(maxWidth: 420, maxHeight: 300).clipShape(RoundedRectangle(cornerRadius: 22)) }
            }.padding(.top, 24)
        }.background(Color(uiColor: .systemGroupedBackground))
            .navigationTitle("Done").navigationBarTitleDisplayMode(.inline).navigationBarBackButtonHidden()
            .toolbar { Button(action: finish) { Label("Done", systemImage: "checkmark") } }
            .sheet(isPresented: $sharing) { ShareSheet(items: result.files) }
            .sheet(isPresented: $exporting) { ExportFileSheet(urls: result.files) }
            .alert("Couldn't save to Photos", isPresented: $saveError) { Button("Open Settings") { if let url = URL(string: UIApplication.openSettingsURLString) { openURL(url) } }; Button("Cancel", role: .cancel) {} } message: { Text("Couldn't save. Allow MomentStays to add photos in Settings.") }
    }
    @MainActor private func save() async {
        guard result.files.count >= 2 else { saveError = true; return }
        saving = true; defer { saving = false }
        do {
            try await PHPhotoLibrary.shared().performChanges {
                let request = PHAssetCreationRequest.forAsset()
                request.addResource(with: .photo, fileURL: result.files[0], options: nil)
                request.addResource(with: .pairedVideo, fileURL: result.files[1], options: nil)
            }
            saved = true
        } catch { saveError = true }
    }
}
struct ShareSheet: UIViewControllerRepresentable {
    let items: [URL]
    func makeUIViewController(context: Context) -> UIActivityViewController { UIActivityViewController(activityItems: items, applicationActivities: nil) }
    func updateUIViewController(_ controller: UIActivityViewController, context: Context) {}
}

struct ExportFileSheet: UIViewControllerRepresentable {
    let urls: [URL]
    func makeUIViewController(context: Context) -> UIDocumentPickerViewController {
        UIDocumentPickerViewController(forExporting: urls, asCopy: true)
    }
    func updateUIViewController(_ controller: UIDocumentPickerViewController, context: Context) {}
}
