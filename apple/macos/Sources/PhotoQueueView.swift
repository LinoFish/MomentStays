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
    var showsProcessingScopes = true
    var selectionEnabled = true
    @State private var prefersGrid = true
    @StateObject private var scrollMemory = QueueScrollMemory()
    @State private var coverEditor: PhotoQueueItem?
    @State private var narrowInspectorExpanded = false
    @State private var queueWidth: CGFloat = 0
    @FocusState private var searchFocused: Bool
    @State private var marqueeFrames: [String:QueueMarqueeFrame] = [:]
    @State private var marqueeIDs: Set<String>?
    @State private var expandedBurstGroups: Set<String> = []

    @State private var resultEntries: [String:ReportEntry] = [:]
    @State private var resultFilter = "all"
    private var isExportPreview: Bool { model.workspaceStep == .preview }
    private var isResults: Bool { model.workspaceStep == .results }
    private var reviewsPhotos: Bool { isExportPreview || isResults }
    private var editsSelection: Bool { !reviewsPhotos && selectionEnabled }
    private var baseItems: [PhotoQueueItem] {
        if isExportPreview { return model.migrationScopeItems }
        if isResults, let ids = model.resultScopeIDs { return model.items.filter { ids.contains($0.id) } }
        return model.items
    }
    private var visibleItems: [PhotoQueueItem] {
        let ids = Set(baseItems.map(\.id))
        return model.visibleMigrationItems(in:scope).filter {
            ids.contains($0.id) && (!isResults || matchesResult($0,filter:resultFilter))
        }
    }
    private var previewItem: PhotoQueueItem? {
        // Never inspect a photo the active category/filter hides.
        let items = visibleItems
        if let selected = model.selectedItem, items.contains(where:{ $0.id == selected.id }) { return selected }
        return items.first
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
                .alert(AppText.text("有照片需要手动处理"), isPresented:$model.manualPromptPresented) {
                    Button(AppText.text("处理")) { model.resolveManualPrompt(process:true) }
                    Button(AppText.text("本次跳过")) { model.resolveManualPrompt(process:false) }
                    Button(AppText.text("取消"),role:.cancel) { model.resolveManualPrompt(process:nil) }
                } message: { Text(AppText.text("封面时间未确定。可先确认封面策略，或仅转换已选的可转换照片；原片与选择会保留。")) }
                .onAppear { queueWidth = geometry.size.width; configureDiagnostics() }
                .onChange(of:geometry.size.width) { queueWidth = $0 }
                .onChange(of:model.migrationFilterKey) { _ in scrollMemory.switching = true }
                .onReceive(model.$queueScope.removeDuplicates()) { _ in scrollMemory.switching = true }
                .onReceive(model.$report) { report in
                    resultEntries = Dictionary((report?.entries ?? []).map { ($0.source,$0) },uniquingKeysWith:{ _,new in new })
                }
                .onChange(of:model.workspaceStep) { _ in marqueeIDs = nil; scrollMemory.switching = true }
        }
    }

    private func configureDiagnostics() {
        #if UI_DIAGNOSTICS
        if CommandLine.arguments.contains("--ui-cover-editor") { coverEditor = model.items.first }
        if CommandLine.arguments.contains("--ui-list") { prefersGrid = false }
        #endif
    }

    private func queueContent(_ layout: QueueLayout) -> some View {
        VStack(spacing: 0) {
                heading(layout)
                taskFeedback
                if reviewsPhotos { reviewHint }
                if model.manualPreparation {
                    HStack {
                        Text(AppText.text("确认需要处理的照片，再继续转换；未确认的项目仅在本轮跳过。")).font(.callout)
                        Spacer()
                        Button(AppText.text("取消"),action:model.cancelManualPreparation)
                        Button(AppText.text("继续转换"),action:model.continueManualConversion)
                    }.padding(12).background(InterfaceStyle.content)
                }
                Group {
                    if !model.items.isEmpty && model.previewVisible && layout.width >= 600 {
                        HStack(spacing:0) {
                            browser.frame(minWidth:240,maxWidth:.infinity)
                            Divider()
                            focusedPreview.frame(width:min(330,layout.width * 0.42))
                        }.clipped()
                    } else if !model.items.isEmpty && narrowInspectorExpanded && layout.width < 600 {
                        VStack(spacing:0) {
                            HStack { Button(AppText.text("返回照片列表")) { narrowInspectorExpanded = false }; Spacer(); Text(AppText.text("照片检查器")).font(.headline) }.padding(10)
                            Divider()
                            focusedPreview
                        }
                    }
                    else { browser }
                }
                .frame(maxWidth:.infinity,maxHeight:.infinity)
                .subtleAnimation(model.previewVisible,duration:0.20)
                .recordLayout("queue-list")
                .overlay(Rectangle().stroke(dropTargeted ? InterfaceStyle.accent : .clear, lineWidth: 2))
                .onDrop(of: [.fileURL], isTargeted: $dropTargeted) { providers in
                    guard editsSelection, model.canImportSources else { return false }
                    return actions.acceptDrop(providers, false)
                }
                if !inspectorIsShown(layout), let item = model.selectedItem, scope.includes(item,settings:model.settings) { detail(item) }
                queueStatusBar(layout)
            }
            .background(InterfaceStyle.gallery)
            .recordLayout("queue-scope-" + scope.rawValue)
    }
    private func inspectorIsShown(_ layout: QueueLayout) -> Bool {
        !model.items.isEmpty && (layout.width >= 600 ? model.previewVisible : narrowInspectorExpanded)
    }

    @ViewBuilder private func coverSheet(_ item: PhotoQueueItem, size: CGSize) -> some View {
        if item.requiresManualFrame {
            ManualMainFrameEditor(item:item,confirmed:model.settings.manualCovers?[item.id],availableSize:size,
                confirm:{ model.confirmManualFrame($0,for:item); coverEditor = nil },cancel:{ coverEditor = nil })
        } else {
            let width = min(620,max(320,size.width-32))
            let height = min(560,max(360,size.height-24))
            VStack(alignment:.leading,spacing:12) {
                Text(AppText.text("选择封面")).font(.headline)
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
                HStack { Spacer(); Button(AppText.text("完成")) { coverEditor = nil }.keyboardShortcut(.defaultAction) }
            }.padding(20).frame(width:width,height:height)
                .tint(InterfaceStyle.accent).accentColor(InterfaceStyle.accent)
                .recordLayout("cover-editor").dismissOnEscape { coverEditor = nil }
        }
    }

    private var browser: some View {
        GeometryReader { geometry in
            let browserLayout = QueueLayout(width:geometry.size.width)
            let items = visibleItems
            let marqueeLayout = makeMarqueeLayout(size:geometry.size,origin:geometry.frame(in:.global).origin,items:items)
            let configuration = marqueeConfiguration(marqueeLayout)
            ZStack {
                if prefersGrid { gridBrowser(items,marqueeLayout:marqueeLayout) }
                else { listBrowser(browserLayout,items:items,marqueeLayout:marqueeLayout) }
                if visibleItems.isEmpty { emptyState }
            }
            .frame(maxWidth:.infinity,maxHeight:.infinity)
            .coordinateSpace(name:QueueMarqueeMath.space)
            .onPreferenceChange(QueueMarqueeFramesKey.self) { value in
                DispatchQueue.main.async { if marqueeFrames != value { marqueeFrames = value } }
            }
            .overlay(QueueMarqueeArea(configuration:configuration,frames:marqueeFrames,accent:InterfaceStyle.nativeAccent,
                draft:{ marqueeIDs = $0 },commit:{ ids in
                    // Model changes can precede SwiftUI's next representable update.
                    // Recheck the frozen scope/version/selection before accepting mouse-up.
                    let fresh = marqueeConfiguration(makeMarqueeLayout(size:geometry.size,origin:geometry.frame(in:.global).origin,items:visibleItems))
                    guard fresh == configuration, fresh.enabled else { return }
                    model.setExplicitSelection(ids)
                }))
            .onDisappear { marqueeIDs = nil; marqueeFrames = [:] }
            .recordLayout("queue-marquee-viewport")
        }
    }
    private func makeMarqueeLayout(size: CGSize, origin: CGPoint, items: [PhotoQueueItem]) -> QueueMarqueeLayout {
        QueueMarqueeLayout(filter:scope.rawValue + "|" + model.migrationFilterKey + "|groups:" + expandedBurstGroups.sorted().joined(separator:",") + "|inspector:\(model.previewVisible)/\(narrowInspectorExpanded)",
            grid:prefersGrid,size:size,ids:items.map(\.id),origin:origin)
    }
    private func marqueeConfiguration(_ layout: QueueMarqueeLayout) -> QueueMarqueeConfiguration {
        QueueMarqueeConfiguration(layout:layout,
            sourceRevision:model.items.flatMap { [$0.id,$0.row.version?.manualSignature ?? ""] },sources:model.sources.map(\.path),
            selected:model.selectedIDs,explicit:model.hasExplicitSelection,
            enabled:editsSelection && !model.isBusy && !model.sharing && !model.settingsPresented && !model.migrationReviewPresented &&
                !model.burstReviewPresented && coverEditor == nil && !layout.ids.isEmpty)
    }
    private func selectedForDisplay(_ item: PhotoQueueItem) -> Bool { (marqueeIDs ?? model.selectedIDs).contains(item.id) }
    private func previewBucket(_ item: PhotoQueueItem) -> Int {
        !model.isMigrationReady(item) ? 2 : model.previewRisk(item) ? 1 : 0
    }
    private func gridBrowser(_ items: [PhotoQueueItem], marqueeLayout: QueueMarqueeLayout) -> some View {
        ScrollView {
            VStack(alignment:.leading,spacing:16) {
                if isExportPreview {
                    ForEach(0..<3,id:\.self) { bucket in
                        let subset = items.filter { previewBucket($0) == bucket }
                        if !subset.isEmpty {
                            Text(AppText.text(["预计可执行","可能失败／部分可执行","明确不会转换"][bucket] + " · \(subset.count) 张"))
                                .font(.headline)
                            thumbnailGrid(subset,layout:marqueeLayout)
                        }
                    }
                } else { thumbnailGrid(items,layout:marqueeLayout) }
            }.padding(12)
        }.background(QueueScrollRestore(key:model.workspaceStep.id.description + "-grid-" + model.migrationFilterKey,memory:scrollMemory))
            .accessibilityIdentifier("photo-queue")
    }
    private func thumbnailGrid(_ items: [PhotoQueueItem], layout: QueueMarqueeLayout) -> some View {
        LazyVGrid(columns:[GridItem(.adaptive(minimum:168),spacing:12)],spacing:12) {
            ForEach(displayEntries(items)) { entry in
                if let group = entry.group { burstEntry(group,members:entry.members,marqueeLayout:layout,isGrid:true) }
                else if let item = entry.members.first { photoCard(item).background(QueueMarqueeFrameReader(id:item.id,layout:layout)) }
            }
        }.transaction { $0.animation = nil }
    }
    private func burstThumbnail(_ item: PhotoQueueItem, group: BurstGroup, layout: QueueMarqueeLayout) -> some View {
        VStack(alignment:.leading,spacing:3) {
            Button { showPreview(item) } label: {
                PhotoThumbnail(source:item.source,isPreview:model.isPreview).frame(height:64).clipped().cornerRadius(4)
            }.buttonStyle(.plain)
            HStack(spacing:3) {
                if !reviewsPhotos, let member = group.members.first(where:{$0.source == item.id}) {
                    Button { model.setBurstMember(member,selected:!model.selectedIDs.contains(item.id) && model.hasExplicitSelection) } label: {
                        Image(systemName:!model.hasExplicitSelection || model.selectedIDs.contains(item.id) ? "checkmark.circle.fill" : "circle")
                    }.buttonStyle(.plain).disabled(model.isBusy || model.sharing)
                }
                Text(item.name).font(.caption2).lineLimit(1).truncationMode(.middle)
            }
            if item.row.captureTimeMissing == true { Text(AppText.text("缺失时间")).font(.caption2).foregroundColor(.secondary) }
        }.background(QueueMarqueeFrameReader(id:item.id,layout:layout))
    }
    @ViewBuilder private func burstFilmstrip(_ item: PhotoQueueItem) -> some View {
        if let group = model.burstGroups.first(where:{$0.members.contains(where:{$0.source == item.id})}) {
            ScrollView(.horizontal,showsIndicators:true) {
                HStack(spacing:5) {
                    ForEach(group.members.filter { member in visibleItems.contains(where:{$0.id == member.source}) },id:\.source) { member in
                        Button { model.selection = member.source } label: {
                        PhotoThumbnail(source:URL(fileURLWithPath:member.source),isPreview:model.isPreview)
                            .frame(width:56,height:44).clipped().cornerRadius(4)
                            .overlay(RoundedRectangle(cornerRadius:4).stroke(member.source == item.id ? InterfaceStyle.accent : .clear,lineWidth:2))
                        }.buttonStyle(.plain)
                            .onHover { inside in if inside { model.selection = member.source } }
                            .accessibilityLabel(AppText.text("预览 ") + URL(fileURLWithPath:member.source).lastPathComponent)
                            .recordLayout("burst-strip-" + member.source)
                    }
                }.padding(2)
            }.frame(height:54)
            Text(AppText.text("悬停照片条查看对应照片")).font(.caption).foregroundColor(.secondary)
        }
    }
    private func listBrowser(_ layout: QueueLayout, items: [PhotoQueueItem], marqueeLayout: QueueMarqueeLayout) -> some View {
        VStack(spacing:0) {
            if !layout.twoLines && !visibleItems.isEmpty { columnHeadings(layout) }
                List {
                    if isExportPreview {
                        ForEach(0..<3,id:\.self) { bucket in
                            let subset = items.filter { previewBucket($0) == bucket }
                            if !subset.isEmpty {
                                Section(header:Text(AppText.text(["预计可执行","可能失败／部分可执行","明确不会转换"][bucket]))) {
                    ForEach(displayEntries(subset)) { entry in
                        if let group = entry.group {
                            burstEntry(group,members:entry.members,marqueeLayout:marqueeLayout,isGrid:false)
                        } else if let item = entry.members.first {
                            listPhoto(item,layout:layout).background(QueueMarqueeFrameReader(id:item.id,layout:marqueeLayout))
                        }
                    }
                                }
                            }
                        }
                    } else {
                    ForEach(displayEntries(items)) { entry in
                        if let group = entry.group {
                            burstEntry(group,members:entry.members,marqueeLayout:marqueeLayout,isGrid:false)
                        } else if let item = entry.members.first {
                            listPhoto(item,layout:layout).background(QueueMarqueeFrameReader(id:item.id,layout:marqueeLayout))
                        }
                    }
                    }
                }
                .listStyle(.plain)
                .environment(\.defaultMinListRowHeight, layout.twoLines ? 50 : 32)
                .background(QueueScrollRestore(key:model.workspaceStep.id.description + "-" + scope.rawValue + "-list-" + model.migrationFilterKey,memory:scrollMemory))
                .accessibilityIdentifier("photo-queue")
        }
    }

    private struct DisplayEntry: Identifiable {
        let id: String
        let group: BurstGroup?
        let members: [PhotoQueueItem]
    }
    private func displayEntries(_ items: [PhotoQueueItem]) -> [DisplayEntry] {
        var membership: [String:BurstGroup] = [:]
        for group in model.burstGroups {
            for member in group.members { membership[member.source] = group }
        }
        let visible = Dictionary(uniqueKeysWithValues:items.map { ($0.id,$0) })
        var seen = Set<String>()
        return items.compactMap { item in
            guard let group = membership[item.id] else { return DisplayEntry(id:item.id,group:nil,members:[item]) }
            guard seen.insert(group.id).inserted else { return nil }
            let members = group.members.compactMap { visible[$0.source] }
            return DisplayEntry(id:"burst:" + group.id,group:group,members:members)
        }
    }
    @ViewBuilder private func burstEntry(_ group: BurstGroup, members: [PhotoQueueItem], marqueeLayout: QueueMarqueeLayout, isGrid: Bool) -> some View {
        if isGrid, let representative = members.first(where:{$0.id == model.burstChoice(group).representative}) ?? members.first {
            burstGridCard(group,members:members,representative:representative,layout:marqueeLayout)
        } else { burstListEntry(group,members:members,marqueeLayout:marqueeLayout,isGrid:false) }
    }
    private func burstGridCard(_ group: BurstGroup, members: [PhotoQueueItem], representative: PhotoQueueItem, layout: QueueMarqueeLayout) -> some View {
        let count = model.burstSelectedCount(group)
        return VStack(spacing:0) {
            ZStack(alignment:.topLeading) {
                Button { showPreview(representative) } label: {
                    PhotoThumbnail(source:representative.source,isPreview:model.isPreview).frame(height:cardImageHeight).clipped()
                }.buttonStyle(.plain).accessibilityLabel(AppText.text("预览连拍，共 \(group.members.count) 张"))
                cardBadges(representative).padding(6)
            }
            Divider()
            HStack(spacing:6) {
                if !reviewsPhotos {
                    Button { model.selectBurst(group,selected:count != group.members.count) } label: {
                        Image(systemName:count == group.members.count ? "checkmark.circle.fill" : count > 0 ? "minus.circle.fill" : "circle")
                            .foregroundColor(count > 0 ? InterfaceStyle.accent : .secondary)
                    }.buttonStyle(.plain).disabled(model.isBusy || model.sharing)
                        .accessibilityLabel(AppText.text("选择整组连拍，共 \(group.members.count) 张"))
                }
                Button { showPreview(representative) } label: {
                    VStack(alignment:.leading,spacing:2) {
                        Text(representative.name).font(.callout.weight(.medium)).foregroundColor(.primary).lineLimit(1).truncationMode(.middle)
                        Text(AppText.text(reviewsPhotos ? groupReviewTitle(members) : "\(group.members.count) 张 · 预览查看成员")).font(.caption).foregroundColor(.secondary).lineLimit(1)
                    }.frame(maxWidth:.infinity,alignment:.leading).contentShape(Rectangle())
                }.buttonStyle(.plain)
            }.padding(.horizontal,8).padding(.vertical,6)
        }.background(InterfaceStyle.content).cornerRadius(InterfaceStyle.photoRadius)
            .overlay(RoundedRectangle(cornerRadius:InterfaceStyle.photoRadius).stroke(InterfaceStyle.separator,lineWidth:0.5))
            .background(ZStack { ForEach(members) { item in QueueMarqueeFrameReader(id:item.id,layout:layout) } })
            .contextMenu { itemActions(representative) }
            .recordLayout("burst-card-" + group.id)
    }
    private func burstListEntry(_ group: BurstGroup, members: [PhotoQueueItem], marqueeLayout: QueueMarqueeLayout, isGrid: Bool) -> some View {
        let expanded = expandedBurstGroups.contains(group.id)
        let selectedCount = model.burstSelectedCount(group)
        let representative = members.first(where:{ $0.id == model.burstChoice(group).representative }) ?? members.first
        return VStack(alignment:.leading,spacing:8) {
            HStack(spacing:8) {
                if !reviewsPhotos {
                Button { model.selectBurst(group,selected:selectedCount != group.members.count) } label: {
                    Image(systemName:selectedCount == group.members.count ? "checkmark.square.fill" : selectedCount > 0 ? "minus.square.fill" : "square")
                        .foregroundColor(selectedCount > 0 ? InterfaceStyle.accent : .secondary)
                }.buttonStyle(.plain).disabled(model.isBusy || model.sharing)
                    .accessibilityLabel(AppText.text("选择整组连拍，共 \(group.members.count) 张"))
                    .help(AppText.text("选择或取消整个真实连拍组，包含被当前筛选隐藏的成员"))
                }
                Button {
                    if expanded { expandedBurstGroups.remove(group.id) } else { expandedBurstGroups.insert(group.id) }
                } label: {
                    HStack(spacing:6) {
                        Image(systemName:expanded ? "chevron.down" : "chevron.right")
                        VStack(alignment:.leading,spacing:2) {
                            Text(AppText.text("连拍 · \(group.members.count) 张")).font(.callout.weight(.semibold))
                            Text(AppText.text((reviewsPhotos ? "本轮 \(members.count) 张" : "已选 \(selectedCount) 张") + (members.count != group.members.count ? "，显示 \(members.count) 张" : "")))
                                .font(.caption).foregroundColor(.secondary)
                        }
                        Spacer(minLength:0)
                    }.contentShape(Rectangle())
                }.buttonStyle(.plain).accessibilityValue(AppText.text(expanded ? "已展开" : "已收起"))
                    .accessibilityLabel(AppText.text(expanded ? "收起连拍成员" : "展开查看连拍成员"))
                    .recordLayout("burst-disclosure-" + group.id)
            }.background(QueueMarqueeFrameReader(id:"control:burst-header:" + group.id,layout:marqueeLayout))
            if !expanded, isGrid, let representative = representative {
                Button { showPreview(representative) } label: {
                    PhotoThumbnail(source:representative.source,isPreview:model.isPreview)
                        .frame(height:cardImageHeight).clipped().cornerRadius(6)
                }.buttonStyle(.plain).accessibilityLabel(AppText.text("预览连拍代表照片"))
                if members.contains(where:{ $0.row.captureTimeMissing == true }) {
                    Label(AppText.text("组内有照片缺失时间"),systemImage:"clock").font(.caption).foregroundColor(.secondary)
                }
            }
            if reviewsPhotos && !expanded {
                Text(AppText.text(groupReviewTitle(members))).font(.caption).foregroundColor(.secondary)
            }
            if expanded {
                if isGrid {
                    LazyVGrid(columns:[GridItem(.adaptive(minimum:70),spacing:6)],spacing:8) {
                        ForEach(members) { item in burstThumbnail(item,group:group,layout:marqueeLayout) }
                    }
                } else {
                    ForEach(members) { item in
                        listPhoto(item,layout:QueueLayout(width:queueWidth),burstMember:group.members.first(where:{$0.source == item.id}))
                            .background(QueueMarqueeFrameReader(id:item.id,layout:marqueeLayout))
                    }
                }
            }
        }.padding(10).frame(maxWidth:.infinity,alignment:.leading)
            .background(InterfaceStyle.content).cornerRadius(InterfaceStyle.photoRadius)
            .background(Group {
                if !expanded {
                    ZStack {
                        ForEach(members) { item in QueueMarqueeFrameReader(id:item.id,layout:marqueeLayout) }
                    }
                }
            })
            .accessibilityIdentifier("photo-burst-group-" + group.id)
            .recordLayout((expanded ? "burst-expanded-" : "burst-collapsed-") + group.id)
    }

    private func coverRequestID(_ item: PhotoQueueItem) -> String {
        item.id + model.settings.direction.rawValue + (item.row.version?.manualSignature ?? "")
    }
    private func listPhoto(_ item: PhotoQueueItem, layout: QueueLayout, burstMember: BurstMember? = nil) -> some View {
        let selected = marqueeIDs?.contains(item.id) ?? (burstMember != nil && !model.hasExplicitSelection || model.selectedIDs.contains(item.id))
        let traits: AccessibilityTraits = selected ? .isSelected : []
        return HStack(spacing:8) {
            if !reviewsPhotos {
            Button {
                if let member = burstMember { model.setBurstMember(member,selected:!selected) }
                else { model.toggleSelection(item) }
            } label: { PhotoSelectionMark(selected:selected).padding(4) }
                .buttonStyle(.plain).disabled(model.isBusy || model.sharing)
                .accessibilityLabel(AppText.text(selected ? "取消勾选「" : "勾选「") + item.name + "」")
            }
            row(item,layout:layout).contentShape(Rectangle())
                .onTapGesture { click(item) }
        }
        .padding(.vertical,2).padding(.horizontal,4)
        .background(item.id == model.selection ? InterfaceStyle.selection : Color.clear).cornerRadius(6)
        .accessibilityAddTraits(traits)
        .task(id:coverRequestID(item)) { model.requestCoverCandidates(item) }
        .listRowInsets(EdgeInsets(top:layout.twoLines ? 5 : 2,leading:8,bottom:layout.twoLines ? 5 : 2,trailing:8))
        .contextMenu { itemActions(item) }
        .recordLayout("photo-row-" + item.id)
    }
    private var focusedPreview: some View {
        ScrollView {
            if let item = previewItem { inspector(item) }
            else {
                VStack(alignment:.leading,spacing:12) {
                    Text(AppText.text("照片检查器")).font(.headline)
                    Text(AppText.text("添加来源后，使用照片的“预览”查看特性和处理计划。")).foregroundColor(.secondary)
                }.padding(16).frame(maxWidth:.infinity,alignment:.leading)
            }
        }.frame(maxWidth:.infinity,maxHeight:.infinity)
            .background(InterfaceStyle.content).recordLayout("preview").recordLayout("inspector")
    }
    private func inspector(_ item: PhotoQueueItem) -> some View {
        VStack(alignment:.leading,spacing:12) {
            inspectorMedia(item)
            burstFilmstrip(item)
            Divider()
            if reviewsPhotos { reviewInfo(item) } else { inspectorPlan(item) }
            Divider()
            inspectorSource(item)
        }.frame(maxWidth:.infinity,alignment:.leading).padding(16).recordLayout("preview-photo")
            .task(id:coverRequestID(item)) { model.requestCoverCandidates(item) }
    }
    private func inspectorMedia(_ item: PhotoQueueItem) -> some View {
        Group {
            Text(AppText.text("照片检查器")).font(.headline)
            Text(item.name).font(.callout.weight(.semibold)).lineLimit(3).textSelection(.enabled)
            if !model.isBusy && item.isMotion && !item.requiresManualFrame {
                PhotoMotionPreview(item:item,direction:model.settings.direction.rawValue,
                    timeDomain:model.settings.options.timeDomain,isDemo:model.isPreview,height:220,
                    select:{ model.selection = item.id },selectedCover:model.settings.coverIndex(for:item.source),
                    chooseCover:{ model.selectCover($0,for:item.source,expectedVersion:item.row.version) },
                    allowsCoverSelection:!isResults && !model.isBusy && !model.sharing && model.settings.exportMode != .auxiliaryOnly,
                    showsCandidateStatus:true).id(item.id)
            } else { PhotoThumbnail(source:item.source,isPreview:model.isPreview).frame(height:200) }
            if reviewsPhotos { reviewLabel(item) }
            else { statusLabel(item) }
        }
    }
    private func inspectorPlan(_ item: PhotoQueueItem) -> some View {
        Group {
            Text(AppText.text("特性与本次计划")).font(.callout.weight(.semibold))
            Text(AppText.text(model.migrationPlanSummary(item))).font(.callout).foregroundColor(.secondary).fixedSize(horizontal:false,vertical:true)
            if let original = item.originalDelivery { Text(AppText.text(original)).font(.caption).textSelection(.enabled) }
            if !item.isMotion && item.row.burst == nil {
                Text(AppText.text("静态图片：不生成视频、动态封面或实况配对。")).font(.caption).foregroundColor(.secondary)
            }
            if let frame = item.row.burst {
                Text(AppText.text("连拍原序号：\(frame.index)；相机原始总帧数未知。")).font(.caption).foregroundColor(.secondary)
            }
            coverButton(item)
            manualControls(item)
            Button(AppText.text(model.selectedIDs.contains(item.id) ? "取消勾选此照片" : "勾选此照片")) { model.toggleSelection(item) }
                .disabled(model.isBusy || model.sharing)
        }
    }
    private func inspectorSource(_ item: PhotoQueueItem) -> some View {
        Group {
            Text(AppText.text("来源与资源")).font(.callout.weight(.semibold))
            Text(item.relative).font(.caption).textSelection(.enabled)
            Text(AppText.text(item.detail)).font(.caption).foregroundColor(.secondary).textSelection(.enabled)
            Text(AppText.text("查看预览不改变全局勾选。转换与原样导出生成独立副本，原片保持不变。"))
                .font(.caption).foregroundColor(.secondary)
        }
    }

    // Browsing uses source-feature tabs; processing categories are reserved for results.
    private func heading(_ layout: QueueLayout) -> some View {
        // Keep feature names visible on narrow windows by separating browser tools.
        let narrow = layout.width < 660
        return VStack(alignment:.leading,spacing:8) {
            if narrow {
                browsingFilters(layout)
                HStack(spacing:10) { Spacer(minLength:0); toolButtons(layout) }
            } else {
                HStack(spacing:10) { browsingFilters(layout); toolButtons(layout) }
            }
            if model.migrationSearchExpanded && layout.width < 860 { searchField }
        }.padding(.horizontal,12).padding(.vertical,6)
            .frame(maxWidth:.infinity,alignment:.leading).background(InterfaceStyle.content)
    }
    @ViewBuilder private func toolButtons(_ layout: QueueLayout) -> some View {
        if showsProcessingScopes || isResults { featureMenu }
        if !reviewsPhotos && !model.burstGroups.isEmpty {
            Button { model.burstReviewPresented = true } label: { Image(systemName:"square.stack.3d.down.right") }
                .buttonStyle(.borderless).accessibilityLabel(AppText.text("检查连拍组"))
                .accessibilityIdentifier("burst-review-button").recordLayout("burst-review-button")
                .help(AppText.text("检查连拍组：\(model.burstGroups.count) 组 · \(model.burstGroups.reduce(0) { $0 + $1.members.count }) 帧"))
                .disabled(model.isBusy || model.sharing)
        }
        if model.migrationSearchExpanded && layout.width >= 860 { searchField.frame(width:200) }
        else if !model.migrationSearchExpanded { searchToggle }
        browserControls(layout)
    }
    private var searchToggle: some View {
        Button {
            model.migrationSearchExpanded = true
            searchFocused = true
        } label: { Image(systemName:"magnifyingglass") }
            .buttonStyle(.borderless)
            .accessibilityLabel(AppText.text("搜索照片")).accessibilityValue(AppText.text("已收起"))
            .help(AppText.text("搜索文件名、来源或拍摄设备（⌘F）")).keyboardShortcut("f",modifiers:.command)
            .accessibilityIdentifier("queue-search-toggle")
    }
    private func featureBinding(_ feature: MigrationPhotoFeature) -> Binding<Bool> {
        Binding(get:{ model.migrationFeatureFilters.contains(feature) },set:{ value in
            if value { model.migrationFeatureFilters.insert(feature) } else { model.migrationFeatureFilters.remove(feature) }
        })
    }
    private var featureMenu: some View {
        let active = model.migrationFeatureFilters.count
        return Menu {
            ForEach(MigrationPhotoFeature.allCases) { feature in
                Toggle(AppText.text(feature.title),isOn:featureBinding(feature)).accessibilityIdentifier("feature-" + feature.rawValue)
            }
            Divider()
            Button(AppText.text("清除特性筛选")) { model.migrationFeatureFilters = [] }.disabled(active == 0)
        } label: {
            Label(AppText.text(active == 0 ? "筛选" : "筛选 \(active)"),systemImage:active == 0 ? "line.3.horizontal.decrease.circle" : "line.3.horizontal.decrease.circle.fill")
        }
            .menuStyle(.borderlessButton).fixedSize()
            .help(AppText.text("按照片特性筛选；多项勾选时只显示同时具备的照片，不改变本轮处理范围"))
            .accessibilityIdentifier("feature-filter-menu").recordLayout("feature-filter-menu")
    }
    private var searchField: some View {
        HStack(spacing:4) {
            TextField(AppText.text("搜索文件名、来源或设备"),text:$model.migrationSearch).textFieldStyle(.roundedBorder)
                .frame(minWidth:120,maxWidth:.infinity).focused($searchFocused).accessibilityIdentifier("queue-search")
                .onAppear { searchFocused = true }
                .dismissOnEscape(closeSearch)
            Button(action:closeSearch) { Image(systemName:"xmark.circle.fill").foregroundColor(.secondary) }
                .buttonStyle(.borderless).help(AppText.text("清除并收起搜索")).accessibilityLabel(AppText.text("清除并收起搜索"))
        }
    }
    private func closeSearch() {
        model.migrationSearch = ""
        model.migrationSearchExpanded = false
        searchFocused = false
    }
    private var selectionSummary: String {
        if let ids = marqueeIDs { return "框选预览：\(ids.count) 张 · 松开鼠标确认，Esc 取消" }
        if isExportPreview { return "导出预估 · \(baseItems.count) 张 · 原片缩略图" }
        if isResults { return "执行结果 · \(baseItems.count) 张 · 原片缩略图" }
        if !model.hasExplicitSelection { return "范围：全部待处理照片" }
        let hidden = model.hiddenMigrationSelectionCount
        return "已选 \(model.selectedIDs.count) 张" + (hidden > 0 ? " · 隐藏选中 \(hidden) 张" : "")
    }
    private func queueStatusBar(_ layout: QueueLayout) -> some View {
        VStack(spacing:0) {
            Divider()
            HStack(spacing:8) {
                Text(AppText.text(selectionSummary)).lineLimit(1).truncationMode(.tail).layoutPriority(1)
                    .help(AppText.text(selectionSummary + "；空白处拖动框选，Shift 增选，⌘ 切换；普通框选保留隐藏选中。筛选与预览不改变处理范围"))
                    .accessibilityIdentifier("queue-selection-summary")
                if !reviewsPhotos && model.hasExplicitSelection {
                    Button(AppText.text("恢复默认范围"),action:model.clearSelection).disabled(model.isBusy || model.sharing)
                        .help(AppText.text("清除显式勾选，恢复处理全部待处理照片；不会删除照片"))
                        .accessibilityIdentifier("queue-restore-default")
                        .recordLayout("queue-restore-default")
                }
                Spacer(minLength:0)
                if layout.width >= 700 || !model.hasExplicitSelection {
                    Text(AppText.text("显示 \(visibleItems.count) / \(baseItems.count)")).foregroundColor(.secondary).lineLimit(1).monospacedDigit()
                }
                if !reviewsPhotos {
                    Button(AppText.text("查看计划…")) { model.migrationReviewPresented = true }.disabled(!model.canReviewMigration)
                        .help(AppText.text("查看就绪项、需人工处理和不能转换的照片"))
                }
            }.font(.caption).padding(.horizontal,12).padding(.vertical,4)
        }.background(InterfaceStyle.content).recordLayout("queue-status-bar")
    }
    @ViewBuilder private func browsingFilters(_ layout: QueueLayout) -> some View {
        if isResults { resultFilters }
        else if showsProcessingScopes { scopeFilters(layout) }
        else {
            ScrollView(.horizontal,showsIndicators:false) {
                HStack(spacing:5) {
                    featureTab("全部",count:baseItems.count,selected:model.migrationFeatureFilters.isEmpty) {
                        model.migrationFeatureFilters = []
                    }
                    ForEach(MigrationPhotoFeature.allCases) { feature in
                        let count = baseItems.filter { feature.includes($0) }.count
                        if count > 0 || feature == .missingTime {
                            featureTab(feature.title,count:count,selected:model.migrationFeatureFilters == [feature]) {
                                model.migrationFeatureFilters = [feature]
                            }
                        }
                    }
                }
            }.frame(maxWidth:.infinity,alignment:.leading).recordLayout("photo-feature-tabs")
        }
    }
    private func featureTab(_ title: String, count: Int, selected: Bool, action: @escaping () -> Void) -> some View {
        Button(action:action) {
            HStack(spacing:4) {
                Text(AppText.text(title))
                Text(AppText.text("\(count)")).foregroundColor(.secondary).monospacedDigit()
            }.font(.callout).padding(.horizontal,8).padding(.vertical,5)
                .background(selected ? InterfaceStyle.selection : .clear)
                .cornerRadius(InterfaceStyle.controlRadius)
        }.buttonStyle(.plain).fixedSize()
            .accessibilityLabel(AppText.text("\(title)，\(count) 张"))
            .accessibilityAddTraits(selected ? .isSelected : [])
            .help(AppText.text("只筛选显示，不改变本轮处理范围"))
    }
    private func scopeFilters(_ layout: QueueLayout) -> some View {
        HStack(spacing:4) {
            ForEach([PhotoQueueScope.ready,.completed,.manual,.attention,.all]) { value in scopeButton(value) }
        }.frame(maxWidth:.infinity,alignment:.leading).recordLayout("queue-scopes")
    }
    private func browserControls(_ layout: QueueLayout) -> some View {
        HStack(spacing:8) {
            Button {
                if layout.width >= 600 { model.previewVisible.toggle() } else { narrowInspectorExpanded.toggle() }
            } label: { Image(systemName:"sidebar.right") }
                .buttonStyle(.borderless).disabled(model.items.isEmpty)
                .help(AppText.text("展开或收起照片检查器（⌘⌥P）"))
                .keyboardShortcut("p",modifiers:[.command,.option]).accessibilityIdentifier("preview-button")
                .recordLayout("preview-button").accessibilityLabel(AppText.text("开合照片检查器"))
                .accessibilityValue(AppText.text((layout.width >= 600 ? model.previewVisible : narrowInspectorExpanded) ? "已展开" : "已收起"))
            Button { prefersGrid.toggle() } label: { Image(systemName:prefersGrid ? "list.bullet" : "square.grid.2x2") }
                .buttonStyle(.borderless)
                .help(AppText.text(prefersGrid ? "切换为列表" : "切换为网格")).accessibilityLabel(AppText.text("切换网格或列表"))
                .accessibilityValue(AppText.text(prefersGrid ? "当前为网格" : "当前为列表")).accessibilityIdentifier("queue-view-toggle")
                .recordLayout("queue-view-toggle")
            if !reviewsPhotos {
            Menu {
                Button(AppText.text("选择当前筛选结果")) { for item in visibleItems where !model.selectedIDs.contains(item.id) { model.toggleSelection(item) } }
                    .disabled(model.isBusy || model.sharing || visibleItems.isEmpty)
                Button(AppText.text("取消全部勾选")) { for item in model.items where model.selectedIDs.contains(item.id) { model.toggleSelection(item) } }
                    .disabled(model.selectedIDs.isEmpty || model.isBusy || model.sharing)
                Button(AppText.text("恢复默认处理范围"),action:model.clearSelection).disabled(!model.hasExplicitSelection || model.isBusy || model.sharing)
                Divider()
                if showsProcessingScopes { Button(AppText.text("查看静态照片")) { scope = .still } }
                Button(AppText.text("清除筛选并查看全部")) { clearFilters() }
                    .disabled(model.items.isEmpty)
            } label: { Image(systemName:"ellipsis.circle") }
                .menuStyle(.borderlessButton).fixedSize().help(AppText.text("勾选与筛选操作"))
                .accessibilityLabel(AppText.text("列表操作")).accessibilityIdentifier("queue-more-menu").recordLayout("queue-more-menu")
            }
        }.fixedSize()
    }
    private func clearFilters() {
        resultFilter = "all"
        model.migrationFeatureFilters = []
        model.migrationSearch = ""
        model.migrationSource = nil
        scope = .all
    }
    // Clicking a photo inspects it; inclusion stays on the check mark (or ⌘-click).
    private func click(_ item: PhotoQueueItem) {
        if editsSelection && NSEvent.modifierFlags.contains(.command) { model.toggleSelection(item) } else { focus(item) }
    }
    private func focus(_ item: PhotoQueueItem) {
        model.selection = item.id
        if queueWidth >= 600 && !model.previewVisible { model.previewVisible = true }
    }
    private func showPreview(_ item: PhotoQueueItem) {
        model.selection = item.id
        if queueWidth >= 600 { model.previewVisible = true } else { narrowInspectorExpanded = true }
    }

    @ViewBuilder private func scopeButton(_ value: PhotoQueueScope) -> some View {
        let count = model.items.filter { value.includes($0,settings:model.settings) }.count
        Button { scope = value } label: {
            HStack(spacing:6) {
                Text(AppText.text(value.title)).fontWeight(scope == value ? .semibold : .regular)
                Text(AppText.text("\(count)")).monospacedDigit().foregroundColor(.secondary)
            }
            .font(.callout).lineLimit(1).fixedSize().padding(.horizontal,9).frame(minHeight:short ? 22 : 28)
            .foregroundColor(.primary)
            .background(RoundedRectangle(cornerRadius:InterfaceStyle.controlRadius)
                .fill(scope == value ? InterfaceStyle.selection : Color.clear)
                .subtleAnimation(scope == value,duration:0.16))
            .cornerRadius(InterfaceStyle.controlRadius)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(scope == value ? .isSelected : [])
        .accessibilityLabel(AppText.text("\(value.title)，\(count) 张"))
        .help(AppText.text("仅显示" + value.title + "，保留其他标签中的选择"))
        .accessibilityIdentifier("queue-scope-" + value.rawValue)
        .recordLayout("scope-button-" + value.rawValue)
    }

    private func photoCard(_ item: PhotoQueueItem) -> some View {
        // Sibling buttons keep opening the editor independent of card selection.
        VStack(spacing:0) {
          ZStack(alignment:.topTrailing) {
            PhotoThumbnail(source:item.source,isPreview:model.isPreview)
                .frame(height:cardImageHeight).clipped().contentShape(Rectangle())
                .onTapGesture { click(item) }
                .recordLayout("photo-image-" + item.id)
                .accessibilityHidden(true)
            cardBadges(item).padding(8).frame(maxWidth:.infinity,alignment:.leading)
            if !reviewsPhotos {
                VStack {
                    Spacer(minLength:0)
                    coverButton(item,onPhoto:true).padding(8)
                }.frame(height:cardImageHeight)
            }
          }
          Divider()
          HStack(spacing:6) {
            if reviewsPhotos { reviewCardFooter(item) }
            else { photoCardSelection(item) }
          }.padding(.horizontal,8).padding(.vertical,6)
        }
            .background(InterfaceStyle.content).cornerRadius(InterfaceStyle.photoRadius)
            .overlay(RoundedRectangle(cornerRadius:InterfaceStyle.photoRadius)
                .stroke(selectedForDisplay(item) ? InterfaceStyle.accent : item.id == model.selection ? Color.secondary : InterfaceStyle.separator,
                    lineWidth:selectedForDisplay(item) ? 2 : item.id == model.selection ? 1.5 : 0.5).allowsHitTesting(false))
            .subtleAnimation(selectedForDisplay(item),duration:0.12)
            .contextMenu { itemActions(item) }
            .task(id:coverRequestID(item)) { model.requestCoverCandidates(item) }
            .help(AppText.text(item.relative + "\n封面：" + item.cover + "\n" + item.detail))
            .recordLayout("photo-card-" + item.id)
    }

    private var cardImageHeight: CGFloat { short ? 100 : 120 }

    private func photoCardSelection(_ item: PhotoQueueItem) -> some View {
        Button { model.toggleSelection(item) } label: {
            HStack(spacing:6) {
                PhotoSelectionMark(selected:selectedForDisplay(item))
                VStack(alignment:.leading,spacing:2) {
                    Text(item.name).font(.callout.weight(.medium)).foregroundColor(.primary)
                        .lineLimit(1).truncationMode(.middle)
                    Text(AppText.text(item.id == model.selection ? "当前预览" : item.manufacturer)).font(.caption).foregroundColor(.secondary).lineLimit(1)
                }
            }.frame(maxWidth:.infinity,alignment:.leading).contentShape(Rectangle())
        }
        .buttonStyle(.plain).disabled(model.isBusy || model.sharing)
        .accessibilityAddTraits(selectedForDisplay(item) ? .isSelected : [])
        .accessibilityLabel(item.name + (item.isMotion ? AppText.text("，实况照片") : "") + ", " + AppText.text(model.displayState(item).title) + ", " + AppText.text(item.hdr))
        .accessibilityHint(AppText.text("点击勾选或取消此项；点击照片查看特性与处理计划"))
    }

    private func cardBadges(_ item: PhotoQueueItem) -> some View {
        VStack(alignment:.leading,spacing:4) {
            HStack(spacing:6) {
                if item.row.burst != nil {
                    Label(AppText.text("连拍"),systemImage:"square.stack.3d.up.fill")
                        .font(.system(size:11,weight:.medium)).fixedSize()
                        .padding(.horizontal,6).padding(.vertical,4)
                        .foregroundColor(.white).background(Color.black.opacity(0.68)).cornerRadius(5)
                        .allowsHitTesting(false).recordLayout("photo-burst-" + item.id)
                }
                if item.isMotion {
                    Label(AppText.text("实况"),systemImage:"livephoto")
                        .font(.system(size:11,weight:.medium)).fixedSize()
                        .padding(.horizontal,6).padding(.vertical,4)
                        .foregroundColor(.white).background(Color.black.opacity(0.68)).cornerRadius(5)
                        .allowsHitTesting(false).accessibilityLabel(AppText.text("实况照片"))
                        .recordLayout("photo-live-" + item.id)
                }
                if item.hdr.localizedCaseInsensitiveContains("HDR") {
                    PhotoHDRBadge(details:item.hdr,sourceID:item.id)
                }
                Spacer(minLength:0)
            }
            if item.row.captureTimeMissing == true {
                Label(AppText.text("缺失时间"),systemImage:"clock")
                    .font(.system(size:11,weight:.medium)).fixedSize()
                    .padding(.horizontal,6).padding(.vertical,4)
                    .foregroundColor(.white).background(Color.black.opacity(0.68)).cornerRadius(5)
                    .allowsHitTesting(false).help(AppText.text("来源照片缺失时间；PhotoTime 只补全输出副本，不修改原片"))
                    .accessibilityIdentifier("photo-missing-time-" + item.id)
                    .recordLayout("photo-missing-time-" + item.id)
            }
            if !reviewsPhotos && model.category(item) != .ready {
                HStack(spacing:6) {

                    Spacer(minLength:0)
                    if !reviewsPhotos && model.category(item) != .ready {
                        cardBadge(model.displayState(item).title).layoutPriority(1).allowsHitTesting(false)
                    }
                }
            }
        }
    }

    private func cardBadge(_ title: String) -> some View {
        Text(AppText.text(title)).font(.system(size:10,weight:.medium)).lineLimit(1)
            .padding(.horizontal,5).padding(.vertical,3)
            .foregroundColor(.white).background(Color.black.opacity(0.72)).cornerRadius(4)
    }

    @ViewBuilder private func manualControls(_ item: PhotoQueueItem) -> some View {
        if item.requiresManualFrame {
            VStack(alignment:.leading,spacing:8) {
                Text(AppText.text("旧华为原封面位置未确定。请逐帧选择动态标记；保留相机原静态照片，原始附属数据留在输出目录，相册仅接收照片与视频。"))
                    .font(.callout).foregroundColor(.secondary)
                Button(AppText.text(model.settings.manualCovers?[item.id].map { "已确认第\($0.frame_index + 1)帧 · 重新选择…" } ?? "选择主视频帧…")) {
                    model.selection = item.id; coverEditor = item
                }.disabled(model.isBusy || model.sharing)
            }
        } else if model.category(item) == .manual {
            VStack(alignment:.leading,spacing:8) {
                Text(AppText.text("原片封面时间未确定。可选择可用的高清候选，或人工确认使用视频中点；中点并非原片记录的封面。"))
                    .font(.callout).foregroundColor(.secondary)
                Button(AppText.text("确认使用视频中点（保留原静态照片）")) { model.confirmManualMidpoint(item) }
                    .disabled(model.isBusy || model.sharing)
            }
        } else if model.settings.manualMidpoints?[item.id] == true {
            Text(AppText.text("人工策略：使用视频中点，保留原静态照片；原始封面时间仍未确定。"))
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
                Label(AppText.text("选择封面…"),systemImage:"photo")
                    .padding(.horizontal,onPhoto ? 8 : 0).padding(.vertical,onPhoto ? 5 : 0)
                    .foregroundColor(onPhoto ? .white : .primary)
                    .background(onPhoto ? Color.black.opacity(0.72) : Color.clear)
                    .cornerRadius(6)
            }
                .buttonStyle(.plain)
                .font(.caption).disabled(model.isBusy || model.sharing)
                .help(AppText.text("为「") + item.name + AppText.text("」选择高清候选封面"))
                .accessibilityLabel(AppText.text("为「") + item.name + AppText.text("」选择封面"))
                .recordLayout((onPhoto ? "photo-cover-" : "detail-cover-") + item.id)
        }
    }

    @ViewBuilder private func itemActions(_ item: PhotoQueueItem) -> some View {
        if !isResults && item.row.burst != nil { Button(AppText.text("检查连拍组与代表帧…")) { model.burstReviewPresented = true }.disabled(model.isBusy || model.sharing) }
        if !isResults && canChooseCover(item) {
            Button(AppText.text("选择高清候选封面…")) { model.selection = item.id; coverEditor = item }
                .disabled(model.isBusy || model.sharing || model.settings.exportMode == .auxiliaryOnly)
        }
        Button(AppText.text("预览与处理计划")) { showPreview(item) }
        Button(AppText.text("复制文件路径")) { actions.copy(item.source.path) }
        Button(AppText.text("显示原片")) { actions.reveal(item.source) }
        Button(AppText.text("复制详情")) { actions.copy(item.detail) }
    }

    private func columnHeadings(_ layout: QueueLayout) -> some View {
        HStack(spacing: 12) {
            Text(AppText.text("文件名")).frame(maxWidth: .infinity, alignment: .leading)
            Text(AppText.text("格式 / HDR")).frame(width: 118, alignment: .leading)
            if layout.showsCover { Text(AppText.text("封面时间")).frame(width: 106, alignment: .leading) }
            Text(AppText.text("状态")).frame(width: 92, alignment: .leading)
        }
        .font(.system(size: 11, weight: .medium)).foregroundColor(.secondary)
        .padding(.leading,44).padding(.trailing,16).padding(.vertical,6)
        .background(InterfaceStyle.canvas)
    }

    @ViewBuilder private func row(_ item: PhotoQueueItem, layout: QueueLayout) -> some View {
        if layout.twoLines {
            VStack(alignment: .leading, spacing: 5) {
                Text(item.name).font(.body.weight(.medium)).lineLimit(1).truncationMode(.middle)
                HStack(spacing: 8) {
                    Text(AppText.text(item.hdr + (item.row.captureTimeMissing == true ? " · 缺失时间" : "") + (layout.width >= 440 ? " · 封面 " + item.cover : ""))).foregroundColor(.secondary).lineLimit(1)
                    Spacer(minLength: 0)
                    statusLabel(item)
                }.font(.callout)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .accessibilityElement(children: .combine)
            .help(AppText.text(item.relative + "\n" + item.detail))
        } else {
            HStack(spacing: 12) {
                Label {
                    Text(item.name).lineLimit(1).truncationMode(.middle)
                } icon: { Image(systemName:item.row.confirmedStatic ? "photo" : "livephoto").foregroundColor(.secondary) }
                .frame(maxWidth: .infinity, alignment: .leading).help(item.source.path)
                Text(AppText.text(item.hdr + (item.row.captureTimeMissing == true ? " · 缺失时间" : ""))).foregroundColor(.secondary).lineLimit(1)
                    .frame(width: 118, alignment: .leading).help(AppText.text(item.hdr))
                if layout.showsCover {
                    Text(AppText.text(item.cover)).foregroundColor(.secondary).lineLimit(1).monospacedDigit()
                        .frame(width: 106, alignment: .leading).help(AppText.text(item.cover))
                }
                statusLabel(item).frame(width: 92, alignment: .leading)
            }
            .font(.callout)
            .accessibilityElement(children: .combine)
            .help(AppText.text(item.relative + "\n" + item.manufacturer + " · " + item.detail))
        }
    }

    private var emptyState: some View {
        VStack(spacing: short ? 8 : 16) {
            if model.isBusy {
                ProgressView().controlSize(.small)
                Text(AppText.text(model.phase == .scanning ? "正在识别照片…" : "正在准备照片…")).font(.headline)
                Text(AppText.text("照片会陆续显示在这里")).font(.callout).foregroundColor(.secondary)
            } else if reviewsPhotos && baseItems.isEmpty {
                Image(systemName:"photo.on.rectangle").font(.title2).foregroundColor(.secondary)
                Text(AppText.text(isExportPreview ? "本轮没有选定照片" : "本轮没有照片结果")).font(.headline)
                Button(AppText.text("返回导入照片")) { model.workspaceStep = .importPhotos }
            } else if model.items.isEmpty {
                if !short { MomentStaysBrandIcon().frame(width:64,height:64) }
                Text(AppText.text("导入要处理的照片"))
                    .font(.title3.weight(.semibold)).multilineTextAlignment(.center)
                Text(AppText.text(model.importScopeDescription))
                    .font(.callout).foregroundColor(.secondary).multilineTextAlignment(.center)
                    .frame(maxWidth:460).fixedSize(horizontal:false,vertical:true)
                HStack(spacing:12) {
                    Button(action:actions.chooseFolder) { Label(AppText.text("从文件夹导入"),systemImage:"folder") }
                        .disabled(!model.canImportSources).accessibilityIdentifier("import-from-folder")
                        .recordLayout("import-from-folder")
                    Button(action:actions.importPhotos) { Label(AppText.text("从相册导入"),systemImage:"photo.on.rectangle") }
                        .disabled(!model.canImportSources || !model.canImportFromLibrary)
                        .help(AppText.text(model.canImportFromLibrary ? "从系统照片相册导入" : "当前方向只支持本地原始 JPEG"))
                        .accessibilityIdentifier("import-from-album").recordLayout("import-from-album")
                }.controlSize(.large)
                if !short {
                    Label(AppText.text("本机处理，保留原片"),systemImage:"lock.shield")
                        .font(.caption).foregroundColor(.secondary)
                }
            } else {
                Image(systemName:scope.symbol).font(.title2).foregroundColor(.secondary).accessibilityHidden(true)
                Text(AppText.text(hasActiveFilters ? "没有符合筛选条件的照片" : scope.emptyTitle)).font(.headline)
                Button(AppText.text(hasActiveFilters ? "清除筛选并查看全部 \(model.items.count) 张" : "查看全部 \(model.items.count) 张照片")) { clearFilters() }
            }
        }
        .padding(InterfaceStyle.Spacing.content)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(InterfaceStyle.gallery)
    }
    private var hasActiveFilters: Bool {
        !model.migrationFeatureFilters.isEmpty || !model.migrationSearch.isEmpty || model.migrationSource != nil
    }

    @ViewBuilder private func detail(_ item: PhotoQueueItem) -> some View {
        if reviewsPhotos { reviewDetail(item) }
        else { VStack(alignment: .leading, spacing: 4) {
            Divider()
            HStack(spacing: 8) {
                Button { model.detailExpanded.toggle() } label: {
                    Label {
                        Text(AppText.text("文件详情"))
                    } icon: {
                        Image(systemName:"chevron.right").rotationEffect(.degrees(model.detailExpanded && !short ? 90 : 0))
                            .subtleAnimation(model.detailExpanded,duration:0.20)
                    }
                }.disabled(short).help(AppText.text(short ? "增加窗口高度后可展开详情" : "展开或收起选中文件的详情"))
                    .recordLayout("file-detail-button")
                Text(AppText.text(item.manufacturer + " · " + item.hdr)).font(.callout).foregroundColor(.secondary).lineLimit(1)
                Spacer(minLength: 0)
                coverButton(item)
                Button { actions.copy(item.detail) } label: { Image(systemName: "doc.on.doc") }
                    .help(AppText.text("复制详情")).accessibilityLabel(AppText.text("复制详情"))
            }.padding(.horizontal, 12)
            manualControls(item).padding(.horizontal,12)
            if model.detailExpanded && !short {
                ScrollView {
                    VStack(alignment: .leading, spacing: 6) {
                        Text(item.relative).fontWeight(.medium)
                        Text(AppText.text(item.detail))
                        Text(AppText.text("封面：" + item.cover)).foregroundColor(.secondary)
                        if !item.manufacturerDetail.isEmpty { Text(AppText.text(item.manufacturerDetail)).foregroundColor(.secondary) }
                    }.font(.callout).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
                }.frame(maxHeight: short ? 70 : 120).padding(.horizontal, 12)
                    .transition(.opacity)
            }
        }.padding(.bottom, 6).background(InterfaceStyle.canvas)
            .subtleAnimation(model.detailExpanded,duration:0.20)
            .recordLayout("file-detail")
        }
    }
}


