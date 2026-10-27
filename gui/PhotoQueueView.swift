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
    @State private var dropTargeted = false

    var body: some View {
        GeometryReader { geometry in
            let layout = QueueLayout(width: geometry.size.width)
            VStack(spacing: 0) {
                heading(layout)
                if !prefersGrid && !layout.twoLines && !visibleItems.isEmpty { columnHeadings(layout) }
                Group {
                    if model.previewVisible && geometry.size.width >= 800 {
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
            .background(InterfaceStyle.content)
            .recordLayout("queue-scope-" + scope.rawValue)
        }
        .sheet(item:$coverEditor) { item in
            VStack(alignment:.leading,spacing:12) {
                HStack {
                    Text("选择封面").font(.headline)
                    Spacer()
                    Button("完成") { coverEditor = nil }.keyboardShortcut(.defaultAction)
                }
                Text(item.name).lineLimit(1).truncationMode(.middle)
                PhotoMotionPreview(item:item,direction:model.settings.direction.rawValue,timeDomain:model.settings.timeDomain.rawValue,
                    isDemo:model.isPreview,height:300,select:{},selectedCover:model.settings.coverIndex(for:item.source),
                    chooseCover:{ model.selectCover($0,for:item.source) },allowsCoverSelection:!model.isBusy && !model.sharing,
                    showsCandidateStatus:true)
            }.padding(20).frame(width:480)
        }
    }

    private func browser(_ layout: QueueLayout) -> some View {
        ZStack {
            if prefersGrid && !layout.twoLines {
                ScrollView {
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 160), spacing: 14)], spacing: 16) {
                        ForEach(visibleItems) { item in
                            photoCard(item)
                        }
                    }
                    .padding(18)
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
            if let item = model.selectedItem ?? visibleItems.first {
                VStack(alignment:.leading,spacing:12) {
                    Text(item.name).font(.headline).lineLimit(2)
                    if model.isBusy {
                        PhotoThumbnail(source:item.source,isPreview:model.isPreview).frame(height:260)
                        Text("任务进行中，完成后可预览动态画面").foregroundColor(.secondary)
                    } else {
                        PhotoMotionPreview(item:item,direction:model.settings.direction.rawValue,
                            timeDomain:model.settings.timeDomain.rawValue,isDemo:model.isPreview,height:300,
                            select:{ model.selection = item.id },selectedCover:model.settings.coverIndex(for:item.source),
                            chooseCover:{ model.selectCover($0,for:item.source) },allowsCoverSelection:!model.sharing && model.settings.exportMode != .auxiliaryOnly,
                            showsCandidateStatus:true).id(item.id)
                    }
                    PhotoStatusLabel(state:model.category(item))
                    Text(item.detail).font(.callout).textSelection(.enabled)
                }.padding(16)
            } else { Text("选择照片后可查看预览").foregroundColor(.secondary).padding(24) }
        }.frame(maxWidth:.infinity,maxHeight:.infinity).recordLayout("preview")
    }

    private func heading(_ layout: QueueLayout) -> some View {
        VStack(alignment:.leading,spacing:short ? 6 : 10) {
            HStack(spacing:8) {
                Text("照片").font(.headline)
                Text("\(model.items.count) 张").foregroundColor(.secondary).monospacedDigit()
                Spacer(minLength:0)
                Button { model.previewVisible.toggle() } label: {
                    Label(model.previewVisible ? "收起预览" : "预览",systemImage:"sidebar.right")
                }.keyboardShortcut("p",modifiers:[.command,.option]).accessibilityIdentifier("preview-button")
                Button { prefersGrid.toggle() } label: { Image(systemName:prefersGrid ? "list.bullet" : "square.grid.2x2") }
                    .help("切换网格或列表").accessibilityLabel("切换网格或列表").accessibilityIdentifier("queue-view-toggle")
                Button { model.rescan() } label: { Image(systemName:"arrow.clockwise") }
                    .disabled(!model.canRescan).help("重新扫描").accessibilityLabel("重新扫描")
            }
            LazyVGrid(columns:Array(repeating:GridItem(.flexible(),spacing:6),count:layout.width < 560 ? 3 : 5),spacing:6) {
                ForEach(PhotoQueueScope.allCases) { value in scopeButton(value) }
            }.recordLayout("queue-scopes")
        }.padding(.horizontal,12).padding(.vertical,short ? 8 : 10)
    }

    @ViewBuilder private func scopeButton(_ value: PhotoQueueScope) -> some View {
        let button = Button { scope = value } label: {
            Text("\(value.title) \(model.items.filter { value.includes($0,settings:model.settings) }.count)")
                .font(.callout).lineLimit(1).frame(maxWidth:.infinity)
        }
        .accessibilityAddTraits(scope == value ? .isSelected : [])
        .accessibilityIdentifier("queue-scope-" + value.rawValue)
        if scope == value { button.buttonStyle(.borderedProminent).foregroundColor(InterfaceStyle.accentForeground) }
        else { button }
    }

    private func photoCard(_ item: PhotoQueueItem) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            PhotoThumbnail(source:item.source,isPreview:model.isPreview)
                .frame(height:short ? 100 : 130).clipped().contentShape(Rectangle())
                .onTapGesture { model.selection = item.id }

            Button { model.selection = item.id } label: {
                VStack(alignment: .leading, spacing: 6) {
                    Text(item.name).font(.callout.weight(.medium)).lineLimit(1).truncationMode(.middle)
                    HStack(spacing: 6) {
                        Text(item.hdr).font(.caption).foregroundColor(.secondary).lineLimit(1)
                        Spacer(minLength: 0)
                        PhotoStatusLabel(state:model.category(item))
                    }
                }.padding(10).frame(maxWidth: .infinity, alignment: .leading).contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityAddTraits(model.selection == item.id ? .isSelected : [])
            .accessibilityLabel(item.name + "，" + item.state.title + "，" + item.hdr)
        }
        .background(InterfaceStyle.content).cornerRadius(9)
        .overlay(RoundedRectangle(cornerRadius: 9).stroke(model.selection == item.id ? InterfaceStyle.accent : InterfaceStyle.separator.opacity(0.5), lineWidth: model.selection == item.id ? 2 : 1))
        .contextMenu { itemActions(item) }
        .help(item.relative + "\n封面：" + item.cover + "\n" + item.detail)
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
        VStack(spacing: short ? 10 : 16) {
            if !short { MomentStaysBrandIcon().frame(width: 56, height: 56) }
            Text(model.phase == .scanning ? "正在识别照片…" : scope.emptyTitle)
                .font(.system(size: 16, weight: .semibold))
            Text(model.phase == .scanning ? "正在分批显示已识别照片" : model.items.isEmpty ? "点击「添加照片」，或将文件拖入此处" : "选择「全部」查看完整队列；转换仍使用全部来源")
                .font(.callout).foregroundColor(.secondary)
                .multilineTextAlignment(.center)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(InterfaceStyle.content)
        .padding(12)
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
                        MomentStaysBrandIcon().frame(width: 48, height: 48)
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
