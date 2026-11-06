import SwiftUI
import UniformTypeIdentifiers

struct WorkspaceView: View {
    @Environment(\.locale) private var locale
    @ObservedObject var model: WorkspaceModel
    let actions: WorkspaceActions
    var layoutObserver: ([String: CGRect]) -> Void = { _ in }
    @State private var outputDropTargeted = false
    @State private var showingError = false
    @FocusState private var settingsFocused: Bool
    private var queueScope: Binding<PhotoQueueScope> {
        Binding(get:{ model.queueScope },set:{ model.selectQueueScope($0) })
    }

    var body: some View {
        GeometryReader { geometry in
            let layout = WorkspaceLayout(size:geometry.size)
            VStack(spacing:0) {
                workspaceHeader(layout)
                Divider()
                HStack(spacing:0) {
                    sourceSidebar.frame(width:geometry.size.width < 1000 ? 148 : layout.sidebarWidth)
                    Divider()
                    PhotoQueueView(model:model,actions:actions,short:layout.short,scope:queueScope)
                        .environment(\.workspaceTaskFeedback,AnyView(WorkspaceTaskFeedback(model:model,actions:actions,showError:{ showingError = true })))
                        .frame(maxWidth:.infinity,maxHeight:.infinity)
                }
            }
            .padding(.top,28).background(InterfaceStyle.canvas)
            .coordinateSpace(name:"workspace").recordLayout("workspace")
            .onPreferenceChange(LayoutFramesKey.self,perform:layoutObserver)
            .sheet(isPresented:$model.settingsPresented,onDismiss:{ settingsFocused = true }) {
                ExportSettingsSheet(model:model,availableSize:geometry.size,actions:actions)
            }
            .sheet(isPresented:$model.migrationReviewPresented) {
                MigrationPlanSheet(model:model,actions:actions,availableSize:geometry.size)
            }
        }
        .ignoresSafeArea(.container,edges:.top).font(.body)
        .tint(InterfaceStyle.accent).accentColor(InterfaceStyle.accent)
        .onAppear {
            #if UI_DIAGNOSTICS
            if let index = CommandLine.arguments.firstIndex(of:"--ui-scope"), index + 1 < CommandLine.arguments.count,
               let value = PhotoQueueScope(rawValue:CommandLine.arguments[index + 1]) { model.selectQueueScope(value) }
            #endif
        }
        .alert("无法完成操作",isPresented:$showingError) { Button("好",role:.cancel) {} }
            message: { Text(model.errorMessage ?? "") }
    }

