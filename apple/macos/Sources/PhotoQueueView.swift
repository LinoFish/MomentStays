import SwiftUI
import ImageIO
import AppKit

// File selection lives in the model so grid/list and window resizing keep it intact.
struct PhotoQueueView: View {
    @Environment(\.workspaceTaskFeedback) private var taskFeedback
    @ObservedObject var model: WorkspaceModel
    let actions: WorkspaceActions
    let short: Bool
    @Binding var scope: PhotoQueueScope
    @State private var prefersGrid = true
    @StateObject private var scrollMemory = QueueScrollMemory()
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
                    coverSheet(item,size:geometry.size)
                }
                .alert("有照片需要手动处理", isPresented:$model.manualPromptPresented) {
                    Button("处理") { model.resolveManualPrompt(process:true) }
                    Button("本次跳过") { model.resolveManualPrompt(process:false) }
                    Button("取消",role:.cancel) { model.resolveManualPrompt(process:nil) }
                } message: { Text("封面时间未确定。可先确认封面策略，或仅转换已选的可转换照片；原片与选择会保留。") }
                .onAppear(perform:configureDiagnostics)
                .onReceive(model.$queueScope.removeDuplicates()) { _ in scrollMemory.switching = true }
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
                taskFeedback
                if model.manualPreparation {
                    HStack {
                        Text("确认需要处理的照片，再继续转换；未确认的项目仅在本轮跳过。").font(.callout)
                        Spacer()
                        Button("取消",action:model.cancelManualPreparation)
                        Button("继续转换",action:model.continueManualConversion)
                    }.padding(12).background(InterfaceStyle.content)
                }
                if !prefersGrid && !layout.twoLines && !visibleItems.isEmpty { columnHeadings(layout) }
                Group {
                    if model.previewVisible && layout.width >= 800 {
                        HSplitView {
                            browser(layout).frame(minWidth:260,idealWidth:440)
                            focusedPreview.frame(minWidth:330).transition(.opacity)
                        }.transition(.opacity)
                    } else if model.previewVisible { focusedPreview }
                    else { browser(layout) }
                }
                .frame(maxWidth:.infinity,maxHeight:.infinity)
                .subtleAnimation(model.previewVisible,duration:0.20)
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

    @ViewBuilder private func coverSheet(_ item: PhotoQueueItem, size: CGSize) -> some View {
        if item.requiresManualFrame {
            ManualMainFrameEditor(item:item,confirmed:model.settings.manualCovers?[item.id],availableSize:size,
                confirm:{ model.confirmManualFrame($0,for:item); coverEditor = nil },cancel:{ coverEditor = nil })
        } else {
            let width = min(620,max(320,size.width-32))
            let height = min(560,max(360,size.height-24))
            VStack(alignment:.leading,spacing:12) {
                Text("选择封面").font(.headline)
                Text(item.name).lineLimit(2).truncationMode(.middle).help(item.name)
                ScrollView {
                    VStack(alignment:.leading,spacing:12) {
                        manualControls(item)
                        PhotoMotionPreview(item:item,direction:model.settings.direction.rawValue,timeDomain:model.settings.timeDomain.rawValue,
                            isDemo:model.isPreview,height:min(260,max(140,height*0.45)),select:{},selectedCover:model.settings.coverIndex(for:item.source),
                            chooseCover:{ model.selectCover($0,for:item.source,expectedVersion:item.row.version) },allowsCoverSelection:!model.isBusy && !model.sharing,
                            showsCandidateStatus:true)
                    }.frame(maxWidth:.infinity)
                }
                Divider()
                HStack { Spacer(); Button("完成") { coverEditor = nil }.keyboardShortcut(.defaultAction) }
            }.padding(20).frame(width:width,height:height)
                .tint(InterfaceStyle.accent).accentColor(InterfaceStyle.accent)
                .recordLayout("cover-editor").dismissOnEscape { coverEditor = nil }
        }
    }

    private func browser(_ layout: QueueLayout) -> some View {
        ZStack {
            if prefersGrid && !layout.twoLines {
                ScrollView {
                    LazyVGrid(columns:[GridItem(.adaptive(minimum:180),spacing:InterfaceStyle.Spacing.content)],spacing:InterfaceStyle.Spacing.content) {
                        ForEach(visibleItems) { item in
                            photoCard(item)
                        }
                    }
                    .transaction { $0.animation = nil }
                    .padding(InterfaceStyle.Spacing.content)
                }
                .background(QueueScrollRestore(key:scope.rawValue + "-grid",memory:scrollMemory))
                .accessibilityIdentifier("photo-queue")
            } else {
                List {
                    ForEach(visibleItems) { item in
                        Button { model.toggleSelection(item) } label: {
                            HStack {
                                if item.isConvertible {
                                    PhotoSelectionMark(selected:model.selectedIDs.contains(item.id))
                                }
                                row(item, layout: layout)
                            }.contentShape(Rectangle())
                        }.buttonStyle(.plain)
                            .accessibilityAddTraits(model.selectedIDs.contains(item.id) ? .isSelected : [])
                            .task(id:item.id + model.settings.direction.rawValue + (item.row.version?.manualSignature ?? "")) { model.requestCoverCandidates(item) }
                            .listRowInsets(EdgeInsets(top: layout.twoLines ? 7 : 4, leading: 12, bottom: layout.twoLines ? 7 : 4, trailing: 12))
                            .contextMenu { itemActions(item) }
                    }
                }
                .listStyle(.plain)
                .environment(\.defaultMinListRowHeight, layout.twoLines ? 50 : 32)
                .background(QueueScrollRestore(key:scope.rawValue + "-list",memory:scrollMemory))
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
                            chooseCover:{ model.selectCover($0,for:item.source,expectedVersion:item.row.version) },allowsCoverSelection:!model.sharing && model.settings.exportMode != .auxiliaryOnly,
                            showsCandidateStatus:true).id(item.id)
                    }
                    PhotoStatusLabel(state:model.category(item)).labelStyle(.titleOnly)
                    coverButton(item)
                    manualControls(item)
                    Button(model.selectedIDs.contains(item.id) ? "取消选择此照片" : "选择此照片") { model.toggleSelection(item) }
                        .disabled(model.isBusy || model.sharing || !item.isConvertible)
                    Text(item.detail).font(.callout).textSelection(.enabled)
                }.padding(16).recordLayout("preview-photo")
                    .task(id:item.id + model.settings.direction.rawValue + (item.row.version?.manualSignature ?? "")) { model.requestCoverCandidates(item) }
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
        LazyVGrid(columns:Array(repeating:GridItem(.flexible(),spacing:4),count:layout.width < 560 ? 3 : 6),spacing:4) {
            ForEach(PhotoQueueScope.allCases) { value in scopeButton(value) }
        }.frame(maxWidth:.infinity).recordLayout("queue-scopes")
    }

    private var browserControls: some View {
        HStack(spacing:8) {
            Text(model.hasExplicitSelection ? "已选择 \(model.selectedIDs.count) 张" : "默认全部")
                .font(.caption).foregroundColor(.secondary).monospacedDigit()
                .help(model.hasExplicitSelection ? "选择包含其他标签中的照片；已选择 0 张时不会转换照片。可在更多菜单中取消选择，恢复默认范围。" : "未指定选择：默认转换全部可转换照片")
                .accessibilityLabel(model.hasExplicitSelection ? "已选择 \(model.selectedIDs.count) 张，包含其他标签" : "默认转换全部可转换照片")
            Button { model.previewVisible.toggle() } label: {
                Label(model.previewVisible ? "收起预览" : "预览",systemImage:"sidebar.right")
            }.keyboardShortcut("p",modifiers:[.command,.option]).accessibilityIdentifier("preview-button")
                .recordLayout("preview-button")
                .accessibilityValue(model.previewVisible ? "已展开" : "已收起")
                .disabled(visibleItems.isEmpty && !model.previewVisible)
            Button { prefersGrid.toggle() } label: { Image(systemName:prefersGrid ? "list.bullet" : "square.grid.2x2") }
                .help("切换网格或列表").accessibilityLabel("切换网格或列表").accessibilityIdentifier("queue-view-toggle")
                .accessibilityValue(prefersGrid ? "网格" : "列表")
            Button { model.rescan() } label: { Image(systemName:"arrow.clockwise") }
                .disabled(!model.canRescan).help("重新扫描").accessibilityLabel("重新扫描")
            Menu {
                Button("取消选择（恢复默认范围）",action:model.clearSelection)
                    .disabled(!model.hasExplicitSelection || model.isBusy || model.sharing)
                    .help("恢复默认：转换全部可转换照片")
                Divider()
                Button("清空列表",role:.destructive,action:actions.clearSources)
                    .disabled(model.sources.isEmpty || model.isBusy || model.sharing)
            } label: { Image(systemName:"ellipsis.circle") }
                .menuStyle(.borderlessButton).fixedSize()
                .help("更多：取消选择、清空列表").accessibilityLabel("列表操作，取消选择或清空列表")
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
            .background(RoundedRectangle(cornerRadius:InterfaceStyle.controlRadius)
                .fill(scope == value ? InterfaceStyle.selection : Color.clear)
                .subtleAnimation(scope == value,duration:0.16))
            .cornerRadius(InterfaceStyle.controlRadius)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(scope == value ? .isSelected : [])
        .accessibilityLabel("\(value.title)，\(count) 张")
        .help("仅显示" + value.title + "，保留其他标签中的选择")
        .accessibilityIdentifier("queue-scope-" + value.rawValue)
        .recordLayout("scope-button-" + value.rawValue)
    }

    private func photoCard(_ item: PhotoQueueItem) -> some View {
        // Sibling buttons keep opening the editor independent of card selection.
        ZStack(alignment:.topTrailing) {
            photoCardSelection(item)
            cardBadges(item).padding(8).frame(maxWidth:.infinity,alignment:.leading)
            VStack {
                Spacer(minLength:0)
                coverButton(item,onPhoto:true).padding(8)
            }.frame(height:cardImageHeight)
        }
            .background(InterfaceStyle.content).cornerRadius(InterfaceStyle.photoRadius)
            .overlay(RoundedRectangle(cornerRadius:InterfaceStyle.photoRadius)
                .stroke(model.selectedIDs.contains(item.id) ? InterfaceStyle.accent : InterfaceStyle.separator,
                    lineWidth:model.selectedIDs.contains(item.id) ? 2 : 0.5).allowsHitTesting(false))
            .subtleAnimation(model.selectedIDs.contains(item.id),duration:0.12)
            .contextMenu { itemActions(item) }
            .help(item.relative + "\n封面：" + item.cover + "\n" + item.detail)
            .recordLayout("photo-card-" + item.id)
    }

    private var cardImageHeight: CGFloat { short ? 112 : 136 }

    private func photoCardSelection(_ item: PhotoQueueItem) -> some View {
        Button { model.toggleSelection(item) } label: {
            VStack(alignment:.leading,spacing:0) {
                PhotoThumbnail(source:item.source,isPreview:model.isPreview)
                    .frame(height:cardImageHeight).clipped()
                    .recordLayout("photo-image-" + item.id)
                HStack(spacing:6) {
                    if item.isConvertible {
                        PhotoSelectionMark(selected:model.selectedIDs.contains(item.id))
                    }
                    Text(item.name).font(.callout.weight(.medium)).foregroundColor(.primary)
                        .lineLimit(1).truncationMode(.middle)
                }
                .padding(.horizontal,10).padding(.vertical,10)
                .frame(maxWidth:.infinity,alignment:.leading)
            }.frame(maxWidth:.infinity,alignment:.leading).contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(model.selectedIDs.contains(item.id) ? .isSelected : [])
        .accessibilityLabel(item.name + "，" + model.category(item).title + "，" + item.hdr)
        .task(id:item.id + model.settings.direction.rawValue + (item.row.version?.manualSignature ?? "")) { model.requestCoverCandidates(item) }
        .accessibilityHint(item.isConvertible ? "点击累加选择，再次点击取消此项；使用预览查看动态画面" : "点击查看文件详情")
    }

    private func cardBadges(_ item: PhotoQueueItem) -> some View {
        HStack(alignment:.top,spacing:8) {
            if item.hdr.localizedCaseInsensitiveContains("HDR") {
                PhotoHDRBadge(details:item.hdr,sourceID:item.id)
            }
            Spacer(minLength:0)
            if model.category(item) != .ready {
                cardBadge(model.category(item).title).layoutPriority(1).allowsHitTesting(false)
            }
        }
    }

    private func cardBadge(_ title: String) -> some View {
        Text(title).font(.system(size:10,weight:.medium)).lineLimit(1)
            .padding(.horizontal,5).padding(.vertical,3)
            .foregroundColor(.white).background(Color.black.opacity(0.72)).cornerRadius(4)
    }

    @ViewBuilder private func manualControls(_ item: PhotoQueueItem) -> some View {
        if item.requiresManualFrame {
            VStack(alignment:.leading,spacing:8) {
                Text("旧华为原封面位置未确定。请逐帧选择动态标记；保留相机原静态照片，原始附属数据留在输出目录，相册仅接收照片与视频。")
                    .font(.callout).foregroundColor(.secondary)
                Button(model.settings.manualCovers?[item.id].map { "已确认第\($0.frame_index + 1)帧 · 重新选择…" } ?? "选择主视频帧…") {
                    model.selection = item.id; coverEditor = item
                }.disabled(model.isBusy || model.sharing)
            }
        } else if model.category(item) == .manual {
            VStack(alignment:.leading,spacing:8) {
                Text("原片封面时间未确定。可选择可用的高清候选，或人工确认使用视频中点；中点并非原片记录的封面。")
                    .font(.callout).foregroundColor(.secondary)
                Button("确认使用视频中点（保留原静态照片）") { model.confirmManualMidpoint(item) }
                    .disabled(model.isBusy || model.sharing)
            }
        } else if model.settings.manualMidpoints?[item.id] == true {
            Text("人工策略：使用视频中点，保留原静态照片；原始封面时间仍未确定。")
                .font(.callout).foregroundColor(.secondary)
        }
    }

    private func canChooseCover(_ item: PhotoQueueItem) -> Bool {
        item.isMotion && !item.requiresManualFrame && model.hasCoverCandidates(item) && model.settings.exportMode != .auxiliaryOnly
    }

    @ViewBuilder private func coverButton(_ item: PhotoQueueItem, onPhoto: Bool = false) -> some View {
        if canChooseCover(item) {
            Button {
                model.selection = item.id
                coverEditor = item
            } label: {
                Label("选择封面…",systemImage:"photo")
                    .padding(.horizontal,onPhoto ? 8 : 0).padding(.vertical,onPhoto ? 5 : 0)
                    .foregroundColor(onPhoto ? .white : .primary)
                    .background(onPhoto ? Color.black.opacity(0.72) : Color.clear)
                    .cornerRadius(6)
            }
                .buttonStyle(.plain)
                .font(.caption).disabled(model.isBusy || model.sharing)
                .help("为「" + item.name + "」选择高清候选封面")
                .accessibilityLabel("为「" + item.name + "」选择封面")
                .recordLayout((onPhoto ? "photo-cover-" : "detail-cover-") + item.id)
        }
    }

    @ViewBuilder private func itemActions(_ item: PhotoQueueItem) -> some View {
        if canChooseCover(item) {
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
                    PhotoStatusLabel(state:model.category(item)).labelStyle(.titleOnly)
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
                PhotoStatusLabel(state:model.category(item)).labelStyle(.titleOnly).frame(width: 92, alignment: .leading)
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
                    Label {
                        Text("文件详情")
                    } icon: {
                        Image(systemName:"chevron.right").rotationEffect(.degrees(model.detailExpanded && !short ? 90 : 0))
                            .subtleAnimation(model.detailExpanded,duration:0.20)
                    }
                }.disabled(short).help(short ? "增加窗口高度后可展开详情" : "展开或收起选中文件的详情")
                    .recordLayout("file-detail-button")
                Text(item.manufacturer + " · " + item.hdr).font(.callout).foregroundColor(.secondary).lineLimit(1)
                Spacer(minLength: 0)
                coverButton(item)
                Button { actions.copy(item.detail) } label: { Image(systemName: "doc.on.doc") }
                    .help("复制详情").accessibilityLabel("复制详情")
            }.padding(.horizontal, 12)
            manualControls(item).padding(.horizontal,12)
            if model.detailExpanded && !short {
                ScrollView {
                    VStack(alignment: .leading, spacing: 6) {
                        Text(item.relative).fontWeight(.medium)
                        Text(item.detail)
                        Text("封面：" + item.cover).foregroundColor(.secondary)
                        if !item.manufacturerDetail.isEmpty { Text(item.manufacturerDetail).foregroundColor(.secondary) }
                    }.font(.callout).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
                }.frame(maxHeight: short ? 70 : 120).padding(.horizontal, 12)
                    .transition(.opacity)
            }
        }.padding(.bottom, 10).background(InterfaceStyle.canvas)
            .subtleAnimation(model.detailExpanded,duration:0.20)
            .recordLayout("file-detail")
    }
}

