import SwiftUI
import Photos

struct ScanView: View {
    let engine: any ConversionEngine
    let chooseFiles: () -> Void
    let onSelect: (URL) -> Void
    let onBatch: ([BatchInput]) -> Void
    @State private var scanTask: Task<Void, Never>?
    @State private var selectionTask: Task<Void, Never>?
    @State private var progress: PhotoScanProgress?
    @State private var result: PhotoScanResult?
    @State private var stopped = false
    @State private var error: String?
    @State private var reading = false
    @State private var managingPhotos = false
    @State private var availableWidth: CGFloat = 0
    @State private var scanID = UUID()
    @State private var selected = Set<String>()
    @State private var batchWork: BatchWork?
    @State private var convertingBatch = false
    private var identifiers: [String] { result?.motionAssetIdentifiers ?? progress?.motionAssetIdentifiers ?? [] }
    private var identities: [String: PhotoResourceIdentity] { result?.matchedResources ?? progress?.matchedResources ?? [:] }
    private var selectable: [String] { identifiers.filter { identities[$0] != nil && PHAsset.fetchAssets(withLocalIdentifiers: [$0], options: nil).firstObject != nil } }
    @Environment(\.openURL) private var openURL
    @Environment(\.dynamicTypeSize) private var typeSize
    var body: some View {
        AdaptiveWorkspace(contentFirst: false) { VStack(alignment: .leading, spacing: 20) {
            Text("Scan the photos you allow access to for Android Motion Photos.").font(.subheadline).foregroundStyle(.secondary)
            if result?.access == .limited {
                VStack(alignment: .leading, spacing: 8) {
                    Label("Only photos you've allowed access to were scanned.", systemImage: "info.circle")
                    Button("Manage Accessible Photos") { managingPhotos = true }
                }.padding(16).frame(maxWidth: .infinity, alignment: .leading).background(Color(uiColor: .secondarySystemGroupedBackground), in: RoundedRectangle(cornerRadius: 22))
            }
            if scanTask != nil {
                VStack(alignment: .leading, spacing: 12) {
                    Text("Looking for Motion Photos…").font(.headline)
                    if let progress, progress.total > 0 {
                        ProgressView(value: Double(progress.completed), total: Double(progress.total))
                        Text("Checked \(progress.completed) of \(progress.total) photos").font(.subheadline).monospacedDigit().foregroundStyle(.secondary)
                    } else { ProgressView() }
                    Button("Stop Scan") { scanTask?.cancel() }
                }
            } else if stopped {
                Text("Scan stopped").font(.headline)
                Text(result?.motionAssetIdentifiers.isEmpty == false ? "Showing the photos found so far." : "No Motion Photos found yet.").foregroundStyle(.secondary)
                if let result, !result.issues.isEmpty { Text("\(result.issues.count) photos couldn't be checked, so results may be incomplete.").font(.subheadline).foregroundStyle(.secondary) }
            } else if let error {
                Text("Couldn't Complete the Scan").font(.headline)
                Text(error).foregroundStyle(.secondary)
            } else if let result {
                if result.access == .denied || result.access == .restricted {
                    Text("Can't access Photos, so your library can't be scanned. You can still choose from Files.").foregroundStyle(.secondary)
                    if result.access == .denied { Button("Open Settings") { if let url = URL(string: UIApplication.openSettingsURLString) { openURL(url) } } }
                } else {
                    Text(result.motionAssetIdentifiers.isEmpty ? "No Motion Photos Found" : "Motion Photos Found").font(.headline)
                    if !result.issues.isEmpty { Text("\(result.issues.count) photos couldn't be checked, so results may be incomplete.").font(.subheadline).foregroundStyle(.secondary) }
                    if result.motionAssetIdentifiers.isEmpty { Text("Some photos may lose their motion data when shared or compressed. Try the original file.").font(.subheadline).foregroundStyle(.secondary) }
                }
            }
            if !identifiers.isEmpty {
                Text("\(selected.count) of \(selectable.count) selected").font(.subheadline).foregroundStyle(.secondary)
                Button(selected.isEmpty ? "Convert" : "Convert \(selected.count) Photos") {
                    let chosen = selectable.filter { selected.contains($0) }.compactMap { id -> BatchInput? in
                        guard let identity = identities[id] else { return nil }
                        return BatchInput(name: identity.filename, source: .scannedPhoto(id, identity))
                    }
                    scanTask?.cancel(); batchWork = BatchWork(inputs: chosen, engine: engine); convertingBatch = true
                }.buttonStyle(.glassProminent).disabled(selected.isEmpty)
            }
            if scanTask == nil {
                Button("Scan Again") { start() }.buttonStyle(.glass)
                Button("Choose from Files", action: chooseFiles).buttonStyle(.glass)
            }
        } } content: {
            if !identifiers.isEmpty {
                LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 4), count: typeSize.isAccessibilitySize ? 2 : (availableWidth >= 700 ? 4 : 3)), spacing: 4) {
                    ForEach(identifiers, id: \.self) { id in
                        Button { if selected.contains(id) { selected.remove(id) } else { selected.insert(id) } } label: {
                            ScanThumbnail(id: id).overlay(alignment: .topTrailing) { Image(systemName: selected.contains(id) ? "checkmark.circle.fill" : "circle").foregroundStyle(selected.contains(id) ? .orange : .white).font(.title2).padding(6) }
                                .overlay { RoundedRectangle(cornerRadius: 8).stroke(selected.contains(id) ? Color.orange : .clear, lineWidth: 3) }
                        }.disabled(reading || !selectable.contains(id)).accessibilityValue(selected.contains(id) ? "Selected" : "Not Selected").accessibilityAddTraits(selected.contains(id) ? .isSelected : [])
                    }
                }
            }
        }.onGeometryChange(for: CGFloat.self) { $0.size.width } action: { availableWidth = $0 }
            .background(Color(uiColor: .systemGroupedBackground))
        .navigationTitle("Find Motion Photos").navigationBarTitleDisplayMode(.inline)
        .toolbar { Button(scanTask != nil ? "Select All Found" : (selected.count == selectable.count && !selectable.isEmpty ? "Deselect All" : "Select All")) { selected = selected.count == selectable.count ? [] : Set(selectable) }.disabled(selectable.isEmpty) }
        .navigationDestination(isPresented: $convertingBatch) {
            if let batchWork { BatchResultsView(work: batchWork, selected: Set(batchWork.rows.map(\.id)), brand: "Google", direction: .androidToLive) { convertingBatch = false; self.batchWork = nil } }
        }
        .background(LimitedLibraryPresenter(isPresented: $managingPhotos) { start() }.frame(width: 0, height: 0))
        .task { if result == nil && progress == nil && scanTask == nil { start() } }.onDisappear {
            scanID = UUID()
            scanTask?.cancel(); scanTask = nil
            selectionTask?.cancel(); selectionTask = nil; reading = false
        }
    }
    private func start() {
        scanTask?.cancel(); selectionTask?.cancel(); selectionTask = nil; reading = false
        result = nil; error = nil; stopped = false; progress = nil; selected = []
        let currentID = UUID(); scanID = currentID
        let scanner = PhotoScanner(source: PhotosLibrarySource()) { resource in
            let url = resource.temporaryFileURL()
            try resource.data.write(to: url); defer { try? FileManager.default.removeItem(at: url) }
            let cancellation = InspectionCancellation()
            let inspected: InspectionResult = try await withTaskCancellationHandler(operation: {
                try await withCheckedThrowingContinuation { continuation in
                    engine.inspect(input: url, cancellation: cancellation) { continuation.resume(with: $0) }
                }
            }, onCancel: { cancellation.cancel() })
            switch inspected.classification { case .motion: return true; case .staticImage: return false; case .unknown: throw ConversionFailure.unavailable }
        }
        scanTask = Task {
            do {
                let found = try await scanner.scan { value in await MainActor.run { if scanID == currentID { progress = value } } }
                guard scanID == currentID else { return }; result = found
            } catch is CancellationError {
                if scanID == currentID {
                    stopped = true
                    if let progress { result = PhotoScanResult(access: progress.access, motionAssetIdentifiers: progress.motionAssetIdentifiers, issues: progress.issues, matchedResources: progress.matchedResources) }
                }
            }
            catch { if scanID == currentID { self.error = error.localizedDescription } }
            if scanID == currentID { scanTask = nil }
        }
    }
    @MainActor private func select(_ id: String, session: UUID) async {
        guard session == scanID, let identity = result?.matchedResources[id] else { return }
        reading = true
        defer { if session == scanID { reading = false; selectionTask = nil } }
        // The worker owns I/O and propagates cancellation to the Photos request and engine.
        let worker = Task.detached(priority: .utility) { () async throws -> URL in
            let resources = try await PhotosLibrarySource().originalResources(for: id)
            try Task.checkCancellation()
            guard resources.indices.contains(identity.resourceIndex) else { throw ConversionFailure.unreadable }
            let resource = resources[identity.resourceIndex]
            guard resource.filename == identity.filename, resource.uniformTypeIdentifier == identity.uniformTypeIdentifier else { throw ConversionFailure.unreadable }
            let url = resource.temporaryFileURL()
            do {
                try resource.data.write(to: url)
                try Task.checkCancellation()
                let cancellation = InspectionCancellation()
                let inspected: InspectionResult = try await withTaskCancellationHandler(operation: {
                    try await withCheckedThrowingContinuation { continuation in
                        engine.inspect(input: url, cancellation: cancellation) { continuation.resume(with: $0) }
                    }
                }, onCancel: { cancellation.cancel() })
                try Task.checkCancellation()
                switch inspected.classification { case .motion: break; case .staticImage, .unknown: throw ConversionFailure.unreadable }
                return url
            } catch {
                try? FileManager.default.removeItem(at: url)
                throw error
            }
        }
        do {
            let url = try await withTaskCancellationHandler(operation: { try await worker.value }, onCancel: { worker.cancel() })
            guard !Task.isCancelled, session == scanID else { try? FileManager.default.removeItem(at: url); return }
            onSelect(url)
        } catch is CancellationError {
            // Leaving the screen or starting a new scan cancels this selection.
        } catch {
            if !Task.isCancelled && session == scanID { stopped = false; self.error = String(localized: "Can't Read the Original Photo") }
        }
    }

}
struct ScanThumbnail: View {
    let id: String
    @State private var image: UIImage?
    var body: some View {
        ZStack(alignment: .bottomLeading) {
            Rectangle().fill(Color(uiColor: .tertiarySystemFill)).aspectRatio(1, contentMode: .fit).overlay { if let image { GeometryReader { geo in Image(uiImage: image).resizable().scaledToFill().frame(width: geo.size.width, height: geo.size.height).clipped() } } }
            Image(systemName: "livephoto").foregroundStyle(.white).padding(6).background(.black.opacity(0.8), in: Capsule()).padding(5)
        }.clipShape(RoundedRectangle(cornerRadius: 8)).accessibilityLabel("Motion Photo").task {
            guard let asset = PHAsset.fetchAssets(withLocalIdentifiers: [id], options: nil).firstObject else { return }
            PHImageManager.default().requestImage(for: asset, targetSize: CGSize(width: 300, height: 300), contentMode: .aspectFill, options: nil) { image, _ in self.image = image }
        }
    }
}

// Presents from a controller installed in this SwiftUI hierarchy, not from a global scene.
private struct LimitedLibraryPresenter: UIViewControllerRepresentable {
    @Binding var isPresented: Bool
    let onFinished: () -> Void
    func makeUIViewController(context: Context) -> UIViewController { UIViewController() }
    func updateUIViewController(_ controller: UIViewController, context: Context) {
        guard isPresented, !context.coordinator.presenting, controller.viewIfLoaded?.window != nil else { return }
        context.coordinator.presenting = true
        PHPhotoLibrary.shared().presentLimitedLibraryPicker(from: controller) { _ in
            DispatchQueue.main.async {
                context.coordinator.presenting = false
                isPresented = false
                onFinished()
            }
        }
    }
    func makeCoordinator() -> Coordinator { Coordinator() }
    final class Coordinator { var presenting = false }
}
