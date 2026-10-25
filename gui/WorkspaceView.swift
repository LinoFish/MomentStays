import SwiftUI
import UniformTypeIdentifiers

struct WorkspaceView: View {
    @Environment(\.locale) private var locale
    @ObservedObject var model: WorkspaceModel
    let actions: WorkspaceActions
    var layoutObserver: ([String: CGRect]) -> Void = { _ in }
    @State private var outputDropTargeted = false
    @State private var queueScope = PhotoQueueScope.all
    @State private var showingError = false
    @FocusState private var settingsFocused: Bool

    var body: some View {
        GeometryReader { geometry in
            let layout = WorkspaceLayout(size: geometry.size)
            let showInspector = layout.supportsInspector && model.inspectorVisible && !model.settingsPresented
            VStack(spacing: 0) {
                commandBar(layout)
                Divider()
                HStack(spacing: 0) {
                    if layout.supportsSidebar {
                        workspaceSidebar
                            .frame(width: layout.sidebarWidth)
                        Divider()
                    }
                    VStack(spacing: 0) {
                        workspaceHeader(layout)
                        sourceSummary(layout, showInspector: showInspector)
                        PhotoQueueView(model: model, actions: actions, short: layout.short, scope: $queueScope, showsScopePicker: !layout.supportsSidebar)
                            .frame(maxWidth: .infinity, maxHeight: .infinity)
                    }
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    if showInspector {
                        Divider()
                        VStack(spacing: 0) {
                            HStack(alignment: .firstTextBaseline) {
                                VStack(alignment: .leading, spacing: 4) {
                                    Text("交付设置").font(.headline)
                                    Text(model.settings.direction == .toApple ? "Live Photo" : "Motion Photo")
                                        .font(.title3.weight(.semibold))
                                }
                                Spacer()
                            }.padding(20)
                            Divider()
                            ExportSettingsView(model: model, actions: actions)
                        }
                        .frame(width: layout.inspectorWidth)
                        .recordLayout("inspector")
                    }
                }.frame(maxWidth: .infinity, maxHeight: .infinity)
                Divider()
                taskStatus(layout)
            }
            .background(InterfaceStyle.canvas)
            .coordinateSpace(name: "workspace")
            .recordLayout("workspace")
            .onPreferenceChange(LayoutFramesKey.self, perform: layoutObserver)
            .sheet(isPresented: $model.settingsPresented, onDismiss: { settingsFocused = true }) {
                ExportSettingsSheet(model: model, availableSize: geometry.size, actions: actions)
            }
        }
        .font(.body)
        .alert("无法完成操作", isPresented: $showingError) {
            Button("好", role: .cancel) {}
        } message: { Text(model.errorMessage ?? "") }
    }

    private var direction: Binding<ConversionDirection> {
        Binding(get: { model.settings.direction }, set: { value in model.updateSettings { $0.direction = value } })
    }

    private func commandBar(_ layout: WorkspaceLayout) -> some View {
        HStack(spacing: 12) {
            MomentStaysBrandIcon()
                .frame(width: 32, height: 32)
                .accessibilityHidden(true)
            Text("MomentStays").font(.system(size: 15, weight: .semibold))
            Spacer(minLength: 0)
            if !layout.compact {
                Text("留住原来的瞬间").font(.callout).foregroundColor(.secondary)
            }
            Link(destination: UserGuide.url(for: locale)) {
                Image(systemName: "questionmark.circle")
            }
            .help(UserGuide.title(for: locale))
            .accessibilityLabel(UserGuide.title(for: locale))
            .accessibilityIdentifier("user-guide-button")
            Button {
                if layout.supportsInspector { model.inspectorVisible.toggle() }
                else { model.settingsPresented = true }
            } label: { Image(systemName: "sidebar.right") }
            .focused($settingsFocused)
            .help("交付设置").accessibilityLabel("交付设置")
            .accessibilityIdentifier("settings-button")
        }
        .buttonStyle(.plain)
        .padding(.horizontal, layout.margin).padding(.vertical, 12)
        .recordLayout("command-bar")
    }

