import SwiftUI
import PhotosUI
import UniformTypeIdentifiers

struct MSRootView: View {
    @State private var flow: MSFlow
    @State private var pickedPhotos: [PhotosPickerItem] = []

    init(engine: any ConversionEngine) { _flow = State(initialValue: MSFlow(engine: engine)) }

    var body: some View {
        @Bindable var flow = flow
        NavigationStack(path: $flow.path) {
            MSHomeView()
                .navigationDestination(for: MSRoute.self) { route in
                    switch route {
                    case .select: MSSelectionView()
                    case .scan: MSScanView()
                    case .brand: MSBrandView()
                    case .converting: MSConvertingView()
                    case .results: MSResultsView()
                    }
                }
        }
        .environment(flow)
        .tint(.msLink)
        .photosPicker(isPresented: $flow.pickingPhotos, selection: $pickedPhotos, maxSelectionCount: nil, selectionBehavior: .ordered,
                      matching: flow.direction == .liveToAndroid ? .livePhotos : .images, photoLibrary: .shared())
        .onChange(of: pickedPhotos) { _, picked in
            guard !picked.isEmpty else { return }
            pickedPhotos = []
            Task { await flow.photosPicked(picked) }
        }
        .fileImporter(isPresented: $flow.pickingFiles, allowedContentTypes: importTypes, allowsMultipleSelection: true) { result in
            Task { await flow.filesPicked(result) }
        }
        .alert(flow.notice ?? "", isPresented: Binding(get: { flow.notice != nil }, set: { if !$0 { flow.notice = nil; flow.noticeMessage = nil } })) {
            Button("好", role: .cancel) {}
        } message: {
            if let message = flow.noticeMessage { Text(message) }
        }
    }

    /// Live Photo pairs need the still and its movie; Motion Photos are a single image.
    private var importTypes: [UTType] {
        flow.direction == .liveToAndroid ? [.jpeg, .heic, .heif, .quickTimeMovie, .mpeg4Movie] : [.jpeg, .heic, .heif]
    }
}

// MARK: - Home v2 (home-v2.md: one line of explanation, two large buttons)

struct MSHomeView: View {
    @Environment(MSFlow.self) private var flow
    @State private var sourceFor: MSDirection?
    @State private var about = false

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                Text("留刻")
                    .font(.largeTitle.bold())
                    .accessibilityAddTraits(.isHeader)
                    .dynamicTypeSize(...DynamicTypeSize.accessibility2)
                Text("iPhone 实况照片与安卓动态照片互相转换。")
                    .font(.body)
                    .foregroundStyle(Color(uiColor: .secondaryLabel))
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(.top, 6)
                    .dynamicTypeSize(...DynamicTypeSize.accessibility2)
                VStack(spacing: 16) {
                    MSHomeButton(direction: .androidToLive) { sourceFor = .androidToLive }
                    MSHomeButton(direction: .liveToAndroid) { sourceFor = .liveToAndroid }
                }
                .padding(.top, 28)
            }
            .padding(.horizontal, 20)
            .padding(.bottom, 20)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .background(Color.msBackground)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button { about = true } label: { Image(systemName: "info.circle").frame(width: 44, height: 44) }
                    .accessibilityLabel("说明")
            }
        }
        .confirmationDialog(sourceFor?.buttonTitle ?? "", isPresented: Binding(get: { sourceFor != nil }, set: { if !$0 { sourceFor = nil } }), titleVisibility: .visible, presenting: sourceFor) { direction in
            Button("从照片中选择") { flow.open(direction, .photos) }
            Button("从文件中选择") { flow.open(direction, .files) }
            if direction == .androidToLive { Button("扫描相册") { flow.open(direction, .scan) } }
            Button("取消", role: .cancel) {}
        }
        .sheet(isPresented: $about) { MSAboutView() }
    }
}

struct MSHomeButton: View {
    let direction: MSDirection
    let action: () -> Void
    @ScaledMetric(relativeTo: .title2) private var height: CGFloat = 132
    @ScaledMetric(relativeTo: .title2) private var diameter: CGFloat = 56
    @ScaledMetric private var gap: CGFloat = 12

