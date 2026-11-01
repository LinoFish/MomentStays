import SwiftUI
import ImageIO

// File selection lives in the model so grid/list and window resizing keep it intact.
struct PhotoQueueView: View {
    @ObservedObject var model: WorkspaceModel
    let actions: WorkspaceActions
    let short: Bool
    @Binding var scope: PhotoQueueScope
    @State private var prefersGrid = true
    @State private var coverEditor: PhotoQueueItem?

    private var visibleItems: [PhotoQueueItem] { model.items.filter { scope.includes($0,settings:model.settings) } }
    private var previewItem: PhotoQueueItem? {
        if let selected = model.selectedItem, scope.includes(selected,settings:model.settings) { return selected }
        return visibleItems.first
    }
    @State private var dropTargeted = false

    var body: some View {
        GeometryReader { geometry in
            let layout = QueueLayout(width: geometry.size.width)
            queueContent(layout)
                .sheet(item:$coverEditor) { item in
                    coverSheet(item,width:min(480,max(320,geometry.size.width - 32)))
                }
                .onAppear(perform:configureDiagnostics)
        }
    }

    private func configureDiagnostics() {
        #if UI_DIAGNOSTICS
        if CommandLine.arguments.contains("--ui-cover-editor") { coverEditor = model.items.first }
        #endif
    }

    private func queueContent(_ layout: QueueLayout) -> some View {
        VStack(spacing: 0) {
                heading(layout)
                if !prefersGrid && !layout.twoLines && !visibleItems.isEmpty { columnHeadings(layout) }
                Group {
                    if model.previewVisible && layout.width >= 800 {
                        HSplitView {
                            browser(layout).frame(minWidth:260,idealWidth:440)
                            focusedPreview.frame(minWidth:330)
                        }
                    } else if model.previewVisible { focusedPreview }
                    else { browser(layout) }
                }
                .frame(maxWidth:.infinity,maxHeight:.infinity)
                .recordLayout("queue-list")
                .overlay(Rectangle().stroke(dropTargeted ? InterfaceStyle.accent : .clear, lineWidth: 2))
                .onDrop(of: [.fileURL], isTargeted: $dropTargeted) { providers in
                    guard !model.isBusy, !model.sharing else { return false }
                    return actions.acceptDrop(providers, false)
                }
                if let item = model.selectedItem, scope.includes(item,settings:model.settings) { detail(item) }
            }
            .background(InterfaceStyle.gallery)
            .recordLayout("queue-scope-" + scope.rawValue)
    }

    private func coverSheet(_ item: PhotoQueueItem, width: CGFloat) -> some View {
        VStack(alignment:.leading,spacing:12) {
                HStack {
                    Text("选择封面").font(.headline)
                    Spacer()
                    Button("完成") { coverEditor = nil }.keyboardShortcut(.defaultAction)
                }
                Text(item.name).lineLimit(1).truncationMode(.middle)
                PhotoMotionPreview(item:item,direction:model.settings.direction.rawValue,timeDomain:model.settings.timeDomain.rawValue,
                    isDemo:model.isPreview,height:short ? 180 : 260,select:{},selectedCover:model.settings.coverIndex(for:item.source),
                    chooseCover:{ model.selectCover($0,for:item.source) },allowsCoverSelection:!model.isBusy && !model.sharing,
                    showsCandidateStatus:true)
        }.padding(20).frame(width:width)
            .tint(InterfaceStyle.accent).accentColor(InterfaceStyle.accent)
            .recordLayout("cover-editor")
            .dismissOnEscape { coverEditor = nil }
    }

