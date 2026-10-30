import SwiftUI
import Photos
import ImageIO

struct BatchSelectionView: View {
    let inputs: [BatchInput]
    let direction: Direction
    let chooseFiles: () -> Void
    @StateObject private var work: BatchWork
    @State private var selected = Set<UUID>()
    @State private var started = false
    @State private var brand = "Google"
    @State private var width: CGFloat = 0
    @Environment(\.dynamicTypeSize) private var typeSize
    @Environment(\.dismiss) private var dismiss

    init(inputs: [BatchInput], direction: Direction, engine: any ConversionEngine, folder: BatchFolderAccess? = nil, chooseFiles: @escaping () -> Void) {
        self.inputs = inputs; self.direction = direction; self.chooseFiles = chooseFiles
        _work = StateObject(wrappedValue: BatchWork(inputs: inputs, engine: engine, folder: folder))
    }
    private var available: [BatchInput] { inputs.filter { work.eligibility[$0.id] == nil } }
    var body: some View {
        AdaptiveWorkspace(contentFirst: false) {
            VStack(alignment: .leading, spacing: 16) {
                Text("\(selected.count) of \(available.count) selected").font(.subheadline).foregroundStyle(.secondary)
                if !work.eligibility.isEmpty { Text("\(work.eligibility.count) photos can't be converted").font(.subheadline).foregroundStyle(.secondary) }
                if work.checking { ProgressView("Checking originals…") }
                if direction == .liveToAndroid {
                    Picker("Recipient's phone brand", selection: $brand) {
                        ForEach(["Google", "Xiaomi", "Samsung", "HUAWEI", "HONOR", "OPPO", "vivo"], id: \.self) { Text($0).tag($0) }
                    }
                }
                if available.isEmpty && !work.checking {
                    Label("No Photos to Convert", systemImage: "photo.on.rectangle.angled").font(.headline)
                    Text("Sharing or compression may remove motion data. Try the original files.").foregroundStyle(.secondary)
                    Button("Choose from Files", action: chooseFiles)
                }
                Button { started = true } label: { Text(selected.isEmpty ? "Convert" : "Convert \(selected.count) Photos").frame(maxWidth: .infinity, minHeight: 54) }
                    .buttonStyle(.glassProminent).disabled(selected.isEmpty || work.checking)
            }
        } content: {
            LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 4), count: typeSize.isAccessibilitySize ? 2 : (width >= 700 ? 4 : 3)), spacing: 4) {
                ForEach(Array(inputs.enumerated()), id: \.element.id) { index, item in
                    Button {
                        if selected.contains(item.id) { selected.remove(item.id) } else { selected.insert(item.id) }
                    } label: {
                        ZStack(alignment: .topTrailing) {
                            BatchThumbnail(item: item, file: work.thumbnailURL(for: item.id))
                                .overlay(alignment: .bottomLeading) { Text(item.name).font(.caption2).lineLimit(1).padding(4).background(.regularMaterial) }
                            Image(systemName: selected.contains(item.id) ? "checkmark.circle.fill" : "circle").font(.title2).foregroundStyle(selected.contains(item.id) ? .orange : .white).padding(6)
                        }.clipShape(RoundedRectangle(cornerRadius: 8)).overlay { RoundedRectangle(cornerRadius: 8).stroke(selected.contains(item.id) ? Color.orange : .clear, lineWidth: 3) }.opacity(work.eligibility[item.id] == nil ? 1 : 0.4)
                    }.buttonStyle(.plain).disabled(work.checking || work.eligibility[item.id] != nil)
                        .accessibilityLabel("Motion Photo, photo \(index + 1)")
                        .accessibilityValue(work.eligibility[item.id] ?? (selected.contains(item.id) ? "Selected" : "Not Selected"))
                        .accessibilityAddTraits(selected.contains(item.id) ? .isSelected : [])
                }
            }
        }.onGeometryChange(for: CGFloat.self) { $0.size.width } action: { width = $0 }
            .navigationTitle("Choose Photos to Convert").navigationBarTitleDisplayMode(.inline)
            .toolbar { Button(selected.count == available.count && !available.isEmpty ? "Deselect All" : "Select All") { selected = selected.count == available.count ? [] : Set(available.map(\.id)) }.disabled(work.checking || available.isEmpty) }
            .task { await work.checkInputs(); selected = Set(available.map(\.id)) }
            .onDisappear { if !started { work.discardWhenIdle() } }
            .navigationDestination(isPresented: $started) { BatchResultsView(work: work, selected: selected, brand: brand, direction: direction) { started = false; dismiss() } }
    }
}