    var body: some View {
        let d = min(diameter, 88)
        Button(action: action) {
            VStack(spacing: gap) {
                Circle().fill(Color.msAccentFill)
                    .frame(width: d, height: d)
                    .overlay {
                        Image(systemName: direction.homeSymbol)
                            .font(.system(size: d * 0.5, weight: .semibold))
                            .foregroundStyle(Color.msOnAccent)
                    }
                    .accessibilityHidden(true)
                Text(direction.buttonTitle)
                    .font(.title2.weight(.semibold))
                    .foregroundStyle(Color(uiColor: .label))
                    .multilineTextAlignment(.center)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .padding(.vertical, 20)
            .padding(.horizontal, 16)
            .frame(maxWidth: .infinity, minHeight: min(height, 210))
            .contentShape(RoundedRectangle(cornerRadius: 22, style: .continuous))
        }
        .buttonStyle(MSHomeButtonStyle())
        .accessibilityHint("选择照片来源")
    }
}

/// Pressed state only changes colour, no scaling (also under Reduce Motion).
struct MSHomeButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .background {
                RoundedRectangle(cornerRadius: 22, style: .continuous)
                    .fill(Color.msSurface)
                    .overlay {
                        if configuration.isPressed {
                            RoundedRectangle(cornerRadius: 22, style: .continuous).fill(Color(uiColor: .tertiarySystemFill))
                        }
                    }
            }
    }
}

/// Icon left of text normally; at accessibility sizes icon on top, text full width (same rule as Home rows).
struct MSAboutRow: View {
    let text: LocalizedStringKey
    let symbol: String
    @Environment(\.dynamicTypeSize) private var size

    var body: some View {
        let layout = size.isAccessibilitySize
            ? AnyLayout(VStackLayout(alignment: .leading, spacing: 8))
            : AnyLayout(HStackLayout(alignment: .firstTextBaseline, spacing: 16))
        layout {
            Image(systemName: symbol)
                .foregroundStyle(Color.msLink)
                .frame(minWidth: 28, alignment: .leading)
                .accessibilityHidden(true)
            Text(text)
                .frame(maxWidth: .infinity, alignment: .leading)
                .fixedSize(horizontal: false, vertical: true)
        }
        .accessibilityElement(children: .combine)
    }
}

struct MSAboutView: View {
    @Environment(\.dismiss) private var dismiss
    var body: some View {
        NavigationStack {
            List {
                Section {
                    MSAboutRow(text: "照片只在本机处理，不会上传。", symbol: "lock")
                    MSAboutRow(text: "也可以从照片或文件的分享菜单打开留刻。", symbol: "square.and.arrow.up")
                }
                Section {
                    LabeledContent("版本", value: Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "1.0")
                }
                Section { MadeWithAirBuild() }
            }
            .navigationTitle("说明")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button("完成") { dismiss() } } }
        }
        .tint(.msLink)
    }
}

// MARK: - Selection (design.md §6.2)

struct MSSelectionView: View {
    @Environment(MSFlow.self) private var flow
    @Environment(\.dynamicTypeSize) private var typeSize

    private var ids: [UUID] { flow.candidates.map(\.id) }
    private var allSelected: Bool { !ids.isEmpty && flow.selected.count == ids.count }