    private var target: Binding<MigrationTargetPreset?> {
        Binding(get:{ model.migrationTargetConfirmed ? model.migrationTarget : nil },set:{ if let value = $0 { model.selectMigrationTarget(value) } })
    }
    private var destination: Binding<OutputDestination> {
        Binding(get:{ model.settings.destination },set:{ value in model.updateSettings { $0.destination = value } })
    }
    private var addButton: some View {
        SourceMenuButton(requested:$model.sourceMenuRequested,disabled:!model.migrationTargetConfirmed || !model.canAppendSources,
            title:"添加来源…",details:model.migrationTargetConfirmed ? model.sources.map(\.path).joined(separator:"\n") : "请先选择目标兼容格式",
            hasSources:!model.sources.isEmpty,allowsLibrary:model.settings.direction != .androidToAndroid,
            addFiles:actions.chooseSources,addLibrary:actions.importPhotos)
            .frame(width:132).fixedSize(horizontal:false,vertical:true).recordLayout("add-button")
    }
    private var outputControl: some View {
        OutputLocationControl(title:"输出：" + outputSummary,details:outputHelp,
            symbol:model.settings.destination.importsPhotos ? "photo.on.rectangle" : model.settings.destination.shares ? "square.and.arrow.up" : "folder",
            disabled:!model.migrationTargetConfirmed || model.isBusy || model.sharing,destination:destination,destinations:model.settings.destinations,
            outputPath:model.settings.destination.usesCache ? nil : model.output?.path,
            changeLocation:{ if model.settings.destination.usesCache { model.settingsPresented = true } else { actions.chooseOutput() } },
            chooseFolder:actions.chooseOutput,chooseAlbum:actions.chooseAlbum,
            copyPath:{ if let output = model.output { actions.copy(output.path) } },
            chooseDestination:model.settings.direction == .toApple ? actions.chooseDestination : nil)
            .frame(height:20).frame(minWidth:150,maxWidth:.infinity)
            .help(outputHelp).accessibilityLabel("输出：" + outputSummary)
            .background(outputDropTargeted ? InterfaceStyle.selection : .clear)
            .onDrop(of:[.fileURL],isTargeted:$outputDropTargeted) { providers in
                guard model.migrationTargetConfirmed, !model.isBusy, !model.sharing, !model.settings.destination.usesCache else { return false }
                return actions.acceptDrop(providers,true)
            }.recordLayout("output-control")
    }
    private func workspaceHeader(_ layout: WorkspaceLayout) -> some View {
        VStack(alignment:.leading,spacing:10) {
            HStack(alignment:.bottom,spacing:16) {
                VStack(alignment:.leading,spacing:4) {
                    Text("1  目标兼容格式").font(.caption).foregroundColor(.secondary)
                    Picker("目标兼容格式",selection:target) {
                        Text("先选择照片要使用的平台…").tag(Optional<MigrationTargetPreset>.none)
                        ForEach(MigrationTargetPreset.allCases,id:\.self) { Text($0.title).tag(Optional($0)) }
                    }.labelsHidden().disabled(!model.canSwitchDirection)
                        .accessibilityIdentifier("migration-target").recordLayout("conversion-directions")
                }.frame(minWidth:240,maxWidth:360)
                VStack(alignment:.leading,spacing:4) {
                    Text("2  来源").font(.caption).foregroundColor(.secondary)
                    addButton
                }
                Spacer(minLength:0)
                primaryAction.frame(width:148)
            }
            HStack(spacing:12) {
                outputControl
                Button("迁移选项…") { model.settingsPresented = true }
                    .focused($settingsFocused).disabled(!model.migrationTargetConfirmed || model.isBusy || model.sharing)
                    .accessibilityIdentifier("settings-button").recordLayout("settings-button")
                Button("导出结果…",action:actions.exportResults).disabled(!model.resultExportAvailable || model.isBusy || model.sharing)
                    .accessibilityIdentifier("export-results-button")
                resultMenu
            }
            UnconvertedExportControl(model:model,actions:actions)
            Text(model.migrationTargetConfirmed ? model.migrationTarget.detail : "先确定目标，再添加来源并处理。生成独立副本，原片保持不变。")
                .font(.caption).foregroundColor(.secondary).lineLimit(2).fixedSize(horizontal:false,vertical:true)
        }.padding(.horizontal,layout.margin).padding(.vertical,10).recordLayout("command-bar")
    }
    private var resultMenu: some View {
        Menu {
            Button("查看迁移计划…") { model.migrationReviewPresented = true }.disabled(!model.canReviewMigration)
            Button("最近结果…",action:actions.recentResults).disabled(model.isBusy || model.sharing)
            Button("打开输出文件夹",action:actions.openOutput).disabled(!model.canOpenOutput).help(model.outputOpenHelp)
            if model.report != nil { Button("处理报告…",action:actions.showReport).disabled(model.isBusy) }
            if model.shareReady { Button("共享结果…",action:actions.shareResults).disabled(model.isBusy || model.sharing) }
            if model.errorMessage?.isEmpty == false { Button("查看错误原因…") { showingError = true } }
            if model.libraryMonitoring { Button("停止图库监测",action:actions.toggleLibraryMonitoring) }
            Divider()
            Link(UserGuide.title(for:locale),destination:UserGuide.url(for:locale))
        } label: { Image(systemName:"ellipsis.circle") }
            .menuStyle(.borderlessButton).fixedSize().help("结果与更多操作").accessibilityLabel("结果与更多操作")
            .recordLayout("workspace-more-menu")
    }
    @ViewBuilder private var primaryAction: some View {
        if model.isBusy {
            Button(action:model.stop) { Label(model.phase == .stopping ? "停止中" : "停止",systemImage:"stop.fill") }
                .disabled(!model.canStop).recordLayout("primary-action").accessibilityIdentifier("stop-button")
        } else {
            Button(model.hasExplicitSelection && model.selectedIDs.isEmpty ? "已选 0 张" : "开始处理") { model.approveMigrationReview() }
                .buttonStyle(.borderedProminent).tint(InterfaceStyle.accent).foregroundColor(InterfaceStyle.accentForeground)
                .disabled(!model.canApproveMigration).help("直接处理本轮就绪项，包含隐藏选中；未确认手动项与不支持项不阻断无关就绪照片")
                .recordLayout("primary-action").accessibilityIdentifier("convert-button")
        }
    }
    private var sourceSidebar: some View {
        ScrollView {
            VStack(alignment:.leading,spacing:12) {
              Group {
                Text("来源").font(.headline)
                Text("\(model.sources.count) 个入口 · \(model.items.count) 张").font(.caption).foregroundColor(.secondary)
                sidebarButton("全部来源",symbol:"square.stack",selected:model.migrationSource == nil) { model.migrationSource = nil }
                LazyVStack(alignment:.leading,spacing:4) {
                    ForEach(model.sources,id:\.path) { source in
                        sidebarButton(source.lastPathComponent,symbol:"folder",selected:model.migrationSource == source.path) { model.migrationSource = source.path }
                            .help(source.path)
                    }
                }
              }
                Divider().padding(.vertical,4)
                Text("操作顺序").font(.headline)
                Text("选择目标\n添加来源\n查看计划（可选）\n处理与交付").font(.callout).foregroundColor(.secondary).lineSpacing(9)
                Divider().padding(.vertical,4)
              Group {
                Text("任务与结果").font(.headline)
                Text(model.isBusy ? model.status : !model.deliverySummary.isEmpty ? model.deliverySummary : model.report?.summary ?? "尚无本轮结果").font(.caption).foregroundColor(.secondary)
                Button("导出转换结果…",action:actions.exportResults).disabled(!model.resultExportAvailable || model.isBusy || model.sharing)
                Button("最近结果…",action:actions.recentResults).disabled(model.isBusy || model.sharing)
                if model.libraryMonitoring {
                    Label("正在监测图库",systemImage:"photo.on.rectangle").font(.caption).foregroundColor(.secondary)
                    Button("停止监测",action:actions.toggleLibraryMonitoring).disabled(model.isBusy)
                }
                Text("转换与原样导出分开计数。打开共享菜单不代表设备已接收。")
                    .font(.caption).foregroundColor(.secondary).padding(.top,8)
              }
            }.frame(maxWidth:.infinity,alignment:.leading).padding(14)
        }.background(InterfaceStyle.content).recordLayout("source-sidebar")
    }
    private func sidebarButton(_ title: String, symbol: String, selected: Bool, action: @escaping () -> Void) -> some View {
        Button(action:action) {
            Label(title,systemImage:symbol).font(.callout).lineLimit(1).truncationMode(.middle)
                .frame(maxWidth:.infinity,alignment:.leading).padding(.horizontal,7).padding(.vertical,6)
                .background(selected ? InterfaceStyle.selection : .clear).cornerRadius(InterfaceStyle.controlRadius)
        }.buttonStyle(.plain).accessibilityAddTraits(selected ? .isSelected : [])
    }
    private var outputHelp: String {
        "输出：" + outputSummary + "\n" + (model.settings.destination.usesCache ? "使用持久任务缓存；可在结果中再次导出" : model.output?.path ?? "尚未选择文件夹")
    }
    private var outputSummary: String {
        if !model.outputIsChosen { return "选择输出位置…" }
        switch model.settings.destination {
        case .ask: return "询问位置"
        case .photos: return "照片图库"
        case .folderAndPhotos: return (model.output?.lastPathComponent ?? "文件夹") + " ＋ 照片图库"
        case .share: return "任务缓存 · 共享菜单"
        case .folderAndShare: return (model.output?.lastPathComponent ?? "文件夹") + " ＋ 共享菜单"
        case .folder: return model.output?.lastPathComponent ?? "选择文件夹…"
        }
    }
}