    private func browser(_ layout: QueueLayout) -> some View {
        ZStack {
            if prefersGrid && !layout.twoLines {
                ScrollView {
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 180), spacing: InterfaceStyle.Spacing.content)], spacing: InterfaceStyle.Spacing.content) {
                        ForEach(visibleItems) { item in
                            photoCard(item)
                        }
                    }
                    .padding(InterfaceStyle.Spacing.content)
                }
                .accessibilityIdentifier("photo-queue")
            } else {
                List(selection: $model.selection) {
                    ForEach(visibleItems) { item in
                        row(item, layout: layout).tag(item.id)
                            .listRowInsets(EdgeInsets(top: layout.twoLines ? 7 : 4, leading: 12, bottom: layout.twoLines ? 7 : 4, trailing: 12))
                            .contextMenu { itemActions(item) }
                    }
                }
                .listStyle(.plain)
                .environment(\.defaultMinListRowHeight, layout.twoLines ? 50 : 32)
                .accessibilityIdentifier("photo-queue")
            }
            if visibleItems.isEmpty { emptyState }
        }
    }

    private var focusedPreview: some View {
        ScrollView {
            if let item = previewItem {
                VStack(alignment:.leading,spacing:12) {
                    Text(item.name).font(.headline).lineLimit(2)
                    if model.isBusy {
                        PhotoThumbnail(source:item.source,isPreview:model.isPreview).frame(height:260)
                        Text("任务进行中，完成后可预览动态画面").foregroundColor(.secondary)
                    } else {
                        PhotoMotionPreview(item:item,direction:model.settings.direction.rawValue,
                            timeDomain:model.settings.options.timeDomain,isDemo:model.isPreview,height:300,
                            select:{ model.selection = item.id },selectedCover:model.settings.coverIndex(for:item.source),
                            chooseCover:{ model.selectCover($0,for:item.source) },allowsCoverSelection:!model.sharing && model.settings.exportMode != .auxiliaryOnly,
                            showsCandidateStatus:true).id(item.id)
                    }
                    PhotoStatusLabel(state:model.category(item))
                    coverButton(item)
                    Text(item.detail).font(.callout).textSelection(.enabled)
                }.padding(16).recordLayout("preview-photo")
            } else { emptyState }
        }.frame(maxWidth:.infinity,maxHeight:.infinity)
            .background(InterfaceStyle.content).recordLayout("preview")
    }

    private func heading(_ layout: QueueLayout) -> some View {
        VStack(alignment:.leading,spacing:6) {
            HStack(spacing:12) {
                Text("照片").font(.headline)
                if layout.width >= 1000 {
                    scopeFilters(layout)
                } else {
                    Text("\(visibleItems.count) / \(model.items.count) 张")
                        .font(.callout).foregroundColor(.secondary).monospacedDigit()
                    Spacer(minLength:0)
                }
                browserControls
            }.frame(maxWidth:.infinity)
            if layout.width < 1000 { scopeFilters(layout) }
        }
        .padding(.horizontal,InterfaceStyle.Spacing.content).padding(.vertical,8)
        .frame(maxWidth:.infinity,alignment:.leading)
        .background(InterfaceStyle.content)
    }

    private func scopeFilters(_ layout: QueueLayout) -> some View {
        LazyVGrid(columns:Array(repeating:GridItem(.flexible(),spacing:4),count:layout.width < 560 ? 3 : 5),spacing:4) {
            ForEach(PhotoQueueScope.allCases) { value in scopeButton(value) }
        }.frame(maxWidth:.infinity).recordLayout("queue-scopes")
    }

    private var browserControls: some View {
        HStack(spacing:8) {
            Button { model.previewVisible.toggle() } label: {
                Label(model.previewVisible ? "收起预览" : "预览",systemImage:"sidebar.right")
            }.keyboardShortcut("p",modifiers:[.command,.option]).accessibilityIdentifier("preview-button")
                .accessibilityValue(model.previewVisible ? "已展开" : "已收起")
                .disabled(visibleItems.isEmpty && !model.previewVisible)
            Button { prefersGrid.toggle() } label: { Image(systemName:prefersGrid ? "list.bullet" : "square.grid.2x2") }
                .help("切换网格或列表").accessibilityLabel("切换网格或列表").accessibilityIdentifier("queue-view-toggle")
                .accessibilityValue(prefersGrid ? "网格" : "列表")
            Button { model.rescan() } label: { Image(systemName:"arrow.clockwise") }
                .disabled(!model.canRescan).help("重新扫描").accessibilityLabel("重新扫描")
            Menu {
                Button("清空列表",role:.destructive,action:actions.clearSources)
                    .disabled(model.sources.isEmpty || model.isBusy || model.sharing)
            } label: { Image(systemName:"ellipsis.circle") }
                .menuStyle(.borderlessButton).fixedSize()
                .help("列表操作").accessibilityLabel("列表操作，清空列表")
                .accessibilityIdentifier("queue-more-menu").recordLayout("queue-more-menu")
        }.fixedSize()
    }

    @ViewBuilder private func scopeButton(_ value: PhotoQueueScope) -> some View {
        let count = model.items.filter { value.includes($0,settings:model.settings) }.count
        Button { scope = value } label: {
            HStack(spacing:6) {
                Text(value.title).fontWeight(scope == value ? .semibold : .regular)
                Text("\(count)").monospacedDigit().foregroundColor(.secondary)
            }
            .font(.callout).lineLimit(1).frame(maxWidth:.infinity,minHeight:short ? 22 : 28)
            .foregroundColor(.primary)
            .background(scope == value ? InterfaceStyle.selection : Color.clear)
            .cornerRadius(InterfaceStyle.controlRadius)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(scope == value ? .isSelected : [])
        .accessibilityLabel("\(value.title)，\(count) 张")
        .help("仅显示" + value.title + "，转换仍使用全部来源")
        .accessibilityIdentifier("queue-scope-" + value.rawValue)
    }

    private func photoCard(_ item: PhotoQueueItem) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            Button { model.selection = item.id } label: {
                VStack(alignment:.leading,spacing:0) {
                    PhotoThumbnail(source:item.source,isPreview:model.isPreview)
                        .frame(height:short ? 100 : 140).clipped()
                    VStack(alignment:.leading,spacing:8) {
                        Text(item.name).font(.callout.weight(.medium)).foregroundColor(.primary)
                            .lineLimit(1).truncationMode(.middle)
                        HStack(spacing:6) {
                            Text(item.hdr).font(.caption).foregroundColor(.secondary).lineLimit(1)
                            Spacer(minLength:0)
                            PhotoStatusLabel(state:model.category(item))
                        }
                    }.padding(12)
                }.frame(maxWidth:.infinity,alignment:.leading).contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityAddTraits(model.selection == item.id ? .isSelected : [])
            .accessibilityLabel(item.name + "，" + model.category(item).title + "，" + item.hdr)
            .accessibilityHint("选择照片，使用预览查看动态画面")
            if canChooseCover(item) {
                HStack {
                    coverButton(item)
                    Spacer(minLength:0)
                    if model.selection == item.id {
                        Image(systemName:"checkmark.circle.fill").foregroundColor(InterfaceStyle.accent).accessibilityHidden(true)
                    }
                }.padding(.horizontal,12).padding(.bottom,10)
            }
        }
        .background(InterfaceStyle.content).cornerRadius(InterfaceStyle.photoRadius)
        .overlay(RoundedRectangle(cornerRadius: InterfaceStyle.photoRadius).stroke(model.selection == item.id ? InterfaceStyle.accent : InterfaceStyle.separator, lineWidth: model.selection == item.id ? 2 : 0.5).allowsHitTesting(false))
        .contextMenu { itemActions(item) }
        .help(item.relative + "\n封面：" + item.cover + "\n" + item.detail)
    }

    private func canChooseCover(_ item: PhotoQueueItem) -> Bool {
        item.isMotion && model.settings.direction == .toApple && model.settings.exportMode != .auxiliaryOnly
    }

    @ViewBuilder private func coverButton(_ item: PhotoQueueItem) -> some View {
        if canChooseCover(item) {
            Button {
                model.selection = item.id
                coverEditor = item
            } label: { Label("选择封面…",systemImage:"photo.badge.checkmark") }
                .font(.caption).disabled(model.isBusy || model.sharing)
                .help("为「" + item.name + "」选择高清候选封面")
                .accessibilityLabel("为「" + item.name + "」选择封面")
        }
    }

    @ViewBuilder private func itemActions(_ item: PhotoQueueItem) -> some View {
        if item.isMotion && model.settings.direction == .toApple {
            Button("选择高清候选封面…") { model.selection = item.id; coverEditor = item }
                .disabled(model.isBusy || model.sharing || model.settings.exportMode == .auxiliaryOnly)
        }
        Button("复制文件路径") { actions.copy(item.source.path) }
        Button("显示原片") { actions.reveal(item.source) }
        Button("复制详情") { actions.copy(item.detail) }
    }

    private func columnHeadings(_ layout: QueueLayout) -> some View {
        HStack(spacing: 12) {
            Text("文件名").frame(maxWidth: .infinity, alignment: .leading)
            Text("格式 / HDR").frame(width: 118, alignment: .leading)
            if layout.showsCover { Text("封面时间").frame(width: 106, alignment: .leading) }
            Text("状态").frame(width: 92, alignment: .leading)
        }
        .font(.system(size: 11, weight: .medium)).foregroundColor(.secondary)
        .padding(.horizontal, 20).padding(.vertical, 6)
        .background(InterfaceStyle.canvas)
    }

    @ViewBuilder private func row(_ item: PhotoQueueItem, layout: QueueLayout) -> some View {
        if layout.twoLines {
            VStack(alignment: .leading, spacing: 5) {
                Text(item.name).font(.body.weight(.medium)).lineLimit(1).truncationMode(.middle)
                HStack(spacing: 8) {
                    Text(item.hdr + (layout.width >= 440 ? " · 封面 " + item.cover : "")).foregroundColor(.secondary).lineLimit(1)
                    Spacer(minLength: 0)
                    PhotoStatusLabel(state:model.category(item))
                }.font(.callout)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .accessibilityElement(children: .combine)
            .help(item.relative + "\n" + item.detail)
        } else {
            HStack(spacing: 12) {
                Label {
                    Text(item.name).lineLimit(1).truncationMode(.middle)
                } icon: { Image(systemName:item.row.confirmedStatic ? "photo" : "livephoto").foregroundColor(.secondary) }
                .frame(maxWidth: .infinity, alignment: .leading).help(item.source.path)
                Text(item.hdr).foregroundColor(.secondary).lineLimit(1)
                    .frame(width: 118, alignment: .leading).help(item.hdr)
                if layout.showsCover {
                    Text(item.cover).foregroundColor(.secondary).lineLimit(1).monospacedDigit()
                        .frame(width: 106, alignment: .leading).help(item.cover)
                }
                PhotoStatusLabel(state:model.category(item)).frame(width: 92, alignment: .leading)
            }
            .font(.callout)
            .accessibilityElement(children: .combine)
            .help(item.relative + "\n" + item.manufacturer + " · " + item.detail)
        }
    }

    private var emptyState: some View {
        VStack(spacing: short ? 8 : 16) {
            if model.isBusy {
                ProgressView().controlSize(.small)
                Text(model.phase == .scanning ? "正在识别照片…" : "正在准备照片…").font(.headline)
                Text("照片会陆续显示在这里").font(.callout).foregroundColor(.secondary)
            } else if model.items.isEmpty {
                if !short { MomentStaysBrandIcon().frame(width:64,height:64) }
                Text(model.settings.direction == .androidToAndroid ? "暂仅支持 Google → 荣耀" : model.settings.direction == .toApple ? "将动态照片转为 Live Photo" : "将 Live Photo 转为动态照片")
                    .font(.title3.weight(.semibold)).multilineTextAlignment(.center)
                Text("拖入照片或文件夹，或从图库选择")
                    .font(.callout).foregroundColor(.secondary).multilineTextAlignment(.center)
                HStack(spacing:12) {
                    Button("选择文件…",action:actions.chooseSources)
                    Button("系统照片图库…",action:actions.importPhotos)
                }.disabled(model.isBusy || model.sharing)
                if !short {
                    Label("本机处理，保留原片",systemImage:"lock.shield")
                        .font(.caption).foregroundColor(.secondary)
                }
            } else {
                Image(systemName:scope.symbol).font(.title2).foregroundColor(.secondary).accessibilityHidden(true)
                Text(scope.emptyTitle).font(.headline)
                Button("查看全部 \(model.items.count) 张照片") { scope = .all }
            }
        }
        .padding(InterfaceStyle.Spacing.content)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(InterfaceStyle.gallery)
    }

    private func detail(_ item: PhotoQueueItem) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Divider()
            HStack(spacing: 8) {
                Button { model.detailExpanded.toggle() } label: {
                    Label("文件详情", systemImage: model.detailExpanded && !short ? "chevron.down" : "chevron.right")
                }.disabled(short).help(short ? "增加窗口高度后可展开详情" : "展开或收起选中文件的详情")
                Text(item.manufacturer + " · " + item.hdr).font(.callout).foregroundColor(.secondary).lineLimit(1)
                Spacer(minLength: 0)
                coverButton(item)
                Button { actions.copy(item.detail) } label: { Image(systemName: "doc.on.doc") }
                    .help("复制详情").accessibilityLabel("复制详情")
            }.padding(.horizontal, 12)
            if model.detailExpanded && !short {
                ScrollView {
                    VStack(alignment: .leading, spacing: 6) {
                        Text(item.relative).fontWeight(.medium)
                        Text(item.detail)
                        Text("封面：" + item.cover).foregroundColor(.secondary)
                        if !item.manufacturerDetail.isEmpty { Text(item.manufacturerDetail).foregroundColor(.secondary) }
                    }.font(.callout).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
                }.frame(maxHeight: short ? 70 : 120).padding(.horizontal, 12)
            }
        }.padding(.bottom, 10).background(InterfaceStyle.canvas)
    }
}

