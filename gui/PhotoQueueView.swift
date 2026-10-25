import SwiftUI
import ImageIO

// File selection lives in the model so grid/list and window resizing keep it intact.
struct PhotoQueueView: View {
    @ObservedObject var model: WorkspaceModel
    let actions: WorkspaceActions
    let short: Bool
    @Binding var scope: PhotoQueueScope
    let showsScopePicker: Bool
    @State private var prefersGrid = true

    private var visibleItems: [PhotoQueueItem] { model.items.filter { scope.includes($0) } }
    @State private var dropTargeted = false

    var body: some View {
        GeometryReader { geometry in
            let layout = QueueLayout(width: geometry.size.width)
            VStack(spacing: 0) {
                heading(layout)
                if !prefersGrid && !layout.twoLines && !visibleItems.isEmpty { columnHeadings(layout) }
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
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .recordLayout("queue-list")
                .overlay(Rectangle().stroke(dropTargeted ? Color.accentColor : .clear, lineWidth: 2))
                .onDrop(of: [.fileURL], isTargeted: $dropTargeted) { providers in
                    guard !model.isBusy, !model.sharing else { return false }
                    return actions.acceptDrop(providers, false)
                }
                if let item = model.selectedItem, scope.includes(item) { detail(item) }
            }
            .background(InterfaceStyle.content)
        }
    }

    private func heading(_ layout: QueueLayout) -> some View {
        HStack(spacing: 12) {
            if showsScopePicker {
                Picker("照片范围", selection: $scope) {
                    ForEach(PhotoQueueScope.allCases) { Text($0.title).tag($0) }
                }
                .labelsHidden().fixedSize()
            } else {
                Text(scope.title).font(.system(size: 14, weight: .semibold))
            }
            Text("\(visibleItems.count) 张").foregroundColor(.secondary).monospacedDigit()
            if !layout.twoLines && model.issueCount > 0 {
                Label("\(model.issueCount) 项需检查", systemImage: "exclamationmark.triangle")
                    .foregroundColor(.orange).font(.callout)
            }
            Spacer(minLength: 0)
            if !layout.twoLines {
                Button { prefersGrid.toggle() } label: {
                    Image(systemName: prefersGrid ? "list.bullet" : "square.grid.2x2")
                }
                .help(prefersGrid ? "切换为列表" : "切换为缩略图")
                .accessibilityLabel(prefersGrid ? "切换为列表" : "切换为缩略图")
                .accessibilityIdentifier("queue-view-toggle")
            }
            Button { model.rescan() } label: {
                if layout.twoLines { Image(systemName: "arrow.clockwise") }
                else { Label("重新扫描", systemImage: "arrow.clockwise") }
            }
            .disabled(!model.canRescan)
            .help("重新扫描当前来源").accessibilityLabel("重新扫描")
        }
        .font(.callout).padding(.horizontal, 16).padding(.vertical, 10)
    }

    private func photoCard(_ item: PhotoQueueItem) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            PhotoMotionPreview(item: item, direction: model.settings.direction.rawValue,
                timeDomain: model.settings.timeDomain.rawValue, isDemo: model.isPreview,
                height: short ? 100 : 130, select: { model.selection = item.id })
            Button { model.selection = item.id } label: {
                VStack(alignment: .leading, spacing: 6) {
                    Text(item.name).font(.callout.weight(.medium)).lineLimit(1).truncationMode(.middle)
                    HStack(spacing: 6) {
                        Text(item.hdr).font(.caption).foregroundColor(.secondary).lineLimit(1)
                        Spacer(minLength: 0)
                        PhotoStatusLabel(state: item.state)
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
                    PhotoStatusLabel(state: item.state)
                }.font(.callout)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .accessibilityElement(children: .combine)
            .help(item.relative + "\n" + item.detail)
        } else {
            HStack(spacing: 12) {
                Label {
                    Text(item.name).lineLimit(1).truncationMode(.middle)
                } icon: { Image(systemName: "livephoto").foregroundColor(.secondary) }
                .frame(maxWidth: .infinity, alignment: .leading).help(item.source.path)
                Text(item.hdr).foregroundColor(.secondary).lineLimit(1)
                    .frame(width: 118, alignment: .leading).help(item.hdr)
                if layout.showsCover {
                    Text(item.cover).foregroundColor(.secondary).lineLimit(1).monospacedDigit()
                        .frame(width: 106, alignment: .leading).help(item.cover)
                }
                PhotoStatusLabel(state: item.state).frame(width: 92, alignment: .leading)
            }
            .font(.callout)
            .accessibilityElement(children: .combine)
            .help(item.relative + "\n" + item.manufacturer + " · " + item.detail)
        }
    }

    private var emptyState: some View {
        VStack(spacing: short ? 10 : 16) {
            if !short { MomentStaysBrandIcon().frame(width: 56, height: 56) }
            Text(model.phase == .scanning ? "正在识别照片…" : model.items.isEmpty ? "照片会显示在这里" : "此范围暂无照片")
                .font(.system(size: 16, weight: .semibold))
            Text(model.phase == .scanning ? "扫描完成后显示文件与格式信息" : model.items.isEmpty ? "点击「添加照片」，或将文件拖入此处" : "选择「全部照片」查看完整队列；转换仍使用全部来源")
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