struct UnconvertedExportControl: View {
    @ObservedObject var model: WorkspaceModel
    let actions: WorkspaceActions
    private var enabled: Binding<Bool> {
        Binding(get:{ model.unconvertedExport != .none },set:{ value in
            model.unconvertedExport = value ? model.unconvertedExportLocation : .none
        })
    }
    private var location: Binding<UnconvertedExportChoice> {
        Binding(get:{ model.unconvertedExportLocation },set:{ value in
            model.unconvertedExportLocation = value
            if model.unconvertedExport != .none { model.unconvertedExport = value }
        })
    }
    var body: some View {
        VStack(alignment:.leading,spacing:7) {
            HStack(spacing:10) {
                Toggle("原样导出未转换照片",isOn:enabled).toggleStyle(.checkbox).fixedSize()
                if model.unconvertedExport != .none {
                    Picker("原样导出位置",selection:location) {
                        Text("导出到相同位置").tag(UnconvertedExportChoice.sameLocation)
                        Text("导出到不同位置").tag(UnconvertedExportChoice.separateLocation)
                    }.labelsHidden().frame(maxWidth:220)
                    if model.unconvertedExport == .separateLocation {
                        Button("选择目录…",action:actions.chooseUnconvertedOutput)
                    }
                }
                Spacer(minLength:0)
            }.font(.callout).disabled(model.isBusy || model.sharing)
            if model.unconvertedExport != .none {
                Text(model.unconvertedExport == .separateLocation ? model.unconvertedOutput?.path ?? "请选择不同的原文件导出目录" : "使用本次转换输出位置，另建原文件目录，不覆盖来源；原样导出不会计为转换成功。")
                    .font(.caption).foregroundColor(.secondary).lineLimit(2).textSelection(.enabled)
            }
        }.accessibilityIdentifier("unconverted-export-options")
    }
}

