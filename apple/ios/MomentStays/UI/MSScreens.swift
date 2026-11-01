import SwiftUI

struct MSRootView: View {
    @State private var flow = MSFlow()
    var body: some View {
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
        .alert(flow.notice ?? "", isPresented: Binding(get: { flow.notice != nil }, set: { if !$0 { flow.notice = nil } })) {
            Button("好", role: .cancel) {}
        } message: { Text("界面预览版暂未接入转换功能，此操作不会执行。") }
    }
}

// MARK: - Home (design.md §6.1)

struct MSHomeView: View {
    @Environment(MSFlow.self) private var flow
    @Environment(\.dynamicTypeSize) private var typeSize
    private var shareHint: some View {
        Text("也可从照片或文件的分享菜单中打开留刻")
            .font(.footnote).foregroundStyle(.secondary)
            .multilineTextAlignment(.center)
    }
    @State private var sourceFor: MSDirection?
    @State private var about = false

    var body: some View {
        List {
            // A plain row, not a section header: list headers are drawn lighter than body secondary text.
            Section {
                Text("实况照片与动态照片互转")
                    .font(.subheadline).foregroundStyle(Color(uiColor: .secondaryLabel))
                    .listRowBackground(Color.clear)
                    .listRowInsets(EdgeInsets(top: 0, leading: 0, bottom: 0, trailing: 0))
            }
            .listSectionSpacing(.custom(12))
            Section {
                row(.androidToLive)
                row(.liveToAndroid)
            } footer: {
                Text("照片只在本机处理，不会上传。")
            }
            if typeSize.isAccessibilitySize {
                Section { shareHint.frame(maxWidth: .infinity).listRowBackground(Color.clear) }
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle("留刻")
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button { about = true } label: { Image(systemName: "info.circle") }
                    .accessibilityLabel("关于")
            }
        }
        .safeAreaInset(edge: .bottom) {
            if !typeSize.isAccessibilitySize {
                shareHint.padding(.horizontal, 20).padding(.bottom, 8)
            }
        }
        .confirmationDialog(sourceFor?.title ?? "", isPresented: Binding(get: { sourceFor != nil }, set: { if !$0 { sourceFor = nil } }), titleVisibility: .visible, presenting: sourceFor) { direction in
            Button("从照片中选择") { flow.open(direction, .photos) }
            Button("从文件中选择") { flow.open(direction, .files) }
            if direction == .androidToLive { Button("扫描相册") { flow.open(direction, .scan) } }
            Button("取消", role: .cancel) {}
        }
        .sheet(isPresented: $about) { AboutView() }
    }

    private func row(_ direction: MSDirection) -> some View {
        let icon = Image(systemName: direction.symbol)
            .font(.title2)
            .foregroundStyle(Color.msLink)
            .accessibilityHidden(true)
        // Fixed size: the chevron does not grow with Dynamic Type.
        let chevron = Image(systemName: "chevron.right").font(.system(size: 14, weight: .semibold)).foregroundStyle(.tertiary)
            .accessibilityHidden(true)
        let texts = VStack(alignment: .leading, spacing: 3) {
            Text(direction.title).font(.body).foregroundStyle(.primary)
            Text(direction.subtitle).font(.subheadline).foregroundStyle(.secondary)
        }
        return Button { sourceFor = direction } label: {
            Group {
                if typeSize.isAccessibilitySize {
                    VStack(alignment: .leading, spacing: 10) {
                        HStack { icon; Spacer(); chevron }
                        texts.frame(maxWidth: .infinity, alignment: .leading)
                    }
                    .padding(.vertical, 8)
                } else {
                    HStack(alignment: .firstTextBaseline, spacing: 14) {
                        icon.frame(minWidth: 28)
                        texts.frame(maxWidth: .infinity, alignment: .leading)
                        chevron
                    }
                    .padding(.vertical, 8)
                }
            }
            .frame(minHeight: 64)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityHint("选择照片来源")
    }
}

// MARK: - Selection (design.md §6.2)

struct MSSelectionView: View {
    @Environment(MSFlow.self) private var flow
    @Environment(\.dynamicTypeSize) private var typeSize