private extension PhotoQueueView {
    func resultEntry(_ item: PhotoQueueItem) -> ReportEntry? {
        if let entry = resultEntries[item.id] { return entry }
        guard let group = model.burstGroups.first(where:{ $0.members.contains(where:{$0.source == item.id}) }) else { return nil }
        return group.members.compactMap { resultEntries[$0.source] }.first
    }
    func resultState(_ item: PhotoQueueItem) -> PhotoState? { resultEntry(item)?.state ?? item.resultState ?? item.originalExportState }
    func matchesResult(_ item: PhotoQueueItem, filter: String) -> Bool {
        filter == "all" || resultState(item)?.rawValue == filter || item.originalExportState?.rawValue == filter
    }
    func reviewTitle(_ item: PhotoQueueItem) -> String {
        if isResults && item.originalExportState == .copied && resultState(item) == .failed { return "转换失败 · 已原样输出" }
        return isExportPreview ? model.anticipatedStatus(item) : resultState(item)?.title ?? (model.phase == .stopped ? "已停止，未完成" : model.phase == .failed ? "未获得执行结果" : "等待处理")
    }
    func groupReviewTitle(_ members: [PhotoQueueItem]) -> String {
        let titles = Set(members.map(reviewTitle))
        return titles.count == 1 ? titles.first ?? "" : isExportPreview ? "部分预计可执行，展开查看" : "成员结果不同，展开查看"
    }
    func reviewLabel(_ item: PhotoQueueItem) -> some View {
        Label(AppText.text(reviewTitle(item)),systemImage:isExportPreview ? "eye" : resultState(item)?.symbol ?? "clock")
            .font(.caption).foregroundColor(isResults && resultState(item) == .failed ? .red : .secondary)
            .lineLimit(1).help(AppText.text(isExportPreview ? model.migrationPlanSummary(item) : resultEntry(item)?.detail ?? item.resultDetail ?? reviewTitle(item)))
            .recordLayout((isExportPreview ? "photo-estimate-" : "photo-result-") + item.id)
    }
    @ViewBuilder func statusLabel(_ item: PhotoQueueItem) -> some View {
        if reviewsPhotos { reviewLabel(item) }
        else { PhotoStatusLabel(state:model.displayState(item)).labelStyle(.titleOnly) }
    }
    var reviewHint: some View {
        HStack(spacing:8) {
            Image(systemName:isExportPreview ? "eye" : "checkmark.rectangle.stack").foregroundColor(.secondary)
            Text(AppText.text(isExportPreview ? "预计可执行 \(model.migrationReadyItems.count) 张；点击照片查看输出资源与原因。预估不代表实际成功。" : "文件生成与相册交付分别记录；点击照片查看实际结果和原因。"))
                .font(.caption).foregroundColor(.secondary).fixedSize(horizontal:false,vertical:true)
            Spacer(minLength:0)
        }.padding(.horizontal,12).padding(.vertical,5).background(InterfaceStyle.content)
            .recordLayout(isExportPreview ? "export-preview-summary" : "execution-results-summary")
    }
    var resultFilters: some View {
        ScrollView(.horizontal,showsIndicators:false) {
            HStack(spacing:5) {
                featureTab("全部",count:baseItems.count,selected:resultFilter == "all") { resultFilter = "all" }
                ForEach([PhotoState.converted,.resumed,.copied,.failed,.skipped],id:\.self) { state in
                    let count = baseItems.filter { matchesResult($0,filter:state.rawValue) }.count
                    if count > 0 {
                        featureTab(state.title,count:count,selected:resultFilter == state.rawValue) { resultFilter = state.rawValue }
                    }
                }
            }
        }.frame(maxWidth:.infinity,alignment:.leading).recordLayout("result-filter-tabs")
    }
    func reviewCardFooter(_ item: PhotoQueueItem) -> some View {
        Button { showPreview(item) } label: {
            VStack(alignment:.leading,spacing:3) {
                Text(item.name).font(.callout.weight(.medium)).foregroundColor(.primary).lineLimit(1).truncationMode(.middle)
                reviewLabel(item)
            }.frame(maxWidth:.infinity,alignment:.leading).contentShape(Rectangle())
        }.buttonStyle(.plain).accessibilityLabel(item.name + ", " + AppText.text(reviewTitle(item)) + AppText.text("，查看详情"))
    }
    @ViewBuilder func reviewInfo(_ item: PhotoQueueItem) -> some View {
        if isExportPreview {
            Text(AppText.text("本次预计输出")).font(.callout.weight(.semibold))
            Text(AppText.text(model.expectedResources(item))).font(.callout)
            Text(AppText.text(model.migrationPlanSummary(item))).font(.caption).foregroundColor(.secondary)
            if let time = model.anticipatedTime(item) { Text(AppText.text(time)).font(.caption).foregroundColor(.secondary) }
            Text(AppText.text("原片缩略图；未提前生成或模拟转换后的照片效果。"))
                .font(.caption).foregroundColor(.secondary)
            if model.category(item) == .manual {
                Button(AppText.text("返回选择特性并确认封面")) { model.workspaceStep = .conversion }
                    .disabled(model.isBusy || model.sharing)
            }
        } else {
            Text(AppText.text("执行结果")).font(.callout.weight(.semibold))
            Text(AppText.text(resultEntry(item)?.detail ?? item.resultDetail ?? "尚无已完成的处理记录。"))
                .font(.callout).textSelection(.enabled)
            if let original = item.originalDelivery { Text(AppText.text(original)).font(.callout).textSelection(.enabled) }
            if let output = resultEntry(item)?.output {
                Button(AppText.text("显示输出文件")) { actions.reveal(URL(fileURLWithPath:output)) }
            }
            if let output = item.originalExportOutput {
                Button(AppText.text("显示原样输出")) { actions.reveal(URL(fileURLWithPath:output)) }
            }
            if !model.deliverySummary.isEmpty { Text(AppText.text(model.deliverySummary)).font(.caption).foregroundColor(.secondary) }
        }
    }
    func reviewDetail(_ item: PhotoQueueItem) -> some View {
        VStack(alignment:.leading,spacing:6) {
            Divider()
            HStack {
                Text(item.name).font(.callout.weight(.medium)).lineLimit(1).truncationMode(.middle)
                Spacer(minLength:0)
                reviewLabel(item)
                Button(AppText.text("查看详情")) { showPreview(item) }
            }
            Text(AppText.text(isExportPreview ? model.expectedResources(item) : resultEntry(item)?.detail ?? item.resultDetail ?? item.originalDelivery ?? "尚无已完成的处理记录。"))
                .font(.caption).foregroundColor(.secondary).lineLimit(2)
        }.padding(.horizontal,12).padding(.bottom,8).background(InterfaceStyle.content)
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
            if expanded { Text(AppText.text(details)).transition(.opacity) }
            else { Text(AppText.text(shortTitle)).transition(.opacity) }
        }
        .font(.system(size:11,weight:.medium)).lineLimit(1)
        .padding(.horizontal,7).padding(.vertical,4)
        .foregroundColor(.white).background(Color.black.opacity(0.68)).cornerRadius(5)
        .background(HDRBadgeHoverArea(identifier:"photo-hdr-hover-" + sourceID,change:{ hovering = $0 }))
        .animation(usesReducedMotion ? nil : .easeInOut(duration:0.16),value:expanded)
        .focusable().focused($focused)
        .help(AppText.text(details)).accessibilityLabel(AppText.text("HDR 格式：" + details))
        .accessibilityHint(AppText.text("悬停或键盘聚焦可展开格式名称"))
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
                        Text(AppText.text(isPreview ? "示意照片" : "照片预览")).font(.caption)
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
            Text(AppText.text("连拍迁移计划")).font(.title2)
            Text(AppText.text("仅支持可信荣耀 JPEG → Apple。允许缺帧，保留现有成员的原始帧序号，不补帧、不合成视频。文件名不能证明相机原始总帧数；这里只核对当前来源中可见的成员。"))
                .font(.callout).foregroundColor(.secondary)
            ScrollView {
                LazyVStack(alignment:.leading,spacing:14) {
                    ForEach(model.burstGroups) { group in groupSection(group) }
                }.padding(4)
            }
            Text(AppText.text("文件标记校验与图库成组分别记录。导入后会回查全部成员、组标识和代表图；所选代表帧仅影响优先导入，Photos 不保证采用它。"))
                .font(.caption).foregroundColor(.secondary)
            Divider()
            HStack {
                Button(AppText.text("完成")) { model.burstReviewPresented = false }.keyboardShortcut(.cancelAction)
                Spacer()
                Button(AppText.text("保存选择并返回工作台")) { model.burstReviewPresented = false }
                    .accessibilityIdentifier("burst-save-button").keyboardShortcut(.defaultAction)
            }
        }.padding(20).frame(minWidth:600,idealWidth:720,maxWidth:820,minHeight:440,idealHeight:600,maxHeight:720)
    }
    private func groupSection(_ group: BurstGroup) -> some View {
        DisclosureGroup(isExpanded:Binding(get:{ expanded.contains(group.id) },set:{ if $0 { expanded.insert(group.id) } else { expanded.remove(group.id) } })) {
            VStack(alignment:.leading,spacing:10) {
                Text(AppText.text(URL(fileURLWithPath:group.members.first?.source ?? "").deletingLastPathComponent().path))
                    .font(.caption).textSelection(.enabled)
                HStack {
                    Button(AppText.text("选择全组")) { model.selectBurst(group,selected:true) }.disabled(!group.supported)
                    Button(AppText.text("取消全组")) { model.selectBurst(group,selected:false) }
                }
                Toggle(AppText.text("按所选独立照片导出（不写连拍标记）"),isOn:Binding(get:{ model.burstChoice(group).independent },set:{ model.setBurstChoice(group,independent:$0) }))
                    .disabled(!group.supported)
                ForEach(group.members) { member in
                    HStack(spacing:10) {
                        Toggle(AppText.text("第 \(group.members.firstIndex(where:{$0.source == member.source}).map { $0 + 1 } ?? 1) 帧"),isOn:Binding(get:{ !model.hasExplicitSelection || model.selectedIDs.contains(member.source) },set:{ model.setBurstMember(member,selected:$0) }))
                            .fixedSize().disabled(!group.supported)
                            .accessibilityIdentifier("burst-member-\(group.id)-\(group.members.firstIndex(where:{$0.source == member.source}).map { $0 + 1 } ?? 1)")
                        Text(URL(fileURLWithPath:member.source).lastPathComponent).lineLimit(1).truncationMode(.middle).help(AppText.text(member.source))
                        if member.source_cover { Text(AppText.text("原始 COVER")).font(.caption).foregroundColor(.secondary) }
                        Spacer(minLength:0)
                        Button(AppText.text(model.burstChoice(group).representative == member.source ? "代表帧 ✓" : "设为代表帧")) { model.setBurstChoice(group,representative:member.source) }
                            .disabled(model.burstChoice(group).independent || !group.supported)
                            .accessibilityLabel(AppText.text("将第 \(group.members.firstIndex(where:{$0.source == member.source}).map { $0 + 1 } ?? 1) 帧设为代表帧"))
                            .accessibilityIdentifier("burst-representative-\(group.id)-\(group.members.firstIndex(where:{$0.source == member.source}).map { $0 + 1 } ?? 1)")
                    }
                }
            }.padding(.vertical,8)
        } label: {
            VStack(alignment:.leading,spacing:4) {
                Text(AppText.text("\(group.label) · \(group.members.count) 帧 · 已选 \(model.burstSelectedCount(group)) 帧")).font(.headline)
                if let reason = model.burstBlockingReason(group) { Label(AppText.text(reason),systemImage:"exclamationmark.triangle").font(.callout).foregroundColor(.secondary) }
                else { Text(AppText.text(model.burstChoice(group).independent ? "独立照片 · 原片保留" : group.has_gaps == true ? "允许缺帧 · 保留现有原帧序 · 整组原子输出" : "按原始帧序 · 整组原子输出")).font(.caption).foregroundColor(.secondary) }
            }
        }
    }
}
