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
                sourceSummary(layout)
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
        .alert("无法完成操作", isPresented: $showingError) {
            Button("好", role: .cancel) {}
        } message: { Text(model.errorMessage ?? "") }
    }

    private var direction: Binding<ConversionDirection> {
        Binding(get: { model.settings.direction }, set: { value in model.updateSettings { $0.direction = value } })
    }

    private var addButton: some View {
        SourceMenuButton(requested:$model.sourceMenuRequested,disabled:model.isBusy || model.sharing,
            addFiles:actions.chooseSources,addLibrary:actions.importPhotos)
            .fixedSize().recordLayout("add-button")
    }

    private var destination: Binding<OutputDestination> {
        Binding(get:{ model.settings.destination },set:{ value in model.updateSettings { $0.destination = value } })
    }
    private var vendor: Binding<AndroidVendor> {
        Binding(get:{ model.settings.vendor },set:{ value in model.updateSettings { $0.vendor = value } })
    }
    private var destinationPicker: some View {
        Picker("保存到",selection:destination) {
            ForEach(model.settings.destinations,id:\.self) { Text($0.title).tag($0) }
        }.frame(maxWidth:290)
    }
    private var vendorPicker: some View {
        Picker("目标手机",selection:vendor) {
            ForEach(AndroidVendor.allCases,id:\.self) { Text($0.title).tag($0) }
        }.frame(maxWidth:220)
    }
    private var moreOptions: some View {
        HStack(spacing:12) {
            Button("更多选项…") { model.settingsPresented = true }
                .focused($settingsFocused).accessibilityIdentifier("settings-button").recordLayout("settings-button")
            Link(destination:UserGuide.url(for:locale)) { Image(systemName:"questionmark.circle").foregroundColor(InterfaceStyle.accent) }
                .help(UserGuide.title(for:locale)).accessibilityIdentifier("user-guide-button")
        }
    }
    private func workspaceHeader(_ layout: WorkspaceLayout) -> some View {
        VStack(alignment:.leading,spacing:layout.short ? 6 : 10) {
            Picker("转换方向",selection:direction) {
                ForEach(ConversionDirection.allCases,id:\.self) { Text($0.title).tag($0) }
            }.pickerStyle(.segmented).labelsHidden().accessibilityIdentifier("conversion-direction").recordLayout("conversion-directions")
            if layout.size.width < (model.settings.direction == .toAndroid ? 960 : 760) {
                HStack { addButton; Spacer(); moreOptions }
                destinationPicker
                if model.settings.direction == .toAndroid { vendorPicker }
            } else {
                HStack(spacing:14) {
                    addButton
                    destinationPicker
                    if model.settings.direction == .toAndroid { vendorPicker }
                    Spacer(minLength:0)
                    moreOptions
                }
            }
        }
        .disabled(model.isBusy || model.sharing)
        .padding(.horizontal,layout.margin).padding(.vertical,layout.short ? 8 : 12).recordLayout("command-bar")
    }

    private func sourceSummary(_ layout: WorkspaceLayout) -> some View {
        VStack(alignment: .leading, spacing: layout.short ? 4 : 6) {
            sourceLine
            if model.settings.destination.usesCache { outputLine }
            else {
                HStack {
                    outputLine
                    Button("更改…",action:actions.chooseOutput).disabled(model.isBusy || model.sharing || model.sources.isEmpty)
                }
            }
        }
        .padding(.horizontal, layout.margin).padding(.vertical, layout.short ? 4 : 10)
        .background(InterfaceStyle.content)
        .recordLayout("source-summary")
    }

    @ViewBuilder private var primaryAction: some View {
        if model.isBusy {
            Button(action: model.stop) {
                Label(model.phase == .stopping ? "停止中" : "停止", systemImage: "stop.fill")
            }
            .disabled(!model.canStop).recordLayout("primary-action")
            .accessibilityIdentifier("stop-button")
        } else {
            Button(queueScope == .all || queueScope == .ready ? model.settings.actionTitle : "转换全部来源", action: actions.start)
                .help("转换所有已选择的来源；分类只筛选显示")
                .buttonStyle(.borderedProminent).tint(InterfaceStyle.accent).foregroundColor(InterfaceStyle.accentForeground)
                .disabled(!model.canStart || model.sharing)
                .recordLayout("primary-action").accessibilityIdentifier("convert-button")
        }
    }

    private var sourceLine: some View {
        HStack(spacing: 8) {
            Image(systemName: "photo.on.rectangle").foregroundColor(.secondary).accessibilityHidden(true)
            Text("来源").foregroundColor(.secondary)
            Text(model.sourceLabel).lineLimit(1).truncationMode(.middle)
                .help(model.sources.map(\.path).joined(separator: "\n"))
            Spacer(minLength: 0)
        }.font(.callout)
    }

    private var outputLine: some View {
        Button { model.settingsPresented = true } label: {
            HStack(spacing: 8) {
                Image(systemName: model.settings.destination.importsPhotos ? "photo.on.rectangle" : model.settings.destination.shares ? "square.and.arrow.up" : "folder")
                    .foregroundColor(.secondary).accessibilityHidden(true)
                Text("输出").foregroundColor(.secondary)
                Text(outputSummary)
                    .lineLimit(1).truncationMode(.middle)
                    .help(outputSummary + "\n" + (model.output?.path ?? "") + "\n点击打开导出设置")
                    .accessibilityIdentifier("output-summary")
                Spacer(minLength: 0)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel("输出：" + outputSummary + "，打开导出设置")
        .font(.callout).padding(2)
        .background(outputDropTargeted ? InterfaceStyle.accent.opacity(0.12) : .clear)
        .onDrop(of: [.fileURL], isTargeted: $outputDropTargeted) { providers in
            guard !model.isBusy, !model.sharing, !model.sources.isEmpty, !model.settings.destination.usesCache else { return false }
            return actions.acceptDrop(providers, true)
        }
    }

    private var outputSummary: String {
        switch model.settings.destination {
        case .photos: return "系统照片图库"
        case .folderAndPhotos: return "文件夹：" + (model.output?.lastPathComponent ?? "待选择") + " · 系统照片图库"
        case .share: return "转换后共享"
        case .folderAndShare: return "文件夹＋共享 · " + (model.output?.lastPathComponent ?? "待选择")
        case .folder: return model.output?.lastPathComponent ?? "选择原片后自动建议"
        }
    }

    private var statusLine: some View {
        VStack(alignment: .leading, spacing: 3) {
            Label(model.status, systemImage: model.phase == .failed ? "exclamationmark.circle" : model.phase == .completed ? "checkmark.circle" : "info.circle")
                .foregroundColor(model.phase == .failed ? .red : .secondary)
                .lineLimit(1).help(model.status)
            if let error = model.errorMessage, !error.isEmpty {
                Label((model.phase == .failed ? "任务错误：" : "操作未完成：") + error, systemImage: "exclamationmark.circle")
                    .foregroundColor(.red).lineLimit(1).help(error)
            }
        }.font(.callout)
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
                statusLine
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
                    statusLine
                    Spacer(minLength: 0)
                    if model.errorMessage != nil { Button("查看原因") { showingError = true } }
                    if model.report != nil { Button("报告", action: actions.showReport) }
                    if model.shareReady && !model.isBusy {
                        Button(action: actions.shareResults) { Image(systemName: "square.and.arrow.up") }
                            .disabled(model.sharing).help("共享完整输出文件").accessibilityLabel("共享完整输出文件")
                    }
                    secondaryActions
                    primaryAction
                }
            }
            if model.libraryMonitoring {
                HStack {
                    Text("正在监测所选图库范围").font(.caption).foregroundColor(.secondary)
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