struct BatchResultsView: View {
    @ObservedObject var work: BatchWork
    let selected: Set<UUID>
    let brand: String
    let direction: Direction
    let finish: () -> Void
    @State private var stop = false
    @State private var leaving = false
    @State private var share = false
    @State private var export = false
    @State private var individual: ConversionOutput?
    @State private var saved = Set<UUID>()
    @State private var saveFailures: [UUID: String] = [:]
    @State private var saving = false
    @State private var savedCount = 0
    @State private var previous: [BatchRowResult] = []
    @State private var runID = UUID()
    @State private var retryIDs: Set<UUID>?
    @State private var started = false
    private var rows: [BatchRowResult] { previous + work.rows }
    private var converted: [(UUID, ConversionOutput)] { rows.compactMap { if case .converted(let output) = $0.state { return ($0.id, output) }; return nil } }
    private var failed: [BatchRowResult] { rows.filter { if case .failed = $0.state { return true }; return false } }
    private var files: [URL] { converted.flatMap { $0.1.files } }
    var body: some View {
        List {
            Section {
                if work.running || !started {
                    Text("Processed \(work.completed) of \(work.rows.count) photos").font(.headline.monospacedDigit())
                    ProgressView(value: Double(work.completed), total: Double(max(work.rows.count, 1))).tint(.orange)
                } else {
                    Label("\(converted.count) converted, \(failed.count) failed", systemImage: failed.isEmpty ? "checkmark.circle.fill" : "exclamationmark.circle.fill").font(.headline)
                }
            }
            Section {
                ForEach(rows) { row in
                    HStack(alignment: .top) {
                        Group {
                            if case .converted(let output) = row.state, let image = output.image {
                                Image(uiImage: image).resizable().scaledToFill()
                            } else { Image(systemName: "photo").foregroundStyle(.secondary) }
                        }.frame(width: 44, height: 44).background(Color(uiColor: .tertiarySystemFill)).clipShape(RoundedRectangle(cornerRadius: 8))
                        VStack(alignment: .leading) {
                            Text(row.name)
                            switch row.state {
                            case .pending: Label("Waiting", systemImage: "clock").foregroundStyle(.secondary)
                            case .converting: ProgressView("Converting")
                            case .failed(let message): Label("Failed", systemImage: "xmark.octagon.fill").foregroundStyle(.red); Text(message).font(.footnote).foregroundStyle(.secondary)
                            case .cancelled: Label("Not Converted", systemImage: "minus.circle").foregroundStyle(.secondary)
                            case .converted(let output):
                                Label(saved.contains(row.id) ? "Saved to Photos" : "Done", systemImage: "checkmark.circle.fill").foregroundStyle(.green)
                                if let message = saveFailures[row.id] { Text("Couldn't Save: \(message)").font(.footnote).foregroundStyle(.secondary) }
                                Button("Preview") { individual = output }
                            }
                        }
                        Spacer()
                        if case .converted(let output) = row.state { ShareLink(items: output.files) { Image(systemName: "square.and.arrow.up").frame(width: 44, height: 44) } }
                    }
                }
            }
            if !work.running && started {
                Section {
                    if !failed.isEmpty { Button("Retry \(failed.count) Failed Photos") { previous = rows.filter { row in !failed.contains(where: { $0.id == row.id }) }; retryIDs = Set(failed.map(\.id)); runID = UUID() }.disabled(saving) }
                    if !converted.isEmpty {
                        if direction == .androidToLive {
                            Button(saving ? "Saving \(savedCount) of \(converted.count)" : (saved.count == converted.count ? "Saved to Photos" : "Save All \(converted.count) to Photos")) { Task { await saveAll() } }.disabled(saving || saved.count == converted.count)
                        }
                        Button(direction == .liveToAndroid ? "Share \(files.count) Files" : "Share") { share = true }
                        if direction == .liveToAndroid { Button("Save to Files") { export = true } }
                    }
                }
            }
        }.navigationTitle(work.running || !started ? "Converting" : "Conversion Results")
            .navigationBarBackButtonHidden()
            .toolbar { ToolbarItem(placement: .topBarLeading) { if work.running { Button("Stop") { stop = true } } }; ToolbarItem(placement: .topBarTrailing) { if !work.running && started { Button("Done") { leaving = true }.disabled(saving) } } }
            .confirmationDialog("Stop converting?", isPresented: $stop) { Button("Stop Converting", role: .destructive) { work.stopAfterCurrent() }; Button("Continue Converting", role: .cancel) {} } message: { Text("Completed photos will be kept. The current conversion will finish; remaining photos won't be converted.") }
            .confirmationDialog("Leave Conversion Results?", isPresented: $leaving) { Button("Leave", role: .destructive) { if !saving { work.discard(); finish() } }; Button("Stay", role: .cancel) {} } message: { Text("Converted results that haven't been shared or saved will be cleared when you leave.") }
            .sheet(isPresented: $share) { ShareSheet(items: files) }
            .sheet(isPresented: $export) { ExportFileSheet(urls: files) }
            .sheet(item: $individual) { output in NavigationStack { ResultView(result: output, finish: { individual = nil }) } }
            .task(id: runID) { started = false; await work.start(brand: brand, selected: retryIDs ?? selected); started = true }
    }
    @MainActor private func saveAll() async {
        saving = true; savedCount = saved.count; defer { saving = false }
        for (id, output) in converted where !saved.contains(id) {
            guard output.files.count >= 2 else { saveFailures[id] = String(localized: "Couldn't find converted files."); continue }
            do {
                try await PHPhotoLibrary.shared().performChanges {
                    let request = PHAssetCreationRequest.forAsset()
                    request.addResource(with: .photo, fileURL: output.files[0], options: nil)
                    request.addResource(with: .pairedVideo, fileURL: output.files[1], options: nil)
                }
                saved.insert(id); savedCount += 1; saveFailures.removeValue(forKey: id)
            } catch { saveFailures[id] = error.localizedDescription }
        }
    }
}

private struct BatchThumbnail: View {
    let item: BatchInput
    let file: URL?
    @State private var image: UIImage?
    var body: some View {
        Rectangle().fill(Color(uiColor: .tertiarySystemFill)).aspectRatio(1, contentMode: .fit)
            .overlay {
                if let image {
                    GeometryReader { geo in
                        Image(uiImage: image).resizable().scaledToFill().frame(width: geo.size.width, height: geo.size.height).clipped()
                    }
                } else { Image(systemName: "livephoto").font(.title).foregroundStyle(.secondary) }
            }.task(id: file) {
                guard let file else { return }
                image = await Task.detached(priority: .utility) {
                    guard let source = CGImageSourceCreateWithURL(file as CFURL, nil),
                          let thumbnail = CGImageSourceCreateThumbnailAtIndex(source, 0, [kCGImageSourceCreateThumbnailFromImageAlways: true, kCGImageSourceThumbnailMaxPixelSize: 300, kCGImageSourceCreateThumbnailWithTransform: true] as CFDictionary) else { return nil as UIImage? }
                    return UIImage(cgImage: thumbnail)
                }.value
            }
    }
}