private struct PhotoSelectionMark: View {
    let selected: Bool
    var body: some View {
        ZStack {
            Image(systemName:"circle").foregroundColor(.secondary).opacity(selected ? 0 : 1)
            Image(systemName:"checkmark.circle.fill").foregroundColor(InterfaceStyle.accent).opacity(selected ? 1 : 0)
        }
        .frame(width:14,height:14).subtleAnimation(selected,duration:0.12)
        .accessibilityHidden(true)
    }
}

// This overlay owns its hover state; expansion never changes the card or grid size.
private struct PhotoHDRBadge: View {
    let details: String
    let sourceID: String
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var hovering = false
    @FocusState private var focused: Bool
    private var expanded: Bool { hovering || focused }
    private var shortTitle: String { details.contains("私有") ? "私有 HDR" : "HDR" }
    private var usesReducedMotion: Bool {
        #if UI_DIAGNOSTICS
        return reduceMotion || CommandLine.arguments.contains("--ui-reduce-motion")
        #else
        return reduceMotion
        #endif
    }

    var body: some View {
        HStack(spacing:0) {
            if expanded { Text(details).transition(.opacity) }
            else { Text(shortTitle).transition(.opacity) }
        }
        .font(.system(size:11,weight:.medium)).lineLimit(1)
        .padding(.horizontal,7).padding(.vertical,4)
        .foregroundColor(.white).background(Color.black.opacity(0.68)).cornerRadius(5)
        .background(HDRBadgeHoverArea(identifier:"photo-hdr-hover-" + sourceID,change:{ hovering = $0 }))
        .animation(usesReducedMotion ? nil : .easeInOut(duration:0.16),value:expanded)
        .focusable().focused($focused)
        .help(details).accessibilityLabel("HDR 格式：" + details)
        .accessibilityHint("悬停或键盘聚焦可展开格式名称")
        .recordLayout("photo-hdr-" + sourceID)
        .onAppear { recordDiagnostics() }
        .onChange(of:expanded) { _ in recordDiagnostics() }
    }