    var body: some View {
        List {
            Section {
                if flow.source == .files {
                    ForEach(flow.candidates) { fileRow($0) }
                } else {
                    grid.listRowInsets(EdgeInsets()).listRowBackground(Color.clear)
                }
            } header: {
                // Explicit secondaryLabel and a size cap: list headers are drawn lighter and grow large.
                Text("已选 \(flow.selected.count) / \(ids.count) 张")
                    .font(.subheadline).foregroundStyle(Color(uiColor: .secondaryLabel)).textCase(nil).monospacedDigit()
                    .dynamicTypeSize(...DynamicTypeSize.accessibility1)
            }
            if flow.direction == .liveToAndroid {
                Section {
                    NavigationLink(value: MSRoute.brand) {
                        LabeledContent("对方的手机品牌") { Text(MSBrand.named(flow.brandID).name) }
                    }
                    .frame(minHeight: 44)
                } footer: {
                    Text("请按对方设备选择品牌，实际支持以文件及设备为准。")
                }
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle("选择照片")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button(allSelected ? "取消全选" : "全选") {
                    flow.selected = allSelected ? [] : Set(ids)
                }
            }
        }
        .safeAreaInset(edge: .bottom) {
            MSBottomBar {
                Button { flow.startConversion() } label: {
                    Text(flow.selected.isEmpty ? "转换" : "转换 \(flow.selected.count) 张")
                }
                .buttonStyle(MSPrimaryButtonStyle())
                .disabled(flow.selected.isEmpty)
                Text(flow.direction.title).font(.footnote).foregroundStyle(.secondary)
            }
        }
    }

    private var grid: some View {
        let columns = typeSize.isAccessibilitySize ? 2 : 3
        return LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 4), count: columns), spacing: 4) {
            ForEach(Array(flow.candidates.enumerated()), id: \.element.id) { index, candidate in
                let id = candidate.id
                let isOn = flow.selected.contains(id)
                Button { toggle(id) } label: {
                    RoundedRectangle(cornerRadius: 2)
                        .fill(Color(uiColor: .tertiarySystemFill))
                        .aspectRatio(1, contentMode: .fit)
                        .overlay {
                            if let image = flow.thumbnail(for: id) {
                                Image(uiImage: image).resizable().scaledToFill()
                            } else {
                                VStack(spacing: 6) {
                                    Image(systemName: "photo").font(.title2).foregroundStyle(.secondary)
                                    Text(candidate.name).font(.caption2).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                                        .dynamicTypeSize(...DynamicTypeSize.xxxLarge).padding(.horizontal, 6)
                                }
                            }
                        }
                        .clipShape(RoundedRectangle(cornerRadius: 2))
                        .overlay { if isOn { RoundedRectangle(cornerRadius: 2).strokeBorder(Color.msAccentFill, lineWidth: 2) } }
                        .overlay(alignment: .topTrailing) { selectionMark(isOn).padding(6) }
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(flow.direction == .liveToAndroid ? "实况照片，第 \(index + 1) 张，\(candidate.name)" : "动态照片，第 \(index + 1) 张，\(candidate.name)")
                .accessibilityValue(isOn ? "已选中" : "未选中")
                .accessibilityAddTraits(isOn ? [.isButton, .isSelected] : .isButton)
                .task(id: id) { await flow.loadThumbnail(id) }
            }
        }
        .padding(4)
    }

    private func fileRow(_ candidate: MSCandidate) -> some View {
        let id = candidate.id
        let isOn = flow.selected.contains(id)
        let side: CGFloat = typeSize.isAccessibilitySize ? 32 : 44
        let thumb = RoundedRectangle(cornerRadius: 6).fill(Color(uiColor: .tertiarySystemFill))
            .frame(width: side, height: side)
            .overlay {
                if let image = flow.thumbnail(for: id) {
                    Image(uiImage: image).resizable().scaledToFill().frame(width: side, height: side)
                } else {
                    Image(systemName: "doc").foregroundStyle(.secondary).imageScale(.small)
                }
            }
            .clipShape(RoundedRectangle(cornerRadius: 6))
        let name = Text(candidate.name).lineLimit(1).truncationMode(.middle).foregroundStyle(.primary)
        return Button { toggle(id) } label: {
            Group {
                if typeSize.isAccessibilitySize {
                    VStack(alignment: .leading, spacing: 6) {
                        HStack { thumb; Spacer(); selectionMark(isOn) }
                        name.frame(maxWidth: .infinity, alignment: .leading)
                    }
                    .padding(.vertical, 6)
                } else {
                    HStack(spacing: 12) { thumb; name; Spacer(); selectionMark(isOn) }
                }
            }
            .frame(minHeight: 56).contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(candidate.name)
        .accessibilityValue(isOn ? "已选中" : "未选中")
        .accessibilityAddTraits(isOn ? [.isButton, .isSelected] : .isButton)
        .task(id: id) { await flow.loadThumbnail(id) }
    }

    private func selectionMark(_ isOn: Bool) -> some View {
        ZStack {
            Circle().fill(isOn ? Color.msAccentFill : Color.black.opacity(0.15))
            Circle().strokeBorder(.white, lineWidth: 2)
            if isOn { Image(systemName: "checkmark").font(.system(size: 12, weight: .bold)).foregroundStyle(Color.msOnAccent) }
        }
        .frame(width: 24, height: 24)
        .accessibilityHidden(true)
    }

    private func toggle(_ id: UUID) {
        if flow.selected.contains(id) { flow.selected.remove(id) } else { flow.selected.insert(id) }
    }
}