    private var ids: [Int] { Array(1...flow.candidateCount) }
    private var allSelected: Bool { flow.selected.count == ids.count }

    var body: some View {
        List {
            Section {
                if flow.source == .files {
                    ForEach(ids, id: \.self) { id in fileRow(id) }
                } else {
                    grid.listRowInsets(EdgeInsets()).listRowBackground(Color.clear)
                }
            } header: {
                // Explicit secondaryLabel and a size cap: list headers are drawn lighter and grow large.
                Text("已选 \(flow.selected.count) / \(ids.count) 张")
                    .font(.subheadline).foregroundStyle(Color(uiColor: .secondaryLabel)).textCase(nil).monospacedDigit()
                    .dynamicTypeSize(...DynamicTypeSize.accessibility1)
            } footer: {
                Text("灰色图形仅为布局占位，不是用户照片。")
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
            ForEach(ids, id: \.self) { id in
                let isOn = flow.selected.contains(id)
                Button { toggle(id) } label: {
                    RoundedRectangle(cornerRadius: 2)
                        .fill(Color(uiColor: .tertiarySystemFill))
                        .aspectRatio(1, contentMode: .fit)
                        .overlay {
                            VStack(spacing: 6) {
                                Image(systemName: "photo").font(.title2).foregroundStyle(.secondary)
                                Text(flow.candidateName(id)).font(.caption2).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                                    .dynamicTypeSize(...DynamicTypeSize.xxxLarge).padding(.horizontal, 6)
                            }
                        }
                        .overlay { if isOn { RoundedRectangle(cornerRadius: 2).strokeBorder(Color.msAccentFill, lineWidth: 2) } }
                        .overlay(alignment: .topTrailing) { selectionMark(isOn).padding(6) }
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityElement(children: .ignore)
                .accessibilityLabel("动态照片，第 \(id) 张，\(flow.candidateName(id))")
                .accessibilityValue(isOn ? "已选中" : "未选中")
                .accessibilityAddTraits(isOn ? [.isButton, .isSelected] : .isButton)
            }
        }
        .padding(4)
    }

    private func fileRow(_ id: Int) -> some View {
        let isOn = flow.selected.contains(id)
        let thumb = RoundedRectangle(cornerRadius: 6).fill(Color(uiColor: .tertiarySystemFill))
            .frame(width: typeSize.isAccessibilitySize ? 32 : 44, height: typeSize.isAccessibilitySize ? 32 : 44)
            .overlay { Image(systemName: "doc").foregroundStyle(.secondary).imageScale(.small) }
        let name = Text(flow.candidateName(id)).lineLimit(1).truncationMode(.middle).foregroundStyle(.primary)
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
        .accessibilityLabel(flow.candidateName(id))
        .accessibilityValue(isOn ? "已选中" : "未选中")
        .accessibilityAddTraits(isOn ? [.isButton, .isSelected] : .isButton)
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

    private func toggle(_ id: Int) {
        if flow.selected.contains(id) { flow.selected.remove(id) } else { flow.selected.insert(id) }
    }
}

// MARK: - Scan (design.md §6.3)

struct MSScanView: View {
    @Environment(MSFlow.self) private var flow
    @Environment(\.dismiss) private var dismiss
    @State private var checked = 128
    @State private var scanning = true
    @State private var stopping = false
    private let total = 500
    private let found = 6

    var body: some View {
        List {
            Section {
                VStack(alignment: .leading, spacing: 10) {
                    Text(scanning ? "正在扫描相册…" : "扫描已停止，结果不完整").font(.headline)
                        .frame(maxWidth: .infinity, alignment: .leading)
                    Text("已检查 \(checked) / \(total) 张").font(.subheadline).foregroundStyle(.secondary).monospacedDigit()
                        .lineLimit(1).minimumScaleFactor(0.7)
                    ProgressView(value: Double(checked), total: Double(total)).tint(.msAccentFill)
                        .accessibilityLabel("扫描进度")
                        .accessibilityValue("已检查 \(checked) 张，共 \(total) 张")
                    if scanning {
                        Button(stopping ? "正在停止…" : "取消扫描") { stop() }
                            .disabled(stopping)
                            .frame(minHeight: 44)
                    }
                }
                .padding(.vertical, 6)
                .frame(maxWidth: .infinity, alignment: .leading)
                .dynamicTypeSize(...DynamicTypeSize.accessibility2)
            }
            Section {
                if scanning {
                    Text("已找到 \(found) 张照片，扫描结束后可选择。")
                        .foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .dynamicTypeSize(...DynamicTypeSize.accessibility2)
                } else {
                    Button("选择已找到的 \(found) 张") {
                        flow.source = .scan; flow.candidateCount = found; flow.selected = []
                        flow.path.append(.select(.androidToLive, .scan))
                    }
                    .frame(minHeight: 44)
                }
            } footer: {
                Label("只检查你允许访问的照片原件。", systemImage: "info.circle")
                    .dynamicTypeSize(...DynamicTypeSize.accessibility1)
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle("扫描相册")
        .navigationBarTitleDisplayMode(.inline)
        .navigationBarBackButtonHidden(scanning)
        .toolbar {
            if scanning {
                ToolbarItem(placement: .topBarLeading) { Button("取消") { stop() }.disabled(stopping) }
            }
        }
        .task {
            while scanning && !stopping && checked < total {
                try? await Task.sleep(for: .milliseconds(400))
                checked = min(total, checked + 12)
            }
            if checked >= total { scanning = false }
        }
    }

    private func stop() {
        stopping = true
        Task {
            try? await Task.sleep(for: .milliseconds(600))
            stopping = false; scanning = false
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
            Button("停止转换", role: .destructive) { flow.stopping = true }
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
                    Text(flow.skipped > 0 ? "\(flow.skipped) 张未转换" : (flow.direction == .liveToAndroid ? "实况照片 → 安卓 · 文件已就绪" : "安卓 → 实况照片 · 可以存入照片"))
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
            if flow.failed > 0 && flow.succeeded > 0 {
                if typeSize.isAccessibilitySize {
                    Section {
                        Picker("筛选", selection: $filter) { filterOptions }
                            .pickerStyle(.menu)
                            .frame(minHeight: 44)
                    }
                    .listSectionSpacing(.custom(24))
                } else {
                    Section {
                        Picker("筛选", selection: $filter) { filterOptions }
                            .pickerStyle(.segmented)
                            .listRowBackground(Color.clear)
                            .listRowInsets(EdgeInsets())
                    }
                    .listSectionSpacing(.custom(24))
                }
            }
            Section {
                ForEach(rows) { MSItemRow(item: $0, showShare: flow.direction == .liveToAndroid) }
            }
            if typeSize.isAccessibilitySize && flow.succeeded > 0 {
                Section {
                    if flow.direction == .liveToAndroid {
                        Button { flow.notice = "存到文件" } label: { Text("存到文件").frame(maxWidth: .infinity, minHeight: 44, alignment: .leading).contentShape(Rectangle()) }
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
            ToolbarItem(placement: .topBarTrailing) { Button("完成") { confirmLeave = true } }
        }
        .safeAreaInset(edge: .bottom) {
            MSBottomBar {
                if flow.succeeded == 0 {
                    Button("重试失败的 \(flow.failed) 张") { filter = .all; flow.retryFailed() }
                        .buttonStyle(MSPrimaryButtonStyle())
                } else {
                    Button(flow.direction == .liveToAndroid ? "分享 \(flow.succeeded) 个文件" : "全部存入照片（\(flow.succeeded)）") {
                        flow.notice = flow.direction == .liveToAndroid ? "分享" : "存入照片"
                    }
                    .buttonStyle(MSPrimaryButtonStyle())
                    if !typeSize.isAccessibilitySize {
                    HStack(spacing: 8) {
                        if flow.direction == .liveToAndroid {
                            Button { flow.notice = "存到文件" } label: { secondaryLabel("存到文件") }
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
                    HStack(alignment: .center) {
                        MSPlaceholderThumb(size: 32)
                        Spacer()
                        if !isShareable { trailing.font(.system(size: 22)) }
                    }
                    texts.frame(maxWidth: .infinity, alignment: .leading)
                    if isShareable { trailing }
                }
                .padding(.vertical, 6)
            } else {
                HStack(spacing: 12) {
                    MSPlaceholderThumb()
                    texts
                    Spacer(minLength: 8)
                    trailing
                }
            }
        }
        .frame(minHeight: 72)
        .accessibilityElement(children: .combine)
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
        case .converting: ProgressView()
        case .failed: Image(systemName: "exclamationmark.circle.fill").foregroundStyle(.red).accessibilityHidden(true)
        case .skipped: Image(systemName: "minus.circle").foregroundStyle(.secondary).accessibilityHidden(true)
        case .done:
            if showShare {
                Button { flow.notice = "分享" } label: {
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

@MainActor private func previewFlow(_ route: MSRoute?) -> MSFlow {
    let flow = MSFlow()
    flow.direction = .liveToAndroid
    flow.items = (1...8).map { MSItem(id: $0, name: "IMG_\(2040 + $0).jpg", status: [3, 7].contains($0) ? .failed("未找到配对视频") : .done) }
    if route == .converting {
        flow.items[3].status = .converting
        for i in 4..<8 { flow.items[i].status = .waiting }
    }
    flow.selected = [1, 4]
    return flow
}

#Preview("首页 浅色") { MSRootView() }
#Preview("首页 深色") { MSRootView().preferredColorScheme(.dark) }
#Preview("首页 最大字号") { MSRootView().dynamicTypeSize(.accessibility5) }
#Preview("选择") { NavigationStack { MSSelectionView() }.environment(previewFlow(nil)).tint(.msLink) }
#Preview("选择 深色") { NavigationStack { MSSelectionView() }.environment(previewFlow(nil)).tint(.msLink).preferredColorScheme(.dark) }
#Preview("选择 最大字号") { NavigationStack { MSSelectionView() }.environment(previewFlow(nil)).tint(.msLink).dynamicTypeSize(.accessibility5) }
#Preview("结果") { NavigationStack { MSResultsView() }.environment(previewFlow(.results)).tint(.msLink) }
#Preview("结果 深色") { NavigationStack { MSResultsView() }.environment(previewFlow(.results)).tint(.msLink).preferredColorScheme(.dark) }
#Preview("结果 最大字号") { NavigationStack { MSResultsView() }.environment(previewFlow(.results)).tint(.msLink).dynamicTypeSize(.accessibility5) }
#Preview("扫描") { NavigationStack { MSScanView() }.environment(MSFlow()).tint(.msLink) }
#Preview("扫描 深色") { NavigationStack { MSScanView() }.environment(MSFlow()).tint(.msLink).preferredColorScheme(.dark) }
#Preview("扫描 最大字号") { NavigationStack { MSScanView() }.environment(MSFlow()).tint(.msLink).dynamicTypeSize(.accessibility5) }
#Preview("品牌") { NavigationStack { MSBrandView() }.environment(MSFlow()).tint(.msLink) }
#Preview("品牌 深色") { NavigationStack { MSBrandView() }.environment(MSFlow()).tint(.msLink).preferredColorScheme(.dark) }
#Preview("品牌 最大字号") { NavigationStack { MSBrandView() }.environment(MSFlow()).tint(.msLink).dynamicTypeSize(.accessibility5) }
