import SwiftUI
import UniformTypeIdentifiers

struct WorkspaceView: View {
    @Environment(\.locale) private var locale
    @ObservedObject var model: WorkspaceModel
    let actions: WorkspaceActions
    var layoutObserver: ([String: CGRect]) -> Void = { _ in }
    @State private var outputDropTargeted = false
    private var queueScope: Binding<PhotoQueueScope> {
        Binding(get: { model.queueScope }, set: { model.selectQueueScope($0) })
    }
    @State private var showingError = false
    @FocusState private var settingsFocused: Bool

    var body: some View {
        GeometryReader { geometry in
            let layout = WorkspaceLayout(size: geometry.size)
            VStack(spacing: 0) {
                workspaceHeader(layout)
                Divider()
                PhotoQueueView(model:model,actions:actions,short:layout.short,scope:queueScope)
                    .environment(\.workspaceTaskFeedback,AnyView(WorkspaceTaskFeedback(model:model,actions:actions,showError:{ showingError = true })))
                    .frame(maxWidth:.infinity,maxHeight:.infinity)
            }
            .padding(.top,28)
            .background(InterfaceStyle.canvas)
            .coordinateSpace(name: "workspace")
            .recordLayout("workspace")
            .onPreferenceChange(LayoutFramesKey.self, perform: layoutObserver)
            .sheet(isPresented: $model.settingsPresented, onDismiss: { settingsFocused = true }) {
                ExportSettingsSheet(model: model, availableSize: geometry.size, actions: actions)
            }
        }
        .ignoresSafeArea(.container,edges:.top)
        .font(.body)
        .tint(InterfaceStyle.accent).accentColor(InterfaceStyle.accent)
        .onAppear {
            #if UI_DIAGNOSTICS
            if let index = CommandLine.arguments.firstIndex(of:"--ui-scope"), index + 1 < CommandLine.arguments.count,
               let value = PhotoQueueScope(rawValue:CommandLine.arguments[index + 1]) { model.selectQueueScope(value) }
            #endif
        }
        .alert("无法完成操作", isPresented: $showingError) {
            Button("好", role: .cancel) {}
        } message: { Text(model.errorMessage ?? "") }
    }

    private var direction: Binding<ConversionDirection> {
        Binding(get: { model.settings.direction }, set: { value in model.updateSettings { $0.direction = value } })
    }

    private var addButton: some View {
        SourceMenuButton(requested:$model.sourceMenuRequested,disabled:!model.canAppendSources,
            title:model.sources.isEmpty ? "添加照片" : model.sourceLabel,
            details:model.sources.isEmpty ? "" : "来源：" + model.sourceLabel + "\n" + model.sources.map(\.path).joined(separator:"\n"),
            hasSources:!model.sources.isEmpty,allowsLibrary:model.settings.direction != .androidToAndroid,addFiles:actions.chooseSources,addLibrary:actions.importPhotos)
            .frame(width:model.sources.isEmpty ? 96 : 160)
            .fixedSize(horizontal:false,vertical:true).recordLayout("add-button")
    }

    private var destination: Binding<OutputDestination> {
        Binding(get:{ model.settings.destination },set:{ value in model.updateSettings { $0.destination = value } })
    }
    private var vendor: Binding<AndroidVendor> {
        Binding(get:{ model.settings.vendor },set:{ value in model.updateSettings { $0.vendor = value } })
    }
    private var outputControl: some View {
        OutputLocationControl(title:"输出：" + outputSummary,details:outputHelp,
            symbol:model.settings.destination.importsPhotos ? "photo.on.rectangle" : model.settings.destination.shares ? "square.and.arrow.up" : "folder",
            disabled:model.isBusy || model.sharing,destination:destination,destinations:model.settings.destinations,
            outputPath:model.settings.destination.usesCache ? nil : model.output?.path,
            changeLocation:{
                if model.settings.destination.usesCache { model.settingsPresented = true }
                else { actions.chooseOutput() }
            },chooseFolder:actions.chooseOutput,chooseAlbum:actions.chooseAlbum,
            copyPath:{ if let output = model.output { actions.copy(output.path) } },
            chooseDestination:model.settings.direction == .toApple ? actions.chooseDestination : nil)
        .frame(height:20)
        .help(outputHelp)
        .accessibilityLabel("输出：" + outputSummary)
        .accessibilityHint("点击更改位置，使用菜单切换保存方式")
        .accessibilityIdentifier("output-location-control")
        .frame(minWidth:150,maxWidth:.infinity)
        .background(outputDropTargeted ? InterfaceStyle.selection : .clear)
        .onDrop(of:[.fileURL],isTargeted:$outputDropTargeted) { providers in
            guard !model.isBusy, !model.sharing, !model.settings.destination.usesCache else { return false }
            return actions.acceptDrop(providers,true)
        }.recordLayout("output-control")
    }

