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
    @State private var sourceFor: MSDirection?
    @State private var about = false

    var body: some View {
        List {
            Section {
                row(.androidToLive)
                row(.liveToAndroid)
            } header: {
                Text("实况照片与动态照片互转")
                    .font(.subheadline).foregroundStyle(.secondary).textCase(nil)
                    .padding(.bottom, 8)
            } footer: {
                Text("照片只在本机处理，不会上传。")
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
            Text("也可从照片或文件的分享菜单中打开留刻")
                .font(.footnote).foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 20).padding(.bottom, 8)
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
        Button { sourceFor = direction } label: {
            HStack(spacing: 14) {
                Image(systemName: direction.symbol)
                    .font(.system(size: 22))
                    .foregroundStyle(Color.msLink)
                    .frame(width: 28)
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 3) {
                    Text(direction.title).font(.body).foregroundStyle(.primary)
                    Text(direction.subtitle).font(.subheadline).foregroundStyle(.secondary)
                }
                Spacer(minLength: 8)
                Image(systemName: "chevron.right").font(.footnote.weight(.semibold)).foregroundStyle(.tertiary)
                    .accessibilityHidden(true)
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
                Text("已选 \(flow.selected.count) / \(ids.count) 张")
                    .font(.subheadline).foregroundStyle(.secondary).textCase(nil).monospacedDigit()
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
                                Text(flow.candidateName(id)).font(.caption2).foregroundStyle(.secondary).lineLimit(1)
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
        .padding(.vertical, 4)
    }

    private func fileRow(_ id: Int) -> some View {
        let isOn = flow.selected.contains(id)
        return Button { toggle(id) } label: {
            HStack(spacing: 12) {
                RoundedRectangle(cornerRadius: 6).fill(Color(uiColor: .tertiarySystemFill)).frame(width: 44, height: 44)
                    .overlay { Image(systemName: "doc").foregroundStyle(.secondary) }
                Text(flow.candidateName(id)).lineLimit(1).truncationMode(.middle).foregroundStyle(.primary)
                Spacer()
                selectionMark(isOn)
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
                    Text("已检查 \(checked) / \(total) 张").font(.subheadline).foregroundStyle(.secondary).monospacedDigit()
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
            }
            Section {
                if scanning {
                    Text("已找到 \(found) 张照片，扫描结束后可选择。")
                        .foregroundStyle(.secondary)
                } else {
                    Button("选择已找到的 \(found) 张") {
                        flow.source = .scan; flow.candidateCount = found; flow.selected = []
                        flow.path.append(.select(.androidToLive, .scan))
                    }
                    .frame(minHeight: 44)
                }
            } footer: {
                Label("只检查你允许访问的照片原件。", systemImage: "info.circle")
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
                            Text(brand.name).foregroundStyle(.primary)
                            Spacer()
                            if flow.brandID == brand.id { Image(systemName: "checkmark").foregroundStyle(Color.msLink) }
                        }
                        .frame(minHeight: 44).contentShape(Rectangle())
                    }
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
    @State private var confirmStop = false

    var body: some View {
        List {
            Section {
                VStack(alignment: .leading, spacing: 8) {
                    Text(flow.direction.title).font(.footnote).foregroundStyle(.secondary)
                    Text("已处理 \(flow.processed) / \(flow.items.count) 张").font(.title2.weight(.semibold)).monospacedDigit()
                    Text("成功 \(flow.succeeded) 张 · 失败 \(flow.failed) 张").font(.subheadline).foregroundStyle(.secondary)
                    ProgressView(value: Double(flow.processed), total: Double(max(flow.items.count, 1)))
                        .tint(.msAccentFill)
                        .padding(.top, 6)
                        .accessibilityLabel("转换进度")
                        .accessibilityValue("已处理 \(flow.processed) 张，共 \(flow.items.count) 张")
                }
                .listRowBackground(Color.clear)
                .listRowInsets(EdgeInsets(top: 4, leading: 4, bottom: 4, trailing: 4))
            }
            Section {
                ForEach(flow.items) { MSItemRow(item: $0, showShare: false) }
            } header: {
                Text("转换明细")
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
                VStack(spacing: 8) {
                    ZStack(alignment: .bottomTrailing) {
                        Circle().fill(Color(uiColor: .tertiarySystemFill)).frame(width: 56, height: 56)
                            .overlay { Image(systemName: "checkmark").font(.title2.weight(.semibold)).foregroundStyle(Color.msLink) }
                        if flow.failed > 0 {
                            Image(systemName: "exclamationmark.triangle.fill").symbolRenderingMode(.multicolor).font(.subheadline).offset(x: 6, y: 4)
                        }
                    }
                    .accessibilityHidden(true)
                    Text(flow.failed > 0 ? "\(flow.succeeded) 张已转换，\(flow.failed) 张失败" : "\(flow.succeeded) 张已转换")
                        .font(.title2.weight(.semibold)).multilineTextAlignment(.center)
                    Text(flow.skipped > 0 ? "\(flow.skipped) 张未转换" : (flow.direction == .liveToAndroid ? "实况照片 → 安卓 · 文件已就绪" : "安卓 → 实况照片 · 可以存入照片"))
                        .font(.subheadline).foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity)
                .padding(.top, 8)
                .listRowBackground(Color.clear)
                .accessibilityElement(children: .combine)
            }
            if flow.failed > 0 && flow.succeeded > 0 {
                Section {
                    Picker("筛选", selection: $filter) {
                        Text("全部").tag(Filter.all)
                        Text("成功").tag(Filter.success)
                        Text("失败").tag(Filter.failed)
                    }
                    .pickerStyle(.segmented)
                    .listRowBackground(Color.clear)
                    .listRowInsets(EdgeInsets())
                }
            }
            Section {
                ForEach(rows) { MSItemRow(item: $0, showShare: flow.direction == .liveToAndroid) }
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
                    HStack(spacing: 16) {
                        if flow.direction == .liveToAndroid {
                            Button("存到文件") { flow.notice = "存到文件" }.frame(minHeight: 44)
                        }
                        if flow.direction == .liveToAndroid && flow.failed > 0 { Divider().frame(height: 16) }
                        if flow.failed > 0 {
                            Button("重试失败的 \(flow.failed) 张") { filter = .all; flow.retryFailed() }.frame(minHeight: 44)
                        }
                    }
                    .font(.subheadline)
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

struct MSItemRow: View {
    @Environment(MSFlow.self) private var flow
    let item: MSItem
    let showShare: Bool

    var body: some View {
        HStack(spacing: 12) {
            MSPlaceholderThumb()
            VStack(alignment: .leading, spacing: 2) {
                Text(item.name).lineLimit(1).truncationMode(.middle)
                Text(statusText).font(.subheadline).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: 8)
            trailing
        }
        .frame(minHeight: 72)
        .accessibilityElement(children: .combine)
    }

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
