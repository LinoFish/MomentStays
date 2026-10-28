import SwiftUI
import UniformTypeIdentifiers

struct WorkspaceView: View {
    @Environment(\.locale) private var locale
    @ObservedObject var model: WorkspaceModel
    let actions: WorkspaceActions
    var layoutObserver: ([String: CGRect]) -> Void = { _ in }
    @State private var outputDropTargeted = false
    @State private var queueScope = PhotoQueueScope.ready
    @State private var showingError = false
    @FocusState private var settingsFocused: Bool

    var body: some View {
        GeometryReader { geometry in
            let layout = WorkspaceLayout(size: geometry.size)
            VStack(spacing: 0) {
                workspaceHeader(layout)
                Divider()
                PhotoQueueView(model:model,actions:actions,short:layout.short,scope:$queueScope)
                    .frame(maxWidth:.infinity,maxHeight:.infinity)
                Divider()
                taskStatus(layout)
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
        .onChange(of:model.sources) { _ in queueScope = .ready }
        .onAppear {
            #if UI_DIAGNOSTICS
            if let index = CommandLine.arguments.firstIndex(of:"--ui-scope"), index + 1 < CommandLine.arguments.count,
               let value = PhotoQueueScope(rawValue:CommandLine.arguments[index + 1]) { queueScope = value }
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
        SourceMenuButton(requested:$model.sourceMenuRequested,disabled:model.isBusy || model.sharing,
            title:model.sources.isEmpty ? "添加照片" : model.sourceLabel,
            details:model.sources.isEmpty ? "" : "来源：" + model.sourceLabel + "\n" + model.sources.map(\.path).joined(separator:"\n"),
            hasSources:!model.sources.isEmpty,addFiles:actions.chooseSources,addLibrary:actions.importPhotos)
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
            copyPath:{ if let output = model.output { actions.copy(output.path) } })
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
        let action = model.settings.destination.usesCache ? "点击打开导出设置；箭头切换保存方式" : "点击更改文件夹；箭头切换保存方式，也可拖入文件夹"
        return "输出：" + outputSummary + (model.settings.destination.usesCache ? "" : "\n" + (model.output?.path ?? "尚未选择文件夹")) + "\n" + action
    }
    private var vendorPicker: some View {
        Picker("目标手机",selection:vendor) {
            ForEach(AndroidVendor.allCases,id:\.self) { Text($0.title).tag($0) }
        }.frame(maxWidth:220)
    }
    private var moreOptions: some View {
        HStack(spacing:12) {
            Button("导出设置…") { model.settingsPresented = true }
                .focused($settingsFocused).accessibilityIdentifier("settings-button").recordLayout("settings-button")
            Link(destination:UserGuide.url(for:locale)) { Image(systemName:"questionmark.circle").foregroundColor(InterfaceStyle.accent) }
                .help(UserGuide.title(for:locale)).accessibilityLabel(UserGuide.title(for:locale)).accessibilityIdentifier("user-guide-button")
        }
    }
    private func workspaceHeader(_ layout: WorkspaceLayout) -> some View {
        Group {
            if layout.size.width >= 1040 {
                HStack(spacing:12) {
                    addButton
                    directionPicker.frame(width:270)
                    outputControl
                    if model.settings.direction == .toAndroid { vendorPicker.frame(maxWidth:160) }
                    Spacer(minLength:0)
                    moreOptions
                }
            } else {
                VStack(alignment:.leading,spacing:8) {
                    HStack(spacing:16) {
                        directionPicker.frame(maxWidth:440)
                        Spacer(minLength:0)
                        moreOptions
                    }
                    HStack(spacing:12) {
                        addButton
                        outputControl
                        if model.settings.direction == .toAndroid { vendorPicker.frame(maxWidth:160) }
                        Spacer(minLength:0)
                    }
                }
            }
        }
        .disabled(model.isBusy || model.sharing)
        .padding(.horizontal,layout.margin).padding(.vertical,8).recordLayout("command-bar")
    }

    private var directionPicker: some View {
        Picker("转换方向",selection:direction) {
            ForEach(ConversionDirection.allCases,id:\.self) { Text($0.title).tag($0) }
        }
        .pickerStyle(.segmented).labelsHidden()
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
                .help("转换所有已选择的来源；分类只筛选显示")
                .buttonStyle(.borderedProminent).tint(InterfaceStyle.accent).foregroundColor(InterfaceStyle.accentForeground)
                .disabled(!model.canStart || model.sharing)
                .recordLayout("primary-action").accessibilityIdentifier("convert-button")
        }
    }

    private var outputSummary: String {
        switch model.settings.destination {
        case .photos: return "系统照片图库"
        case .folderAndPhotos: return (model.output?.lastPathComponent ?? "待选择文件夹") + " ＋ 照片图库"
        case .share: return "转换后共享"
        case .folderAndShare: return "文件夹＋共享 · " + (model.output?.lastPathComponent ?? "待选择")
        case .folder: return model.output?.lastPathComponent ?? "选择文件夹…"
        }
    }

    private var showsTaskFeedback: Bool {
        model.isBusy || model.sharing || model.phase == .stopped || model.phase == .failed ||
            model.notice != nil || model.errorMessage?.isEmpty == false
    }

    private var statusLine: some View {
        VStack(alignment:.leading,spacing:3) {
            if model.isBusy || model.sharing || model.phase == .stopped || model.notice != nil ||
                (model.phase == .failed && model.errorMessage?.isEmpty != false) {
                HStack(spacing:8) {
                    Label(model.notice ?? (model.sharing ? "正在共享…" : model.status),systemImage:"info.circle")
                        .foregroundColor(.secondary).lineLimit(1)
                        .help(model.notice ?? model.status)
                    if model.isBusy, let progress = model.progress {
                        Text("\(Int((min(1,max(0,progress)) * 100).rounded()))%")
                            .monospacedDigit().foregroundColor(.secondary).fixedSize()
                    }
                }
            }
            if let error = model.errorMessage, !error.isEmpty {
                Label(error,systemImage:"exclamationmark.circle")
                    .foregroundColor(.red).lineLimit(1).help(error)
            }
        }.font(.callout).recordLayout("task-feedback")
    }

    @ViewBuilder private var secondaryActions: some View {
        if !model.isBusy {
            Button("最近结果…", action: actions.recentResults).disabled(model.sharing)
        }
        Button("打开输出文件夹", action: actions.openOutput)
            .disabled(!model.canOpenOutput).help(model.outputOpenHelp)
    }

    private func taskStatus(_ layout: WorkspaceLayout) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            if model.isBusy {
                if let progress = model.progress { ProgressView(value: progress).progressViewStyle(.linear) }
                else { ProgressView().progressViewStyle(.linear) }
            }
            if layout.stackedFooter {
                if showsTaskFeedback { statusLine }
                HStack(spacing: 8) {
                    if model.errorMessage != nil { Button("查看原因") { showingError = true } }
                    if model.report != nil { Button("报告", action: actions.showReport) }
                    if model.shareReady && !model.isBusy {
                        Button("共享结果", action: actions.shareResults).disabled(model.sharing)
                    }
                    Spacer(minLength: 0)
                    Menu("更多") { secondaryActions }.fixedSize()
                    primaryAction
                }
            } else {
                HStack(spacing: 8) {
                    if showsTaskFeedback { statusLine }
                    Spacer(minLength: 0)
                    if model.errorMessage != nil { Button("查看原因") { showingError = true } }
                    if model.report != nil { Button("报告", action: actions.showReport) }
                    if model.shareReady && !model.isBusy {
                        Button(action: actions.shareResults) { Image(systemName: "square.and.arrow.up") }
                            .disabled(model.sharing).help("共享结果").accessibilityLabel("共享结果")
                    }
                    secondaryActions
                    primaryAction
                }
            }
            if model.libraryMonitoring {
                HStack {
                    Text("正在监测图库").font(.caption).foregroundColor(.secondary)
                    Button("停止监测", action: actions.toggleLibraryMonitoring).font(.caption)
                }
            }
            if !layout.short && !model.deliverySummary.isEmpty {
                Text(model.deliverySummary).font(.caption).foregroundColor(.secondary).lineLimit(2).help(model.deliverySummary)
            } else if !layout.short && !model.librarySummary.isEmpty && !model.isBusy {
                Text(model.librarySummary).font(.caption).foregroundColor(.secondary).lineLimit(1).help(model.librarySummary)
            }
            if !model.currentFile.isEmpty {
                Text(model.currentFile).font(.callout).foregroundColor(.secondary).lineLimit(1).truncationMode(.middle)
            } else if let report = model.report, !layout.short {
                Text(report.summary).font(.callout).foregroundColor(.secondary).monospacedDigit()
            }
        }
        .padding(.horizontal, layout.margin).padding(.vertical, 10)
        .recordLayout("task-status")
        .accessibilityElement(children: .contain)
    }
}