    private var outputHelp: String {
        if model.settings.direction == .toApple { return "选择相册、文件夹或相册及文件夹；取消保留当前设置" }
        let action = model.settings.destination.usesCache ? "点击打开导出设置；箭头切换保存方式" : "点击更改文件夹；箭头切换保存方式，也可拖入文件夹"
        return "输出：" + outputSummary + (model.settings.destination.usesCache ? "" : "\n" + (model.output?.path ?? "尚未选择文件夹")) + "\n" + action
    }
    private var vendorPicker: some View {
        Picker("目标手机",selection:vendor) {
            ForEach(AndroidVendor.allCases,id:\.self) { Text($0.title).tag($0) }
        }.frame(maxWidth:220).disabled(model.isBusy || model.sharing)
    }
    private var moreOptions: some View {
        HStack(spacing:8) {
            Button("导出设置…") { model.settingsPresented = true }
                .focused($settingsFocused).accessibilityIdentifier("settings-button").recordLayout("settings-button")
                .disabled(model.isBusy || model.sharing)
            Menu {
                Button("最近结果…",action:actions.recentResults).disabled(model.isBusy || model.sharing)
                Button("打开输出文件夹",action:actions.openOutput).disabled(!model.canOpenOutput).help(model.outputOpenHelp)
                if model.report != nil { Button("转换报告…",action:actions.showReport).disabled(model.isBusy) }
                if model.shareReady { Button("共享结果…",action:actions.shareResults).disabled(model.isBusy || model.sharing) }
                if model.errorMessage?.isEmpty == false { Button("查看错误原因…") { showingError = true } }
                if model.libraryMonitoring { Button("停止图库监测",action:actions.toggleLibraryMonitoring) }
                Divider()
                Link(UserGuide.title(for:locale),destination:UserGuide.url(for:locale))
            } label: { Image(systemName:"ellipsis.circle") }
                .menuStyle(.borderlessButton).fixedSize()
                .help("结果与更多操作").accessibilityLabel("结果与更多操作")
                .recordLayout("workspace-more-menu")
            primaryAction.frame(width:96)
        }.fixedSize()
    }
    private func workspaceHeader(_ layout: WorkspaceLayout) -> some View {
        Group {
            if layout.size.width >= 1440 {
                HStack(spacing:12) {
                    addButton
                    directionPicker.frame(width:380)
                    outputControl
                    if model.settings.direction == .toAndroid { vendorPicker.frame(maxWidth:160) }
                    if model.settings.direction == .androidToAndroid { Text("暂仅支持 Google → 荣耀").font(.caption).foregroundColor(.secondary).fixedSize() }
                    Spacer(minLength:0)
                    moreOptions
                }
            } else {
                VStack(alignment:.leading,spacing:8) {
                    HStack(spacing:16) {
                        directionPicker.frame(minWidth:280,maxWidth:440)
                        Spacer(minLength:0)
                        moreOptions
                    }
                    HStack(spacing:12) {
                        addButton
                        outputControl
                        if model.settings.direction == .toAndroid { vendorPicker.frame(maxWidth:160) }
                        if model.settings.direction == .androidToAndroid { Text("暂仅支持 Google → 荣耀").font(.caption).foregroundColor(.secondary).fixedSize() }
                        Spacer(minLength:0)
                    }
                }
            }
        }
        .padding(.horizontal,layout.margin).padding(.vertical,8).recordLayout("command-bar")
    }

    private var directionPicker: some View {
        Picker("转换方向",selection:direction) {
            ForEach(ConversionDirection.allCases,id:\.self) { Text($0.title).tag($0) }
        }
        .pickerStyle(.segmented).labelsHidden()
        .disabled(!model.canSwitchDirection)
        .accessibilityIdentifier("conversion-direction").recordLayout("conversion-directions")
    }

    @ViewBuilder private var primaryAction: some View {
        if model.isBusy {
            Button(action: model.stop) {
                Label(model.phase == .stopping ? "停止中" : "停止", systemImage: "stop.fill")
            }
            .disabled(!model.canStop).recordLayout("primary-action")
            .accessibilityIdentifier("stop-button")
        } else {
            Button(model.settings.actionTitle, action: actions.start)
                .help("未指定选择时转换全部可转换照片；指定后仅转换选中项（含其他标签）")
                .buttonStyle(.borderedProminent).tint(InterfaceStyle.accent).foregroundColor(InterfaceStyle.accentForeground)
                .disabled(!model.canStart || model.sharing)
                .recordLayout("primary-action").accessibilityIdentifier("convert-button")
        }
    }

    private var outputSummary: String {
        if model.settings.direction == .toApple && model.settings.outputConfirmed != true { return "询问位置" }
        switch model.settings.destination {
        case .ask: return "询问位置"
        case .photos: return "相册"
        case .folderAndPhotos: return (model.output?.lastPathComponent ?? "待选择文件夹") + " ＋ 照片图库"
        case .share: return "转换后共享"
        case .folderAndShare: return "文件夹＋共享 · " + (model.output?.lastPathComponent ?? "待选择")
        case .folder: return model.output?.lastPathComponent ?? "选择文件夹…"
        }
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
        case .delivering: return "正在保存转换结果"
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