struct MigrationPlanSheet: View {
    @ObservedObject var model: WorkspaceModel
    let actions: WorkspaceActions
    let availableSize: CGSize
    var body: some View {
        VStack(alignment:.leading,spacing:0) {
            VStack(alignment:.leading,spacing:5) {
                Text("本轮迁移计划").font(.title3.weight(.semibold))
                Text(model.migrationTarget.title).font(.callout).foregroundColor(.secondary)
                Text(model.hasExplicitSelection ? "已选 \(model.selectedIDs.count) 张 · 隐藏选中 \(model.hiddenMigrationSelectionCount) 张" : "范围：全部待处理照片")
                    .font(.caption).foregroundColor(.secondary)
            }.padding(16)
            Divider()
            ScrollView {
                VStack(alignment:.leading,spacing:16) {
                    HStack {
                        Text(model.outputIsChosen ? "输出：" + model.settings.destination.title : "请先选择输出位置").font(.callout)
                        Spacer()
                        Button("选择输出位置…",action:actions.chooseOutput)
                    }
                    if let output = model.output, !model.settings.destination.usesCache { Text(output.path).font(.caption).foregroundColor(.secondary).textSelection(.enabled) }
                    UnconvertedExportControl(model:model,actions:actions)
                    HStack {
                        Text("就绪 \(model.migrationReadyItems.count) 张 · 未转换 \(model.migrationUnconvertedItems.count) 张").font(.headline)
                        Spacer()

                    }
                    LazyVStack(alignment:.leading,spacing:10) {
                        ForEach(model.migrationScopeItems) { item in
                            VStack(alignment:.leading,spacing:5) {
                                HStack { Text(item.name).font(.callout.weight(.medium)).lineLimit(2); Spacer(); PhotoStatusLabel(state:model.displayState(item)) }
                                Text(model.migrationPlanSummary(item)).font(.callout).foregroundColor(.secondary).fixedSize(horizontal:false,vertical:true)
                                if model.category(item) == .manual {
                                    Button("前往手动处理") { model.selection = item.id; model.selectQueueScope(.manual); model.migrationReviewPresented = false }
                                }
                                Divider()
                            }
                        }
                    }
                    Text("原片保持不变。静态 HDR 不生成视频或配对；连拍保留现有成员的原序号，缺帧不补、原总数未知。文件生成、原样导出、图库与共享分别记录。")
                        .font(.caption).foregroundColor(.secondary)
                }.padding(16)
            }
            Divider()
            HStack {
                Button("返回编辑") { model.migrationReviewPresented = false }.keyboardShortcut(.cancelAction)
                Spacer()
                Button(model.migrationReadyItems.isEmpty ? "导出未转换项" : "处理就绪的 \(model.migrationReadyItems.count) 张") { model.approveMigrationReview() }
                    .buttonStyle(.borderedProminent).tint(InterfaceStyle.accent).foregroundColor(InterfaceStyle.accentForeground)
                    .disabled(!model.canApproveMigration).accessibilityIdentifier("migration-plan-start")
            }.padding(16)
        }.frame(width:min(700,max(540,availableSize.width - 32)),height:min(680,max(400,availableSize.height - 52)))
            .background(InterfaceStyle.canvas).tint(InterfaceStyle.accent).accentColor(InterfaceStyle.accent)
    }
}
private struct WorkspaceTaskFeedbackKey: EnvironmentKey {
    static let defaultValue = AnyView(EmptyView())
}

extension EnvironmentValues {
    var workspaceTaskFeedback: AnyView {
        get { self[WorkspaceTaskFeedbackKey.self] }
        set { self[WorkspaceTaskFeedbackKey.self] = newValue }
    }
}