    private var workspaceSidebar: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("工作空间").font(.caption).foregroundColor(.secondary)
                .padding(.horizontal, 12).padding(.bottom, 10)
            ForEach(PhotoQueueScope.allCases) { scope in
                Button { queueScope = scope } label: {
                    HStack(spacing: 9) {
                        Image(systemName: scope.symbol).frame(width: 18)
                        Text(scope.title)
                        Spacer(minLength: 0)
                        Text("\(model.items.filter { scope.includes($0) }.count)")
                            .font(.caption).monospacedDigit().foregroundColor(.secondary)
                    }
                    .font(.callout.weight(queueScope == scope ? .semibold : .regular))
                    .padding(.horizontal, 12).padding(.vertical, 11)
                    .contentShape(Rectangle())
                    .background(queueScope == scope ? InterfaceStyle.accent.opacity(0.11) : .clear)
                    .cornerRadius(8)
                }
                .buttonStyle(.plain)
                .accessibilityAddTraits(queueScope == scope ? .isSelected : [])
                .accessibilityIdentifier("queue-scope-" + scope.rawValue)
            }
            Spacer(minLength: 20)
            Button(action: actions.recentResults) {
                Label("最近结果与交付", systemImage: "clock.arrow.circlepath")
                    .font(.callout)
            }
            .buttonStyle(.plain).disabled(model.isBusy || model.sharing)
            .padding(.horizontal, 12).padding(.bottom, 18)
            Label("本机处理", systemImage: "lock.shield")
                .font(.caption).foregroundColor(.secondary).padding(.horizontal, 12)
            Text("照片始终由你掌握")
                .font(.caption).foregroundColor(.secondary).padding(.horizontal, 12)
        }
        .padding(.horizontal, 12).padding(.vertical, 24)
        .background(InterfaceStyle.canvas)
        .recordLayout("workspace-sidebar")
    }

    private func workspaceHeader(_ layout: WorkspaceLayout) -> some View {
        VStack(alignment: .leading, spacing: layout.short ? 10 : 18) {
            HStack(alignment: .center, spacing: 12) {
                VStack(alignment: .leading, spacing: 4) {
                    Text("让瞬间继续。")
                        .font(.system(size: layout.short ? 19 : 26, weight: .semibold))
                    if !layout.short && !layout.compact {
                        Text("把动态照片带进新的图库。")
                            .font(.callout).foregroundColor(.secondary)
                    }
                }
                Spacer(minLength: 0)
                Menu {
                    Button("扫描系统照片图库…", action: actions.importPhotos)
                } label: { Label("添加照片", systemImage: "plus") }
                primaryAction: { actions.chooseSources() }
                .fixedSize().disabled(model.isBusy || model.sharing)
                .accessibilityIdentifier("import-button")
            }
            Picker("转换方向", selection: direction) {
                ForEach(ConversionDirection.allCases, id: \.self) { Text($0.title).tag($0) }
            }
            .pickerStyle(.segmented).labelsHidden()
            .disabled(model.isBusy || model.sharing)
            .accessibilityIdentifier("conversion-direction")
        }
        .padding(.horizontal, layout.margin).padding(.top, layout.short ? 12 : 22)
        .padding(.bottom, layout.short ? 4 : 8)
        .background(InterfaceStyle.content)
    }

    private func sourceSummary(_ layout: WorkspaceLayout, showInspector: Bool) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            sourceLine
            if !showInspector { outputLine }
        }
        .padding(.horizontal, layout.margin).padding(.vertical, layout.short ? 6 : 10)
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
            Button(queueScope == .all ? model.settings.actionTitle : "转换全部来源", action: actions.start)
                .help("转换所有已选择的来源；侧栏范围只筛选显示")
                .buttonStyle(.borderedProminent).tint(InterfaceStyle.accent)
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
        .background(outputDropTargeted ? Color.accentColor.opacity(0.12) : .clear)
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