    private func recordDiagnostics() {
        #if UI_DIAGNOSTICS
        HDRBadgeDiagnostics.badges[sourceID] = ["expanded":expanded,"title":expanded ? details : shortTitle,
            "animation_enabled":!usesReducedMotion]
        #endif
    }
}

private struct HDRBadgeHoverArea: NSViewRepresentable {
    let identifier: String
    let change: (Bool) -> Void
    func makeNSView(context: Context) -> TrackingView { TrackingView() }
    func updateNSView(_ view: TrackingView, context: Context) {
        view.identifier = NSUserInterfaceItemIdentifier(identifier); view.change = change
    }
    final class TrackingView: NSView {
        var change: ((Bool) -> Void)?
        private var tracking: NSTrackingArea?
        override func updateTrackingAreas() {
            super.updateTrackingAreas()
            if let tracking = tracking { removeTrackingArea(tracking) }
            let area = NSTrackingArea(rect:.zero,options:[.activeInKeyWindow,.inVisibleRect,.mouseEnteredAndExited],owner:self)
            addTrackingArea(area); tracking = area
        }
        override func mouseEntered(with event: NSEvent) { change?(true) }
        override func mouseExited(with event: NSEvent) { change?(false) }
    }
}

#if UI_DIAGNOSTICS
enum HDRBadgeDiagnostics { static var badges: [String:[String:Any]] = [:] }
#endif