private struct WorkspaceTaskFeedback: View {
    @ObservedObject var model: WorkspaceModel
    let actions: WorkspaceActions
    let showError: () -> Void
    @State private var completionVisible = false
    @State private var completionGeneration = UUID()

    private var hasError: Bool { model.errorMessage?.isEmpty == false }
    private var hasFailures: Bool { (model.report?.counts["failed"] ?? 0) > 0 }
    private var visible: Bool {
        model.isBusy || model.sharing || hasError || hasFailures || completionVisible ||
        model.phase == .stopped || model.phase == .failed || model.notice != nil ||
        model.libraryMonitoring || !model.deliverySummary.isEmpty || !model.librarySummary.isEmpty
    }
    private var busy: Bool { model.isBusy || model.sharing }
    private var message: String {
        if hasError { return model.errorMessage ?? "任务未完成" }
        if busy { return model.sharing ? "正在共享…" : model.status }
        if let notice = model.notice { return notice }
        if hasFailures { return "已完成，部分照片需要检查" }
        if !model.deliverySummary.isEmpty { return model.deliverySummary }
        if model.phase == .stopped || model.phase == .failed { return model.status }
        if completionVisible { return model.report?.summary ?? model.status }
        if model.libraryMonitoring { return "正在监测图库" }
        return model.librarySummary
    }
    private var secondaryMessage: String {
        if model.sharing { return "正在处理共享内容" }
        if !model.currentFile.isEmpty { return model.currentFile }
        switch model.phase {
        case .scanning: return "正在识别照片"
        case .stopping: return "正在结束当前处理"
        case .awaitingDecision: return "请确认已有输出的处理方式"
        case .delivering: return "正在保存处理结果"
        case .library: return "正在读取图库资源"
        default: return "正在准备照片"
        }
    }
    private var statusSymbol: String {
        if hasError || hasFailures { return "exclamationmark.circle" }
        if busy { return "arrow.triangle.2.circlepath" }
        if model.phase == .completed { return "checkmark.circle" }
        if model.phase == .stopped { return "stop.circle" }
        return "info.circle"
    }

    var body: some View {
        Group {
            if visible {
                VStack(alignment:.leading,spacing:6) {
                    HStack(spacing:8) {
                        Label {
                            Text(message).foregroundColor(hasError ? .red : .secondary)
                        } icon: {
                            Image(systemName:statusSymbol)
                                .foregroundColor(hasError ? .red : model.phase == .completed && !hasFailures ? .green : .secondary)
                        }.lineLimit(1).help(message)
                            .id(model.phase).transition(.opacity).subtleAnimation(model.phase)
                        Spacer(minLength:8)
                        if busy, let progress = model.progress {
                            Text("\(Int((min(1,max(0,progress)) * 100).rounded()))%")
                                .monospacedDigit().foregroundColor(.secondary)
                        }
                        if hasError { Button("查看原因",action:showError) }
                        if hasFailures { Button("查看报告",action:actions.showReport).disabled(model.isBusy) }
                        if model.libraryMonitoring && !busy { Button("停止监测",action:actions.toggleLibraryMonitoring) }
                    }.font(.callout)
                    if busy {
                        taskProgress
                        Text(secondaryMessage).font(.caption).foregroundColor(.secondary)
                            .lineLimit(1).truncationMode(.middle).help(secondaryMessage)
                            .transaction { $0.animation = nil }
                    }
                }
                .padding(.horizontal,16).padding(.vertical,8)
                .frame(maxWidth:.infinity,minHeight:busy ? 60 : 36,alignment:.leading)
                .background(InterfaceStyle.content)
                .recordLayout("task-feedback")
                .transition(.opacity)
            }
        }
        .subtleAnimation(visible,duration:0.18)
        .subtleAnimation(busy,duration:0.18)
        .onChange(of:model.phase) { phase in
            let generation = UUID(); completionGeneration = generation
            completionVisible = phase == .completed
            if completionVisible {
                DispatchQueue.main.asyncAfter(deadline:.now() + 3.5) {
                    if completionGeneration == generation { completionVisible = false }
                }
            }
        }
    }

    private var taskProgress: some View {
        Group {
            if let progress = model.progress {
                ProgressView(value:min(1,max(0,progress))).progressViewStyle(.linear)
                    .subtleAnimation(progress,duration:0.10)
            } else { ProgressView().progressViewStyle(.linear) }
        }
        .frame(height:6).tint(InterfaceStyle.accent)
        .accessibilityLabel("任务进度")
        .recordLayout("task-progress")
    }
}
