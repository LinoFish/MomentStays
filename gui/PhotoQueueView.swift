import SwiftUI

// One native List keeps its identity and scroll position across width changes.
// Desktop columns collapse into two lines; a resize never replaces the list.
struct PhotoQueueView: View {
    @ObservedObject var model: WorkspaceModel
    let actions: WorkspaceActions
    let short: Bool
    @State private var dropTargeted = false

    var body: some View {
        GeometryReader { geometry in
            let layout = QueueLayout(width: geometry.size.width)
            VStack(spacing: 0) {
                heading(layout)
                if !layout.twoLines && !model.items.isEmpty { columnHeadings(layout) }
                ZStack {
                    List(selection: $model.selection) {
                        ForEach(model.items) { item in
                            row(item, layout: layout).tag(item.id)
                                .listRowInsets(EdgeInsets(top: layout.twoLines ? 7 : 4, leading: 12, bottom: layout.twoLines ? 7 : 4, trailing: 12))
                                .contextMenu {
                                    Button("复制文件路径") { actions.copy(item.source.path) }
                                    Button("显示原片") { actions.reveal(item.source) }
                                    Button("复制详情") { actions.copy(item.detail) }
                                }
                        }
                    }
                    .listStyle(.plain)
                    .environment(\.defaultMinListRowHeight, layout.twoLines ? 50 : 32)
                    .accessibilityIdentifier("photo-queue")
                    if model.items.isEmpty { emptyState }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .recordLayout("queue-list")
                .overlay(Rectangle().stroke(dropTargeted ? Color.accentColor : .clear, lineWidth: 2))
                .onDrop(of: [.fileURL], isTargeted: $dropTargeted) { providers in
                    guard !model.isBusy, !model.sharing else { return false }
                    return actions.acceptDrop(providers, false)
                }
                if let item = model.selectedItem { detail(item) }
            }
            .background(InterfaceStyle.content)
        }
    }

    private func heading(_ layout: QueueLayout) -> some View {
        HStack(spacing: 12) {
            Text("照片队列").font(.system(size: 14, weight: .semibold))
            Text("\(model.items.count) 张").foregroundColor(.secondary).monospacedDigit()
            if !layout.twoLines && model.issueCount > 0 {
                Label("\(model.issueCount) 项需检查", systemImage: "exclamationmark.triangle")
                    .foregroundColor(.orange).font(.callout)
            }
            Spacer(minLength: 0)
            Button { model.rescan() } label: {
                if layout.twoLines { Image(systemName: "arrow.clockwise") }
                else { Label("重新扫描", systemImage: "arrow.clockwise") }
            }
            .disabled(!model.canRescan)
            .help("重新扫描当前来源").accessibilityLabel("重新扫描")
        }
        .font(.callout).padding(.horizontal, 16).padding(.vertical, 10)
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
            if !short { Image(systemName: "photo.on.rectangle").font(.system(size: 30, weight: .light)).foregroundColor(.secondary) }
            Text(model.phase == .scanning ? "正在识别照片…" : "照片会显示在这里")
                .font(.system(size: 16, weight: .semibold))
            Text(model.phase == .scanning ? "扫描完成后显示文件与格式信息" : "使用顶部「选择照片」，或将文件拖入此处")
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