// Decode a bounded thumbnail off the main thread; never load a full-resolution NSImage.
struct PhotoThumbnail: View {
    let source: URL
    let isPreview: Bool
    @State private var thumbnail: CGImage?

    var body: some View {
        GeometryReader { geometry in
            ZStack {
                InterfaceStyle.canvas
                if let thumbnail = thumbnail {
                    Image(decorative: thumbnail, scale: 1)
                        .resizable().scaledToFill()
                        .frame(width: geometry.size.width, height: geometry.size.height)
                        .clipped()
                } else {
                    VStack(spacing: 7) {
                        Image(systemName:"photo").font(.largeTitle).accessibilityHidden(true)
                        Text(isPreview ? "示意照片" : "照片预览").font(.caption)
                    }.foregroundColor(.secondary)
                }
            }.frame(width: geometry.size.width, height: geometry.size.height)
        }
        .accessibilityHidden(true)
        .task(id: source) {
            thumbnail = nil
            guard !isPreview else { return }
            let image = await ThumbnailStore.load(source)
            guard !Task.isCancelled else { return }
            thumbnail = image
        }
    }
}

private enum ThumbnailStore {
    static let cache: NSCache<NSURL, CGImage> = {
        let cache = NSCache<NSURL, CGImage>()
        cache.countLimit = 96
        cache.totalCostLimit = 32 * 1024 * 1024
        return cache
    }()
    static let queue = DispatchQueue(label: "MomentStays.thumbnails", qos: .utility)

    static func load(_ url: URL) async -> CGImage? {
        await withCheckedContinuation { continuation in
            queue.async {
                if let cached = cache.object(forKey: url as NSURL) {
                    continuation.resume(returning: cached)
                    return
                }
                let options: [CFString: Any] = [
                    kCGImageSourceCreateThumbnailFromImageAlways: true,
                    kCGImageSourceCreateThumbnailWithTransform: true,
                    kCGImageSourceThumbnailMaxPixelSize: 480,
                    kCGImageSourceShouldCacheImmediately: true
                ]
                guard let source = CGImageSourceCreateWithURL(url as CFURL, [kCGImageSourceShouldCache: false] as CFDictionary),
                      let image = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary) else {
                    continuation.resume(returning: nil)
                    return
                }
                cache.setObject(image, forKey: url as NSURL, cost: image.bytesPerRow * image.height)
                continuation.resume(returning: image)
            }
        }
    }
}