// Decode a bounded thumbnail off the main thread; never load a full-resolution NSImage.
struct PhotoThumbnail: View {
    let source: URL
    let isPreview: Bool
    var fitsImage = false
    @State private var thumbnail: CGImage?

    var body: some View {
        GeometryReader { geometry in
            ZStack {
                InterfaceStyle.canvas
                if let thumbnail = thumbnail {
                    Image(decorative: thumbnail, scale: 1)
                        .resizable().aspectRatio(contentMode:fitsImage ? .fit : .fill)
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

// Keep native clip-view coordinates per filter and presentation. Tracking the
// clip view preserves partially visible rows too (selection is not a scroll
// anchor). Delayed restores are generation-bound, so rapid tab switches cannot
// scroll a later tab or write its position into the previous one.
private final class QueueScrollMemory: ObservableObject {
    var positions: [String: NSPoint] = [:]
    var switching = false
}
private struct QueueScrollRestore: NSViewRepresentable {
    let key: String
    let memory: QueueScrollMemory
    func makeCoordinator() -> Coordinator { Coordinator(memory: memory) }
    func makeNSView(context: Context) -> NSView { NSView() }
    func updateNSView(_ view: NSView, context: Context) {
        context.coordinator.update(view, key: key)
    }
    static func dismantleNSView(_ view: NSView, coordinator: Coordinator) { coordinator.detach() }
    final class Coordinator {
        let memory: QueueScrollMemory
        weak var scroll: NSScrollView?
        var observer: NSObjectProtocol?
        var key: String?
        var generation = 0
        var restoring = false
        init(memory: QueueScrollMemory) { self.memory = memory }
        deinit { detach() }
        func detach() {
            if let observer = observer { NotificationCenter.default.removeObserver(observer) }
            observer = nil
        }
        func find(_ view: NSView) -> NSScrollView? {
            if let value = view as? NSScrollView { return value }
            for child in view.subviews { if let value = find(child) { return value } }
            return nil
        }
        func update(_ view: NSView, key next: String) {
            guard key != next || scroll == nil else { return }
            key = next; restoring = true; generation += 1
            let revision = generation
            DispatchQueue.main.async { [weak self, weak view] in
                guard let self = self, let view = view, self.generation == revision else { return }
                var parent = view.superview
                var found: NSScrollView?
                while let candidate = parent, found == nil {
                    found = self.find(candidate); parent = candidate.superview
                }
                guard let scroll = found else { self.restoring = false; self.memory.switching = false; return }
                self.detach(); self.scroll = scroll
                scroll.contentView.postsBoundsChangedNotifications = true
                self.observer = NotificationCenter.default.addObserver(forName:NSView.boundsDidChangeNotification,
                    object:scroll.contentView,queue:.main) { [weak self] _ in
                    guard let self = self, !self.restoring, !self.memory.switching, let key = self.key, let scroll = self.scroll else { return }
                    self.memory.positions[key] = scroll.contentView.bounds.origin
                }
                func restore() {
                    guard self.generation == revision else { return }
                    scroll.layoutSubtreeIfNeeded()
                    let point = self.memory.positions[next] ?? .zero
                    let bounds = scroll.documentView?.bounds ?? .zero
                    let clip = scroll.contentView
                    let clamped = NSPoint(x:max(0,min(point.x,bounds.width-clip.bounds.width)),
                        y:max(0,min(point.y,bounds.height-clip.bounds.height)))
                    clip.scroll(to:clamped); scroll.reflectScrolledClipView(clip)
                }
                restore()
                DispatchQueue.main.asyncAfter(deadline:.now()+0.05) { [weak self] in
                    guard let self = self, self.generation == revision else { return }
                    restore(); self.restoring = false; self.memory.switching = false
                }
            }
        }
    }
}
