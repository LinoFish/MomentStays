import SwiftUI
import UniformTypeIdentifiers

struct WorkspaceView: View {
    @ObservedObject var model: WorkspaceModel
    let actions: WorkspaceActions
    var layoutObserver: ([String: CGRect]) -> Void = { _ in }
    @State private var outputDropTargeted = false
    @State private var showingError = false
    @FocusState private var settingsFocused: Bool

    var body: some View {
        GeometryReader { geometry in
            let layout = WorkspaceLayout(size: geometry.size)
            let showInspector = layout.supportsInspector && model.inspectorVisible && !model.settingsPresented
            VStack(spacing: 0) {
                commandBar(layout)
                Divider()
                sourceSummary(layout)
                Divider()
                HStack(spacing: 0) {
                    PhotoQueueView(model: model, actions: actions, compact: layout.compact, short: layout.short)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                    if showInspector {
                        Divider()
                        VStack(spacing: 0) {
                            HStack {
                                Text("导出设置").font(.system(size: 14, weight: .semibold))
                                Spacer()
                                Text(model.settings.direction == .toApple ? "Live Photo" : "Motion Photo")
                                    .font(.caption).foregroundColor(.secondary)
                            }.padding(16)
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
        HStack(spacing: 10) {
            Menu {
                Button("扫描系统照片图库…", action: actions.importPhotos)
            } label: { Label(layout.compact ? "选择照片" : "选择照片…", systemImage: "plus") }
            primaryAction: { actions.chooseSources() }
            .fixedSize().disabled(model.isBusy || model.sharing)
            .accessibilityIdentifier("import-button")
            if !layout.compact {
                Spacer(minLength: 8)
                Picker("转换方向", selection: direction) {
                    ForEach(ConversionDirection.allCases, id: \.self) { Text($0.title).tag($0) }
                }.pickerStyle(.segmented).labelsHidden().frame(maxWidth: 360)
                    .disabled(model.isBusy || model.sharing).accessibilityIdentifier("conversion-direction")
            }
            Spacer(minLength: 0)
            Button {
                if layout.supportsInspector { model.inspectorVisible.toggle() }
                else { model.settingsPresented = true }
            } label: { Image(systemName: "sidebar.right") }
            .focused($settingsFocused)
            .help("导出设置").accessibilityLabel("导出设置")
            .accessibilityIdentifier("settings-button")
            if model.isBusy {
                Button(action: model.stop) { Label(model.phase == .stopping ? "停止中" : "停止", systemImage: "stop.fill") }
                    .disabled(!model.canStop).recordLayout("primary-action")
                    .accessibilityIdentifier("stop-button")
            } else {
                Button(model.settings.actionTitle, action: actions.start)
                    .buttonStyle(.borderedProminent).disabled(!model.canStart || model.sharing)
                    .recordLayout("primary-action").accessibilityIdentifier("convert-button")
            }
        }
        .controlSize(.regular)
        .padding(.horizontal, layout.margin).padding(.vertical, 12)
        .recordLayout("command-bar")
    }

    private func sourceSummary(_ layout: WorkspaceLayout) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            if layout.compact {
                HStack {
                    Picker("方向", selection: direction) {
                        ForEach(ConversionDirection.allCases, id: \.self) { Text($0.title).tag($0) }
                    }.disabled(model.isBusy || model.sharing)
                    Spacer(minLength: 0)
                }
                sourceLine
                outputLine
            } else {
                HStack(spacing: 24) {
                    sourceLine.frame(maxWidth: .infinity, alignment: .leading)
                    outputLine.frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            if !layout.short && !layout.supportsInspector {
                Text(model.settings.summary).font(.callout).foregroundColor(.secondary)
                    .lineLimit(1).truncationMode(.middle)
            }
        }
        .padding(.horizontal, layout.margin).padding(.vertical, layout.short ? 10 : 12)
        .recordLayout("source-summary")
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
        HStack(spacing: 8) {
            Image(systemName: model.settings.destination.importsPhotos ? "photo.on.rectangle" : model.settings.destination.shares ? "square.and.arrow.up" : "folder")
                .foregroundColor(.secondary).accessibilityHidden(true)
            Text("输出").foregroundColor(.secondary)
            Text(outputSummary)
                .lineLimit(1).truncationMode(.middle)
                .help(model.settings.destination.title + "\n" + (model.output?.path ?? ""))
                .accessibilityIdentifier("output-summary")
            Spacer(minLength: 0)
        }
        .font(.callout).padding(2)
        .background(outputDropTargeted ? Color.accentColor.opacity(0.12) : .clear)
        .onDrop(of: [.fileURL], isTargeted: $outputDropTargeted) { providers in
            guard !model.isBusy, !model.sources.isEmpty, !model.settings.destination.usesCache else { return false }
            return actions.acceptDrop(providers, true)
        }
    }

    private var outputSummary: String {
        switch model.settings.destination {
        case .photos: return "相册 · " + model.settings.albumTitle
        case .folderAndPhotos: return "文件夹＋相册 · " + model.settings.albumTitle
        case .share: return "转换后共享"
        case .folderAndShare: return "文件夹＋共享 · " + (model.output?.lastPathComponent ?? "待选择")
        case .folder: return model.output?.lastPathComponent ?? "选择原片后自动建议"
        }
    }

    private func taskStatus(_ layout: WorkspaceLayout) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            if model.isBusy {
                if let progress = model.progress { ProgressView(value: progress).progressViewStyle(.linear) }
                else { ProgressView().progressViewStyle(.linear) }
            }
            HStack(spacing: 8) {
                Image(systemName: model.errorMessage != nil ? "exclamationmark.circle" : model.phase == .completed ? "checkmark.circle" : "info.circle")
                    .foregroundColor(model.errorMessage != nil ? .red : .secondary).accessibilityHidden(true)
                Text(model.status).font(.callout).lineLimit(1).truncationMode(.middle)
                    .help(model.errorMessage ?? model.report?.summary ?? model.status)
                Spacer(minLength: 0)
                if model.shareReady && !model.isBusy {
                    Button(action: actions.shareResults) { Image(systemName: "square.and.arrow.up") }
                        .disabled(model.sharing).help("共享完整输出文件").accessibilityLabel("共享完整输出文件")
                }
                if !model.isBusy {
                    Button(action: actions.recentResults) { Image(systemName: "clock.arrow.circlepath") }
                        .disabled(model.sharing).help("最近结果、重试交付与缓存").accessibilityLabel("最近结果与交付")
                }
                if model.errorMessage != nil {
                    Button("查看原因") { showingError = true }
                } else if model.report != nil {
                    Button("报告", action: actions.showReport)
                }
                if model.output != nil && !model.isPreview {
                    Button(action: actions.openOutput) { Image(systemName: "folder") }
                        .help("打开输出文件夹").accessibilityLabel("打开输出文件夹")
                }
            }
            if model.libraryMonitoring {
                HStack {
                    Text("正在监测所选图库范围").font(.caption).foregroundColor(.secondary)
                    Button("停止监测", action: actions.toggleLibraryMonitoring).font(.caption)
                }
            }
            if !model.deliverySummary.isEmpty {
                Text(model.deliverySummary).font(.caption).foregroundColor(.secondary).lineLimit(2).help(model.deliverySummary)
            } else if !model.librarySummary.isEmpty && !model.isBusy {
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
