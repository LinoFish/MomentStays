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
    @State private var narrowInspectorExpanded = false
    @State private var queueWidth: CGFloat = 0
    @FocusState private var searchFocused: Bool

    private var visibleItems: [PhotoQueueItem] { model.visibleMigrationItems(in:scope) }
    private var previewItem: PhotoQueueItem? {
        if let selected = model.selectedItem { return selected }
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
                .sheet(isPresented:$model.burstReviewPresented) { BurstReviewSheet(model:model) }
                .alert("有照片需要手动处理", isPresented:$model.manualPromptPresented) {
                    Button("处理") { model.resolveManualPrompt(process:true) }
                    Button("本次跳过") { model.resolveManualPrompt(process:false) }
                    Button("取消",role:.cancel) { model.resolveManualPrompt(process:nil) }
                } message: { Text("封面时间未确定。可先确认封面策略，或仅转换已选的可转换照片；原片与选择会保留。") }
                .onAppear { queueWidth = geometry.size.width; configureDiagnostics() }
                .onChange(of:geometry.size.width) { queueWidth = $0 }
                .onChange(of:model.migrationFilterKey) { _ in scrollMemory.switching = true }
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
                if !model.burstGroups.isEmpty {
                    HStack {
                        Label("连拍 \(model.burstGroups.count) 组 · \(model.burstGroups.reduce(0) { $0 + $1.members.count }) 帧",systemImage:"square.stack")
                        Spacer()
                        Button("检查连拍组与代表帧…") { model.burstReviewPresented = true }
                            .accessibilityIdentifier("burst-review-button")
                            .disabled(model.isBusy || model.sharing)
                    }.font(.callout).padding(.horizontal,16).padding(.vertical,8)
                }
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
                            browser(layout).frame(minWidth:260,idealWidth:440,maxWidth:.infinity)
                            focusedPreview.frame(minWidth:280,idealWidth:330,maxWidth:380).transition(.opacity)
                        }.transition(.opacity)
                    } else if narrowInspectorExpanded && layout.width < 800 {
                        VStack(spacing:0) {
                            HStack { Button("返回照片列表") { narrowInspectorExpanded = false }; Spacer(); Text("照片检查器").font(.headline) }.padding(10)
                            Divider()
                            focusedPreview
                        }
                    }
                    else { browser(layout) }
                }
                .frame(maxWidth:.infinity,maxHeight:.infinity)
                .subtleAnimation(model.previewVisible,duration:0.20)
                .recordLayout("queue-list")
                .overlay(Rectangle().stroke(dropTargeted ? InterfaceStyle.accent : .clear, lineWidth: 2))
                .onDrop(of: [.fileURL], isTargeted: $dropTargeted) { providers in
                    guard model.migrationTargetConfirmed, !model.isBusy, !model.sharing else { return false }
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
            if prefersGrid && !layout.twoLines { gridBrowser }
            else { listBrowser(layout) }
            if visibleItems.isEmpty { emptyState }
        }
    }
    private var gridBrowser: some View {
                ScrollView {
                    LazyVGrid(columns:[GridItem(.adaptive(minimum:180),spacing:InterfaceStyle.Spacing.content)],spacing:InterfaceStyle.Spacing.content) {
                        ForEach(visibleItems) { item in
                            photoCard(item)
                        }
                    }
                    .transaction { $0.animation = nil }
                    .padding(InterfaceStyle.Spacing.content)
                }
                .background(QueueScrollRestore(key:scope.rawValue + "-grid-" + model.migrationFilterKey,memory:scrollMemory))
                .accessibilityIdentifier("photo-queue")
    }
    private func listBrowser(_ layout: QueueLayout) -> some View {
                List {
                    ForEach(visibleItems) { item in listPhoto(item,layout:layout) }
                }
                .listStyle(.plain)
                .environment(\.defaultMinListRowHeight, layout.twoLines ? 50 : 32)
                .background(QueueScrollRestore(key:scope.rawValue + "-list-" + model.migrationFilterKey,memory:scrollMemory))
                .accessibilityIdentifier("photo-queue")
    }

    private func coverRequestID(_ item: PhotoQueueItem) -> String {
        item.id + model.settings.direction.rawValue + (item.row.version?.manualSignature ?? "")
    }
    private func listPhoto(_ item: PhotoQueueItem, layout: QueueLayout) -> some View {
        let traits: AccessibilityTraits = model.selectedIDs.contains(item.id) ? .isSelected : []
        return HStack(spacing:8) {
            Button { model.toggleSelection(item) } label: {
                HStack {
                    PhotoSelectionMark(selected:model.selectedIDs.contains(item.id))
                    row(item,layout:layout)
                }.contentShape(Rectangle())
            }.buttonStyle(.plain).disabled(model.isBusy || model.sharing)
            Button { showPreview(item) } label: { Image(systemName:"sidebar.right") }
                .buttonStyle(.borderless).help("预览与处理计划")
                .accessibilityLabel("预览「" + item.name + "」与处理计划")
        }
        .accessibilityAddTraits(traits)
        .task(id:coverRequestID(item)) { model.requestCoverCandidates(item) }
        .listRowInsets(EdgeInsets(top:layout.twoLines ? 7 : 4,leading:12,bottom:layout.twoLines ? 7 : 4,trailing:12))
        .contextMenu { itemActions(item) }
    }
    private var focusedPreview: some View {
        ScrollView {
            if let item = previewItem { inspector(item) }
            else {
                VStack(alignment:.leading,spacing:12) {
                    Text("照片检查器").font(.headline)
                    Text("添加来源后，使用照片的“预览”查看特性和处理计划。").foregroundColor(.secondary)
                }.padding(16).frame(maxWidth:.infinity,alignment:.leading)
            }
        }.frame(maxWidth:.infinity,maxHeight:.infinity)
            .background(InterfaceStyle.content).recordLayout("preview")
    }
    private func inspector(_ item: PhotoQueueItem) -> some View {
        VStack(alignment:.leading,spacing:12) {
            inspectorMedia(item)
            Divider()
            inspectorPlan(item)
            Divider()
            inspectorSource(item)
        }.frame(maxWidth:.infinity,alignment:.leading).padding(16).recordLayout("preview-photo")
            .task(id:coverRequestID(item)) { model.requestCoverCandidates(item) }
    }
    private func inspectorMedia(_ item: PhotoQueueItem) -> some View {
        Group {
            Text("照片检查器").font(.headline)
            Text(item.name).font(.callout.weight(.semibold)).lineLimit(3).textSelection(.enabled)
            if !model.isBusy && item.isMotion && !item.requiresManualFrame {
                PhotoMotionPreview(item:item,direction:model.settings.direction.rawValue,
                    timeDomain:model.settings.options.timeDomain,isDemo:model.isPreview,height:220,
                    select:{ model.selection = item.id },selectedCover:model.settings.coverIndex(for:item.source),
                    chooseCover:{ model.selectCover($0,for:item.source,expectedVersion:item.row.version) },
                    allowsCoverSelection:!model.sharing && model.settings.exportMode != .auxiliaryOnly,
                    showsCandidateStatus:true).id(item.id)
            } else { PhotoThumbnail(source:item.source,isPreview:model.isPreview).frame(height:200) }
            PhotoStatusLabel(state:model.displayState(item)).labelStyle(.titleOnly)
        }
    }
    private func inspectorPlan(_ item: PhotoQueueItem) -> some View {
        Group {
            Text("特性与本次计划").font(.callout.weight(.semibold))
            Text(model.migrationPlanSummary(item)).font(.callout).foregroundColor(.secondary).fixedSize(horizontal:false,vertical:true)
            if let original = item.originalDelivery { Text(original).font(.caption).textSelection(.enabled) }
            if !item.isMotion && item.row.burst == nil {
                Text("静态图片：不生成视频、动态封面或实况配对。").font(.caption).foregroundColor(.secondary)
            }
            if let frame = item.row.burst {
                Text("连拍原序号：\(frame.index)；相机原始总帧数未知。").font(.caption).foregroundColor(.secondary)
                Button("检查组成员与代表帧…") { model.burstReviewPresented = true }.disabled(model.isBusy || model.sharing)
            }
            coverButton(item)
            manualControls(item)
            Button(model.selectedIDs.contains(item.id) ? "取消选择此照片" : "选择此照片") { model.toggleSelection(item) }
                .disabled(model.isBusy || model.sharing)
        }
    }
    private func inspectorSource(_ item: PhotoQueueItem) -> some View {
        Group {
            Text("来源与资源").font(.callout.weight(.semibold))
            Text(item.relative).font(.caption).textSelection(.enabled)
            Text(item.detail).font(.caption).foregroundColor(.secondary).textSelection(.enabled)
            Text("查看预览不改变全局勾选。转换与原样导出生成独立副本，原片保持不变。")
                .font(.caption).foregroundColor(.secondary)
        }
    }

    private func heading(_ layout: QueueLayout) -> some View {
        VStack(alignment:.leading,spacing:10) {
            scopeFilters(layout)
            HStack(spacing:10) {
                ForEach(MigrationPhotoFeature.allCases) { feature in
                    Toggle(feature.title,isOn:Binding(get:{ model.migrationFeatureFilters.contains(feature) },set:{ value in
                        if value { model.migrationFeatureFilters.insert(feature) } else { model.migrationFeatureFilters.remove(feature) }
                    })).toggleStyle(.checkbox).font(.callout).fixedSize()
                        .padding(.horizontal,6).padding(.vertical,4)
                        .background(InterfaceStyle.canvas).cornerRadius(InterfaceStyle.controlRadius)
                        .accessibilityIdentifier("feature-" + feature.rawValue)
                }
                Spacer(minLength:0)
                if model.migrationSearchExpanded && layout.width >= 700 { searchField }
                Button {
                    model.migrationSearchExpanded.toggle()
                    if model.migrationSearchExpanded { searchFocused = true } else { model.migrationSearch = "" }
                } label: { Image(systemName:model.migrationSearchExpanded ? "xmark.circle" : "magnifyingglass") }
                    .accessibilityLabel(model.migrationSearchExpanded ? "收起并清除搜索" : "展开搜索")
                    .help("搜索文件名、来源或拍摄设备").keyboardShortcut("f",modifiers:.command)
                    .accessibilityIdentifier("queue-search-toggle")
            }
            if model.migrationSearchExpanded && layout.width < 700 { HStack { Spacer(); searchField } }
            HStack {
                Text("就绪 \(model.migrationReadyItems.count) · 未转换 \(model.migrationUnconvertedItems.count)").font(.caption).foregroundColor(.secondary)
                Button("查看计划…") { model.migrationReviewPresented = true }.font(.caption).disabled(!model.canReviewMigration)
                Spacer(minLength:0)
            }
            HStack {
                Text(model.hasExplicitSelection ? "已选 \(model.selectedIDs.count) 张 · 隐藏选中 \(model.hiddenMigrationSelectionCount) 张" : "范围：全部待处理")
                    .font(.caption).foregroundColor(.secondary).monospacedDigit()
                Spacer(minLength:0)
                browserControls(layout)
            }
        }.padding(.horizontal,InterfaceStyle.Spacing.content).padding(.vertical,10)
            .frame(maxWidth:.infinity,alignment:.leading).background(InterfaceStyle.content)
    }
    private var searchField: some View {
        TextField("搜索文件名或来源",text:$model.migrationSearch).textFieldStyle(.roundedBorder)
            .frame(minWidth:120,maxWidth:210).focused($searchFocused).accessibilityIdentifier("queue-search")
    }
    private func scopeFilters(_ layout: QueueLayout) -> some View {
        HStack(spacing:4) {
            ForEach([PhotoQueueScope.ready,.completed,.manual,.attention,.all]) { value in scopeButton(value) }
        }.frame(maxWidth:.infinity).recordLayout("queue-scopes")
    }
    private func browserControls(_ layout: QueueLayout) -> some View {
        HStack(spacing:8) {
            Button {
                if layout.width >= 800 { model.previewVisible.toggle() } else { narrowInspectorExpanded.toggle() }
            } label: { Image(systemName:"sidebar.right") }
                .keyboardShortcut("p",modifiers:[.command,.option]).accessibilityIdentifier("preview-button")
                .recordLayout("preview-button").accessibilityLabel("开合照片检查器")
                .accessibilityValue((layout.width >= 800 ? model.previewVisible : narrowInspectorExpanded) ? "已展开" : "已收起")
            Button { prefersGrid.toggle() } label: { Image(systemName:prefersGrid ? "list.bullet" : "square.grid.2x2") }
                .help("切换网格或列表").accessibilityLabel("切换网格或列表").accessibilityIdentifier("queue-view-toggle")
            Menu {
                Button("选择当前筛选结果") { for item in visibleItems where !model.selectedIDs.contains(item.id) { model.toggleSelection(item) } }
                    .disabled(model.isBusy || model.sharing || visibleItems.isEmpty)
                Button("取消选择（恢复默认范围）",action:model.clearSelection).disabled(!model.hasExplicitSelection || model.isBusy || model.sharing)
                Divider()
                Button("重新检查来源") { model.rescan() }.disabled(!model.canRescan)
                Button("查看静态照片") { scope = .still }
                Divider()
                Button("清空列表",role:.destructive,action:actions.clearSources).disabled(model.sources.isEmpty || model.isBusy || model.sharing)
            } label: { Image(systemName:"ellipsis.circle") }
                .menuStyle(.borderlessButton).fixedSize().help("选择、来源与列表操作")
                .accessibilityLabel("列表操作").accessibilityIdentifier("queue-more-menu").recordLayout("queue-more-menu")
        }.fixedSize()
    }
    private func showPreview(_ item: PhotoQueueItem) {
        model.selection = item.id
        if queueWidth >= 800 { model.previewVisible = true } else { narrowInspectorExpanded = true }
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
        VStack(spacing:0) {
          ZStack(alignment:.topTrailing) {
            photoCardSelection(item)
            cardBadges(item).padding(8).frame(maxWidth:.infinity,alignment:.leading)
            VStack {
                Spacer(minLength:0)
                coverButton(item,onPhoto:true).padding(8)
            }.frame(height:cardImageHeight)
          }
          Divider()
          HStack {
            Text(item.id == model.selection ? "当前预览" : item.manufacturer).font(.caption).foregroundColor(.secondary).lineLimit(1)
            Spacer(minLength:0)
            Button("预览") { showPreview(item) }.font(.caption)
          }.padding(.horizontal,10).padding(.vertical,7)
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
                    PhotoSelectionMark(selected:model.selectedIDs.contains(item.id))
                    Text(item.name).font(.callout.weight(.medium)).foregroundColor(.primary)
                        .lineLimit(1).truncationMode(.middle)
                }
                .padding(.horizontal,10).padding(.vertical,10)
                .frame(maxWidth:.infinity,alignment:.leading)
            }.frame(maxWidth:.infinity,alignment:.leading).contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(model.selectedIDs.contains(item.id) ? .isSelected : [])
        .accessibilityLabel(item.name + "，" + model.displayState(item).title + "，" + item.hdr)
        .task(id:item.id + model.settings.direction.rawValue + (item.row.version?.manualSignature ?? "")) { model.requestCoverCandidates(item) }
        .accessibilityHint("点击累加选择，再次点击取消此项；使用预览查看特性与处理计划")
    }

    private func cardBadges(_ item: PhotoQueueItem) -> some View {
        HStack(alignment:.top,spacing:8) {
            if item.hdr.localizedCaseInsensitiveContains("HDR") {
                PhotoHDRBadge(details:item.hdr,sourceID:item.id)
            }
            if let frame = item.row.burst { cardBadge("连拍 · 第 \(frame.index) 帧") }
            Spacer(minLength:0)
            if model.category(item) != .ready {
                cardBadge(model.displayState(item).title).layoutPriority(1).allowsHitTesting(false)
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
        if item.row.burst != nil { Button("检查连拍组与代表帧…") { model.burstReviewPresented = true }.disabled(model.isBusy || model.sharing) }
        if canChooseCover(item) {
            Button("选择高清候选封面…") { model.selection = item.id; coverEditor = item }
                .disabled(model.isBusy || model.sharing || model.settings.exportMode == .auxiliaryOnly)
        }
        Button("预览与处理计划") { showPreview(item) }
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
                    PhotoStatusLabel(state:model.displayState(item)).labelStyle(.titleOnly)
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
                PhotoStatusLabel(state:model.displayState(item)).labelStyle(.titleOnly).frame(width: 92, alignment: .leading)
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
                Text(model.migrationTargetConfirmed ? "添加要迁移的照片" : "先选择照片要使用的目标平台")
                    .font(.title3.weight(.semibold)).multilineTextAlignment(.center)
                Text(model.migrationTargetConfirmed ? "拖入文件、文件夹，或从图库选择。实况、HDR 与连拍会按实际支持路径统一处理。" : "在顶部选择目标兼容格式，再添加来源。输出位置与目标格式分别设置。")
                    .font(.callout).foregroundColor(.secondary).multilineTextAlignment(.center)
                HStack(spacing:12) {
                    Button("选择文件或文件夹…",action:actions.chooseSources)
                    if model.settings.direction != .androidToAndroid { Button("系统照片图库…",action:actions.importPhotos) }
                }.disabled(!model.migrationTargetConfirmed || model.isBusy || model.sharing)
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


// Uses the existing queue selection and task action; no independent burst task UI.
struct BurstReviewSheet: View {
    @ObservedObject var model: WorkspaceModel
    @State private var expanded: Set<String> = []
    var body: some View {
        VStack(alignment:.leading,spacing:12) {
            Text("连拍迁移计划").font(.title2)
            Text("仅支持可信荣耀 JPEG → Apple。允许缺帧，保留现有成员的原始帧序号，不补帧、不合成视频。文件名不能证明相机原始总帧数；这里只核对当前来源中可见的成员。")
                .font(.callout).foregroundColor(.secondary)
            ScrollView {
                LazyVStack(alignment:.leading,spacing:14) {
                    ForEach(model.burstGroups) { group in groupSection(group) }
                }.padding(4)
            }
            Text("文件标记校验与图库成组分别记录。导入后会回查全部成员、组标识和代表图；所选代表帧仅影响优先导入，Photos 不保证采用它。")
                .font(.caption).foregroundColor(.secondary)
            Divider()
            HStack {
                Button("完成") { model.burstReviewPresented = false }.keyboardShortcut(.cancelAction)
                Spacer()
                Button("保存选择并返回工作台") { model.burstReviewPresented = false }
                    .accessibilityIdentifier("burst-save-button").keyboardShortcut(.defaultAction)
            }
        }.padding(20).frame(minWidth:600,idealWidth:720,maxWidth:820,minHeight:440,idealHeight:600,maxHeight:720)
    }
    private func groupSection(_ group: BurstGroup) -> some View {
        DisclosureGroup(isExpanded:Binding(get:{ expanded.contains(group.id) },set:{ if $0 { expanded.insert(group.id) } else { expanded.remove(group.id) } })) {
            VStack(alignment:.leading,spacing:10) {
                Text(URL(fileURLWithPath:group.members.first?.source ?? "").deletingLastPathComponent().path)
                    .font(.caption).textSelection(.enabled)
                HStack {
                    Button("选择全组") { model.selectBurst(group,selected:true) }.disabled(!group.supported)
                    Button("取消全组") { model.selectBurst(group,selected:false) }
                }
                Toggle("按所选独立照片导出（不写连拍标记）",isOn:Binding(get:{ model.burstChoice(group).independent },set:{ model.setBurstChoice(group,independent:$0) }))
                    .disabled(!group.supported)
                ForEach(group.members) { member in
                    HStack(spacing:10) {
                        Toggle("第 \(member.index) 帧",isOn:Binding(get:{ !model.hasExplicitSelection || model.selectedIDs.contains(member.source) },set:{ model.setBurstMember(member,selected:$0) }))
                            .fixedSize().disabled(!group.supported)
                            .accessibilityIdentifier("burst-member-\(group.id)-\(member.index)")
                        Text(URL(fileURLWithPath:member.source).lastPathComponent).lineLimit(1).truncationMode(.middle).help(member.source)
                        if member.source_cover { Text("原始 COVER").font(.caption).foregroundColor(.secondary) }
                        Spacer(minLength:0)
                        Button(model.burstChoice(group).representative == member.source ? "代表帧 ✓" : "设为代表帧") { model.setBurstChoice(group,representative:member.source) }
                            .disabled(model.burstChoice(group).independent || !group.supported)
                            .accessibilityLabel("将第 \(member.index) 帧设为代表帧")
                            .accessibilityIdentifier("burst-representative-\(group.id)-\(member.index)")
                    }
                }
            }.padding(.vertical,8)
        } label: {
            VStack(alignment:.leading,spacing:4) {
                Text("\(group.label) · \(group.members.count) 帧 · 已选 \(model.burstSelectedCount(group)) 帧").font(.headline)
                if let reason = model.burstBlockingReason(group) { Label(reason,systemImage:"exclamationmark.triangle").font(.callout).foregroundColor(.secondary) }
                else { Text(model.burstChoice(group).independent ? "独立照片 · 原片保留" : group.has_gaps == true ? "允许缺帧 · 保留现有原帧序 · 整组原子输出" : "按原始帧序 · 整组原子输出").font(.caption).foregroundColor(.secondary) }
            }
        }
    }
}