// MARK: - Scan (design.md §6.3)

struct MSScanView: View {
    @Environment(MSFlow.self) private var flow
    @Environment(\.dismiss) private var dismiss
    @State private var checked = 0
    @State private var total = 0
    @State private var found: [String] = []
    @State private var identities: [String: PhotoResourceIdentity] = [:]
    @State private var unreadable = 0
    @State private var scanning = true

    var body: some View {
        List {
            Section {
                VStack(alignment: .leading, spacing: 10) {
                    Text(scanning ? "正在扫描相册…" : "扫描完成").font(.headline)
                        .frame(maxWidth: .infinity, alignment: .leading)
                    Text("已检查 \(checked) / \(total) 张").font(.subheadline).foregroundStyle(.secondary).monospacedDigit()
                        .lineLimit(1).minimumScaleFactor(0.7)
                    ProgressView(value: Double(checked), total: Double(max(total, 1))).tint(.msAccentFill)
                        .accessibilityLabel("扫描进度")
                        .accessibilityValue("已检查 \(checked) 张，共 \(total) 张")
                }
                .padding(.vertical, 6)
                .frame(maxWidth: .infinity, alignment: .leading)
                .dynamicTypeSize(...DynamicTypeSize.accessibility2)
            }
            Section {
                if scanning {
                    Text("已找到 \(found.count) 张照片，扫描结束后可选择。")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .dynamicTypeSize(...DynamicTypeSize.accessibility2)
                } else {
                    Button("选择已找到的 \(found.count) 张") { flow.showScanResults(found, identities) }
                        .frame(minHeight: 44)
                        .disabled(found.isEmpty)
                }
            } footer: {
                // Same font and size cap as the "已检查" line so it never outranks the card title.
                VStack(alignment: .leading, spacing: 6) {
                    if !scanning && unreadable > 0 {
                        Text("Originals for \(unreadable) photos couldn't be read, so results may be incomplete.")
                    }
                    Label("只检查你允许访问的照片原件。", systemImage: "info.circle")
                }
                .font(.subheadline)
                .dynamicTypeSize(...DynamicTypeSize.accessibility2)
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle("扫描相册")
        .navigationBarTitleDisplayMode(.inline)
        .navigationBarBackButtonHidden(scanning)
        .toolbar {
            if scanning {
                ToolbarItem(placement: .topBarLeading) { Button("取消") { scanning = false; dismiss() } }
            }
        }
        .task { await scan() }
    }

    private func scan() async {
        do {
            let result = try await flow.scanLibrary { progress in
                checked = progress.completed; total = progress.total
                found = progress.motionAssetIdentifiers; identities = progress.matchedResources
            }
            guard result.access == .authorized || result.access == .limited else {
                flow.notice = String(localized: "Couldn't Complete the Scan")
                flow.noticeMessage = String(localized: "Can't access Photos, so your library can't be scanned. You can still choose from Files.")
                scanning = false; dismiss()
                return
            }
            found = result.motionAssetIdentifiers; identities = result.matchedResources
            unreadable = result.issues.count
            checked = total
            scanning = false
        } catch is CancellationError {
        } catch {
            guard !Task.isCancelled else { return }
            flow.notice = String(localized: "Couldn't Complete the Scan")
            flow.noticeMessage = error.localizedDescription
            scanning = false; dismiss()
        }
    }
}

// MARK: - Brand (design.md §6.2)

struct MSBrandView: View {
    @Environment(MSFlow.self) private var flow
    @Environment(\.dismiss) private var dismiss
    var body: some View {
        List {
            Section {
                ForEach(MSBrand.all) { brand in
                    Button {
                        flow.brandID = brand.id
                        dismiss()
                    } label: {
                        HStack {
                            Text(brand.name).foregroundStyle(Color(uiColor: .label))
                            Spacer()
                            if flow.brandID == brand.id { Image(systemName: "checkmark").foregroundStyle(Color.msLink) }
                        }
                        .frame(minHeight: 44).contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityAddTraits(flow.brandID == brand.id ? .isSelected : [])
                }
            } footer: {
                Text("品牌只决定输出格式。")
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle("对方的手机品牌")
        .navigationBarTitleDisplayMode(.inline)
    }
}

// MARK: - Converting (design.md §6.4)

struct MSConvertingView: View {
    @Environment(MSFlow.self) private var flow
    private var succeededText: some View { Text("成功 \(flow.succeeded) 张").lineLimit(1) }
    private var failedText: some View { Text("失败 \(flow.failed) 张").lineLimit(1) }
    @State private var confirmStop = false

    var body: some View {
        List {
            Section {
                VStack(alignment: .leading, spacing: 8) {
                    Text(flow.direction.title).font(.footnote).foregroundStyle(.secondary)
                    Text("已处理 \(flow.processed) / \(flow.items.count) 张").font(.title2.weight(.semibold)).monospacedDigit()
                        .lineLimit(1).minimumScaleFactor(0.8)
                    // Two phrases so a line never breaks inside a word.
                    ViewThatFits(in: .horizontal) {
                        HStack(spacing: 6) { succeededText; Text("·"); failedText }
                        VStack(alignment: .leading, spacing: 2) { succeededText; failedText }
                    }
                    .font(.subheadline).foregroundStyle(.secondary)
                    ProgressView(value: Double(flow.processed), total: Double(max(flow.items.count, 1)))
                        .tint(.msAccentFill)
                        .padding(.top, 6)
                        .accessibilityLabel("转换进度")
                        .accessibilityValue("已处理 \(flow.processed) 张，共 \(flow.items.count) 张")
                }
                .dynamicTypeSize(...DynamicTypeSize.accessibility1)
                .listRowBackground(Color.clear)
                .listRowInsets(EdgeInsets(top: 4, leading: 4, bottom: 4, trailing: 4))
            }
            .listSectionSpacing(.custom(24))
            Section {
                ForEach(flow.items) { MSItemRow(item: $0, showShare: false) }
            } header: {
                Text("转换明细").font(.footnote).dynamicTypeSize(...DynamicTypeSize.xxxLarge)
            } footer: {
                Text("请保持留刻在前台")
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle("正在转换")
        .navigationBarTitleDisplayMode(.inline)
        .navigationBarBackButtonHidden(true)
        .interactiveDismissDisabled()
        .toolbar {
            ToolbarItem(placement: .topBarLeading) {
                Button(flow.stopping ? "正在停止…" : "停止") { confirmStop = true }.disabled(flow.stopping)
            }
        }
        .alert("要停止转换吗？", isPresented: $confirmStop) {
            Button("继续转换", role: .cancel) {}
            Button("停止转换", role: .destructive) { flow.stop() }
        } message: {
            Text("当前这张完成后停止，已完成的结果会保留。")
        }
        .task(id: flow.runID) { await flow.run() }
    }
}

// MARK: - Results (design.md §6.5)

struct MSResultsView: View {
    enum Filter: Hashable { case all, success, failed }
    @Environment(MSFlow.self) private var flow
    @State private var filter: Filter = .all
    @State private var confirmLeave = false
    @Environment(\.dynamicTypeSize) private var typeSize
    @ScaledMetric(relativeTo: .title) private var badge: CGFloat = 48

    private var rows: [MSItem] {
        switch filter {
        case .all: flow.items
        case .success: flow.items.filter { $0.status == .done }
        case .failed: flow.items.filter(\.isFailed)
        }
    }

    var body: some View {
        List {
            Section {
                VStack(alignment: typeSize.isAccessibilitySize ? .leading : .center, spacing: 8) {
                    Circle().fill(Color(uiColor: .tertiarySystemFill)).frame(width: badge, height: badge)
                        .overlay { Image(systemName: flow.succeeded > 0 ? "checkmark" : "xmark").font(.system(size: badge * 0.5, weight: .semibold)).foregroundStyle(flow.succeeded > 0 ? Color.green : Color.red) }
                        .overlay(alignment: .bottomTrailing) {
                            if flow.failed > 0 && flow.succeeded > 0 {
                                // Sits on the circle's rim, clear of the checkmark.
                                Image(systemName: "exclamationmark.triangle.fill").symbolRenderingMode(.multicolor)
                                    .font(.system(size: badge * 0.3))
                                    .padding(badge * 0.06)
                                    .background(Circle().fill(Color.msBackground))
                                    .offset(x: badge * 0.04, y: badge * 0.04)
                            }
                        }
                        .accessibilityHidden(true)
                    // Two phrases so a line never breaks inside a word.
                    let headline = Group {
                        Text("\(flow.succeeded) 张已转换")
                        if flow.failed > 0 { Text("\(flow.failed) 张失败") }
                    }
                    .font(.title2.weight(.semibold))
                    if typeSize.isAccessibilitySize {
                        VStack(alignment: .leading, spacing: 2) { headline }
                    } else {
                        ViewThatFits { HStack(spacing: 8) { headline }; VStack(spacing: 2) { headline } }
                    }
                    Text(flow.skipped > 0 ? "\(flow.skipped) 张未转换" : (flow.direction == .liveToAndroid ? "转成动态照片 · 文件已就绪" : "转成实况照片 · 可以存入照片"))
                        .font(.subheadline).foregroundStyle(.secondary)
                        .multilineTextAlignment(typeSize.isAccessibilitySize ? .leading : .center)
                }
                .frame(maxWidth: .infinity, alignment: typeSize.isAccessibilitySize ? .leading : .center)
                .padding(.top, 8)
                .dynamicTypeSize(...DynamicTypeSize.accessibility1)
                .listRowBackground(Color.clear)
                .listRowInsets(EdgeInsets(top: 0, leading: 4, bottom: 0, trailing: 4))
                .accessibilityElement(children: .combine)
            }
            .listSectionSpacing(.custom(24))
            Section {
                ForEach(rows) { MSItemRow(item: $0, showShare: flow.direction == .liveToAndroid) }
            }
            if typeSize.isAccessibilitySize && flow.succeeded > 0 {
                Section {
                    if flow.direction == .liveToAndroid {
                        Button { flow.exportToFiles() } label: { Text("存到文件").frame(maxWidth: .infinity, minHeight: 44, alignment: .leading).contentShape(Rectangle()) }
                    }
                    if flow.failed > 0 {
                        Button { filter = .all; flow.retryFailed() } label: { Text("重试失败的 \(flow.failed) 张").frame(maxWidth: .infinity, minHeight: 44, alignment: .leading).contentShape(Rectangle()) }
                    }
                }
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle("转换结果")
        .navigationBarTitleDisplayMode(.inline)
        .navigationBarBackButtonHidden(true)
        .toolbar {
            if flow.failed > 0 && flow.succeeded > 0 {
                ToolbarItem(placement: .topBarLeading) {
                    Menu {
                        Picker("筛选", selection: $filter) { filterOptions }.pickerStyle(.inline)
                    } label: {
                        Image(systemName: "line.3.horizontal.decrease.circle").frame(width: 44, height: 44)
                    }
                    .accessibilityLabel(filterLabel)
                }
            }
            ToolbarItem(placement: .topBarTrailing) { Button("完成") { confirmLeave = true } }
        }
        .safeAreaInset(edge: .bottom) {
            MSBottomBar {
                if flow.succeeded == 0 {
                    Button("重试失败的 \(flow.failed) 张") { filter = .all; flow.retryFailed() }
                        .buttonStyle(MSPrimaryButtonStyle())
                } else {
                    Button(flow.direction == .liveToAndroid ? "分享 \(flow.succeeded) 个文件" : "全部存入照片（\(flow.succeeded)）") {
                        if flow.direction == .liveToAndroid { flow.shareAll() } else { Task { await flow.saveAllToPhotos() } }
                    }
                    .buttonStyle(MSPrimaryButtonStyle())
                    .disabled(flow.saving)
                    if !typeSize.isAccessibilitySize {
                    HStack(spacing: 8) {
                        if flow.direction == .liveToAndroid {
                            Button { flow.exportToFiles() } label: { secondaryLabel("存到文件") }
                        }
                        if flow.direction == .liveToAndroid && flow.failed > 0 { Divider().frame(height: 16) }
                        if flow.failed > 0 {
                            Button { filter = .all; flow.retryFailed() } label: { secondaryLabel("重试失败的 \(flow.failed) 张") }
                        }
                    }
                    .font(.subheadline)
                    }
                }
            }
        }
        .alert("离开转换结果？", isPresented: $confirmLeave) {
            Button("留下", role: .cancel) {}
            Button("离开", role: .destructive) { flow.finish() }
        } message: {
            Text("未分享或保存的转换结果会在离开后清除。")
        }
    }
}

extension MSResultsView {
    var filterLabel: Text {
        switch filter {
        case .all: Text("筛选：全部")
        case .success: Text("筛选：成功")
        case .failed: Text("筛选：失败")
        }
    }

    @ViewBuilder var filterOptions: some View {
        Text("全部").tag(Filter.all)
        Text("成功").tag(Filter.success)
        Text("失败").tag(Filter.failed)
    }

    /// Secondary text action whose whole half of the bar is tappable (at least 44 pt high).
    func secondaryLabel(_ title: LocalizedStringKey) -> some View {
        Text(title)
            .frame(maxWidth: .infinity, minHeight: 44)
            .contentShape(Rectangle())
    }
}

struct MSItemRow: View {
    @Environment(MSFlow.self) private var flow
    @Environment(\.dynamicTypeSize) private var typeSize
    let item: MSItem
    let showShare: Bool
    @ScaledMetric(relativeTo: .title2) private var thumbSize: CGFloat = 32
    @ScaledMetric(relativeTo: .title2) private var statusSize: CGFloat = 20

    private var texts: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(item.name).lineLimit(1).truncationMode(.middle)
            Text(statusText).font(.subheadline).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
        }
    }

    var body: some View {
        Group {
            if typeSize.isAccessibilitySize {
                VStack(alignment: .leading, spacing: 6) {
                    HStack(alignment: .top) {
                        thumb(min(thumbSize, 64))
                        Spacer()
                        if !isShareable {
                            trailing.font(.system(size: min(statusSize, 40)))
                                .frame(minWidth: min(statusSize, 40), minHeight: min(statusSize, 40))
                        }
                    }
                    texts.frame(maxWidth: .infinity, alignment: .leading)
                    if isShareable { trailing }
                }
                .padding(.vertical, 6)
            } else {
                HStack(spacing: 12) {
                    thumb(44)
                    texts
                    Spacer(minLength: 8)
                    trailing
                }
            }
        }
        .frame(minHeight: 72)
        .accessibilityElement(children: .combine)
        .task(id: item.id) { await flow.loadThumbnail(item.id) }
    }

    @ViewBuilder private func thumb(_ size: CGFloat) -> some View {
        if let image = flow.thumbnail(for: item.id) {
            Image(uiImage: image).resizable().scaledToFill()
                .frame(width: size, height: size)
                .clipShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
                .accessibilityHidden(true)
        } else {
            MSPlaceholderThumb(size: size)
        }
    }

    private var isShareable: Bool { showShare && item.status == .done }

    private var statusText: String {
        switch item.status {
        case .waiting: String(localized: "等待")
        case .converting: String(localized: "转换中…")
        case .done: showShare ? String(localized: "完成 · 动态照片") : String(localized: "完成")
        case .failed(let reason): reason
        case .skipped: String(localized: "未转换")
        }
    }

    @ViewBuilder private var trailing: some View {
        switch item.status {
        case .waiting: Image(systemName: "clock").foregroundStyle(.secondary).accessibilityHidden(true)
        case .converting:
            // ProgressView ignores font size, so scale it to match the status icon at accessibility sizes.
            ProgressView().scaleEffect(typeSize.isAccessibilitySize ? min(statusSize, 40) / 20 : 1)
        case .failed: Image(systemName: "exclamationmark.circle.fill").foregroundStyle(.red).accessibilityHidden(true)
        case .skipped: Image(systemName: "minus.circle").foregroundStyle(.secondary).accessibilityHidden(true)
        case .done:
            if showShare {
                Button { flow.share(item) } label: {
                    Image(systemName: "square.and.arrow.up").frame(width: 44, height: 44).contentShape(Rectangle())
                }
                .buttonStyle(.borderless)
                .accessibilityLabel("分享 \(item.name)")
            } else {
                Image(systemName: "checkmark.circle.fill").foregroundStyle(.green).accessibilityHidden(true)
            }
        }
    }
}

// MARK: - Previews

#if DEBUG
@MainActor private func previewFlow(_ route: MSRoute?) -> MSFlow {
    MSFlow.preview(.liveToAndroid, converting: route == .converting)
}

#Preview("首页 浅色") { MSRootView(engine: UnavailableEngine()) }
#Preview("首页 深色") { MSRootView(engine: UnavailableEngine()).preferredColorScheme(.dark) }
#Preview("首页 最大字号") { MSRootView(engine: UnavailableEngine()).dynamicTypeSize(.accessibility5) }
#Preview("选择") { NavigationStack { MSSelectionView() }.environment(previewFlow(nil)).tint(.msLink) }
#Preview("选择 深色") { NavigationStack { MSSelectionView() }.environment(previewFlow(nil)).tint(.msLink).preferredColorScheme(.dark) }
#Preview("选择 最大字号") { NavigationStack { MSSelectionView() }.environment(previewFlow(nil)).tint(.msLink).dynamicTypeSize(.accessibility5) }
#Preview("结果") { NavigationStack { MSResultsView() }.environment(previewFlow(.results)).tint(.msLink) }
#Preview("结果 深色") { NavigationStack { MSResultsView() }.environment(previewFlow(.results)).tint(.msLink).preferredColorScheme(.dark) }
#Preview("结果 最大字号") { NavigationStack { MSResultsView() }.environment(previewFlow(.results)).tint(.msLink).dynamicTypeSize(.accessibility5) }
#Preview("扫描") { NavigationStack { MSScanView() }.environment(MSFlow(engine: UnavailableEngine())).tint(.msLink) }
#Preview("扫描 深色") { NavigationStack { MSScanView() }.environment(MSFlow(engine: UnavailableEngine())).tint(.msLink).preferredColorScheme(.dark) }
#Preview("扫描 最大字号") { NavigationStack { MSScanView() }.environment(MSFlow(engine: UnavailableEngine())).tint(.msLink).dynamicTypeSize(.accessibility5) }
#Preview("品牌") { NavigationStack { MSBrandView() }.environment(MSFlow(engine: UnavailableEngine())).tint(.msLink) }
#Preview("品牌 深色") { NavigationStack { MSBrandView() }.environment(MSFlow(engine: UnavailableEngine())).tint(.msLink).preferredColorScheme(.dark) }
#Preview("品牌 最大字号") { NavigationStack { MSBrandView() }.environment(MSFlow(engine: UnavailableEngine())).tint(.msLink).dynamicTypeSize(.accessibility5) }
#endif
