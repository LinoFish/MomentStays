import SwiftUI
import UniformTypeIdentifiers

struct WorkspaceView: View {
    @Environment(\.locale) private var locale
    @ObservedObject var model: WorkspaceModel
    let actions: WorkspaceActions
    var layoutObserver: ([String: CGRect]) -> Void = { _ in }
    @State private var showingError = false
    @State private var showingOutputChoices = false
    @State private var furthestStep: WorkspaceStep = .direction

    private var queueScope: Binding<PhotoQueueScope> {
        Binding(get:{ .all },
                set:{ model.selectQueueScope($0) })
    }
    private var canContinueImport: Bool {
        model.migrationTargetConfirmed && !model.isBusy && !model.sharing && !model.items.isEmpty &&
            (!model.hasExplicitSelection || !model.selectedIDs.isEmpty)
    }
    private var executionActive: Bool {
        let phases: [TaskPhase] = [.converting,.awaitingDecision,.delivering,.stopping]
        return phases.contains(model.phase) || model.sharing
    }
    private var queueIsVisible: Bool {
        model.workspaceStep != .direction
    }

    var body: some View {
        GeometryReader { geometry in
            let layout = WorkspaceLayout(size:geometry.size)
            HStack(spacing:0) {
                workspaceCanvas(layout)
                    .frame(maxWidth:.infinity,maxHeight:.infinity)
                Divider()
                workflowSidebar(compact:geometry.size.height < 800)
                    .frame(width:geometry.size.width < 900 ? 264 : 300)
                    .frame(maxHeight:.infinity)
            }
            .padding(.top,28).background(InterfaceStyle.canvas)
            .coordinateSpace(name:"workspace").recordLayout("workspace")
            .onPreferenceChange(LayoutFramesKey.self,perform:layoutObserver)
        }
        .ignoresSafeArea(.container,edges:.top).font(.body)
        .tint(InterfaceStyle.accent).accentColor(InterfaceStyle.accent)
        .onAppear {
            #if UI_DIAGNOSTICS
            if let index = CommandLine.arguments.firstIndex(of:"--ui-scope"), index + 1 < CommandLine.arguments.count,
               let value = PhotoQueueScope(rawValue:CommandLine.arguments[index + 1]) { model.selectQueueScope(value) }
            #endif
        }
        .onChange(of:model.workspaceStep) { step in
            if step.rawValue > furthestStep.rawValue { furthestStep = step }
        }
        .onChange(of:model.phase) { phase in
            if phase == .converting { model.workspaceStep = .results }
        }
        .onChange(of:model.sources.isEmpty) { empty in
            if empty && !executionActive { model.workspaceStep = model.migrationTargetConfirmed ? .importPhotos : .direction; furthestStep = model.workspaceStep }
        }
        .onChange(of:model.settingsPresented) { presented in
            if presented {
                model.settingsPresented = false
                model.workspaceStep = !model.migrationTargetConfirmed ? .direction : model.items.isEmpty ? .importPhotos : .conversion
            }
        }
        .onChange(of:model.migrationReviewPresented) { presented in
            if presented {
                model.migrationReviewPresented = false
                model.workspaceStep = model.canReviewMigration ? .preview : model.migrationTargetConfirmed ? .importPhotos : .direction
            }
        }
        .alert(AppText.text("无法完成操作"),isPresented:$showingError) { Button(AppText.text("好"),role:.cancel) {} }
            message: { Text(AppText.text(model.errorMessage ?? "")) }
    }

    private func workspaceCanvas(_ layout: WorkspaceLayout) -> some View {
        VStack(spacing:0) {
            HStack(alignment:.center,spacing:12) {
                HStack(alignment:.firstTextBaseline,spacing:8) {
                    Text(AppText.text(model.workspaceStep.title)).font(.headline).fixedSize()
                    Text(AppText.text(model.workspaceStep.detail)).font(.caption).foregroundColor(.secondary)
                        .lineLimit(1).truncationMode(.tail).help(AppText.text(model.workspaceStep.detail))
                }
                Spacer(minLength:0)
                resultMenu
            }.padding(.horizontal,12).padding(.vertical,6)
            Divider()
            // Keep the browser mounted across steps so marquee, preview and scroll state survive.
            ZStack {
                PhotoQueueView(model:model,actions:actions,short:layout.short,scope:queueScope,
                               showsProcessingScopes:false,selectionEnabled:queueIsVisible && !executionActive)
                    .environment(\.workspaceTaskFeedback,AnyView(taskFeedback))
                    .opacity(queueIsVisible ? 1 : 0)
                    .allowsHitTesting(queueIsVisible)
                    .accessibilityHidden(!queueIsVisible)
                if model.workspaceStep == .direction { directionCanvas }
            }.frame(maxWidth:.infinity,maxHeight:.infinity)
        }
    }
    private var taskFeedback: some View {
        WorkspaceTaskFeedback(model:model,actions:actions,showError:{ showingError = true })
    }

    private func workflowSidebar(compact: Bool) -> some View {
        VStack(spacing:0) {
            VStack(alignment:.leading,spacing:10) {
                HStack {
                    Text(AppText.text("操作步骤")).font(.headline)
                    Spacer(minLength:0)
                    Text(AppText.text("第 \(model.workspaceStep.rawValue) 步 / 共 \(WorkspaceStep.allCases.count) 步"))
                        .font(.caption).foregroundColor(.secondary)
                }
                VStack(spacing:3) {
                    ForEach(WorkspaceStep.allCases) { step in workflowStep(step,compact:compact) }
                }.accessibilityElement(children:.contain)
                    .accessibilityLabel(AppText.text("完整操作流程，共五步"))
                Divider()
                HStack(spacing:8) {
                    Button(AppText.text("返回")) {
                        if let previous = WorkspaceStep(rawValue:model.workspaceStep.rawValue - 1) {
                            model.workspaceStep = previous
                        }
                    }.disabled(model.workspaceStep == .direction)
                    primaryAction.frame(maxWidth:.infinity)
                }.controlSize(.large).recordLayout("command-bar")
                Text(AppText.text(primaryActionHint)).font(.caption).foregroundColor(.secondary)
                    .fixedSize(horizontal:false,vertical:true)
                    .frame(maxWidth:.infinity,minHeight:28,alignment:.leading)
            }.padding(12)
            Divider()
            Group {
                switch model.workspaceStep {
                case .direction:
                    directionSummary
                case .importPhotos:
                    sourceSidebar
                case .conversion:
                    ExportSettingsView(model:model,actions:actions,section:.conversion)
                case .preview:
                    ExportSettingsView(model:model,actions:actions,section:.delivery)
                case .results:
                    resultSidebar
                }
            }
            .frame(maxWidth:.infinity,maxHeight:.infinity,alignment:.topLeading)
            .disabled(executionActive)
        }.background(InterfaceStyle.content).recordLayout("workflow-sidebar")
    }
    private func workflowStep(_ step: WorkspaceStep, compact: Bool) -> some View {
        let current = model.workspaceStep == step
        let available = canVisit(step)
        return Button {
            guard available else { return }
            model.workspaceStep = step
        } label: {
            HStack(alignment:.top,spacing:9) {
                Text(AppText.text("\(step.rawValue)")).font(.caption.weight(.semibold))
                    .frame(width:24,height:24)
                    .background(current ? InterfaceStyle.accent : InterfaceStyle.canvas)
                    .foregroundColor(current ? InterfaceStyle.accentForeground : .secondary)
                    .clipShape(Circle())
                VStack(alignment:.leading,spacing:2) {
                    HStack {
                        Text(AppText.text(step.title)).font(.callout.weight(current ? .semibold : .regular))
                        Spacer(minLength:0)
                        Text(AppText.text(current ? "当前" : available ? "可查看" : "待进行"))
                            .font(.caption2).foregroundColor(.secondary)
                    }
                }
            }.padding(.horizontal,8).padding(.vertical,5).frame(maxWidth:.infinity,alignment:.leading)
                .background(current ? InterfaceStyle.selection : .clear)
                .cornerRadius(InterfaceStyle.controlRadius)
                .contentShape(Rectangle())
        }.buttonStyle(WorkflowStepButtonStyle()).disabled(!available)
            // Future steps stay legible, and explain their prerequisites instead of disappearing.
            .help(AppText.text(available ? "查看\(step.title)，保留当前选择和设置" : "完成前面的步骤后进入\(step.title)"))
            .accessibilityLabel(AppText.text("第 \(step.rawValue) 步，\(step.title)"))
            .accessibilityHint(AppText.text(step.detail))
            .accessibilityValue(AppText.text(current ? "当前步骤" : available ? "可查看" : "完成前面的步骤后进入"))
            .accessibilityAddTraits(current ? .isSelected : [])
            .accessibilityIdentifier("workflow-step-\(step.rawValue)")
            .recordLayout("workflow-step-\(step.rawValue)")
    }
    private func canVisit(_ step: WorkspaceStep) -> Bool {
        switch step {
        case .direction: return true
        case .importPhotos: return model.migrationTargetConfirmed
        case .conversion: return model.migrationTargetConfirmed && (furthestStep.rawValue >= step.rawValue || canContinueImport)
        case .preview: return model.migrationTargetConfirmed && (furthestStep.rawValue >= step.rawValue || model.canReviewMigration)
        case .results: return furthestStep == .results || executionActive || model.report != nil
        }
    }
    private var directionCanvas: some View {
        ScrollView {
            VStack(alignment:.leading,spacing:22) {
                Text(AppText.text("转换会修改输出副本的格式或元数据；建议保留原图存档备份。源文件保持只读。"))
                    .font(.callout).foregroundColor(.secondary)
                Text(AppText.text("仅支持下列已验证路线。实况、HDR、连拍的实际可执行范围以扫描和预览为准；私有 HDR 为实验功能，默认关闭，不支持任意安卓互转。"))
                    .font(.caption).foregroundColor(.secondary)
                directionGroup("安卓到苹果",presets:[.apple])
                directionGroup("苹果到安卓",presets:[.google,.xiaomi,.samsung,.huawei,.honor,.oppo,.vivo])
                directionGroup("安卓到安卓",presets:[.googleToHonor,.honorToGoogle])
            }.padding(16).frame(maxWidth:.infinity,alignment:.leading)
        }.background(InterfaceStyle.gallery).recordLayout("direction-canvas")
    }
    private func directionGroup(_ title: String, presets: [MigrationTargetPreset]) -> some View {
        VStack(alignment:.leading,spacing:10) {
            Text(AppText.text(title)).font(.headline)
            LazyVGrid(columns:[GridItem(.adaptive(minimum:210),spacing:12)],alignment:.leading,spacing:12) {
                ForEach(presets,id:\.self) { preset in directionCard(preset) }
            }
        }
    }
    private func directionCard(_ preset: MigrationTargetPreset) -> some View {
        let selected = model.migrationTargetConfirmed && model.migrationTarget == preset
        return Button { model.selectMigrationTarget(preset); model.workspaceStep = .importPhotos } label: {
            HStack(alignment:.top,spacing:10) {
                Image(systemName:preset == .apple ? "photo.on.rectangle" : "iphone")
                    .font(.title3).foregroundColor(InterfaceStyle.accent).frame(width:24)
                VStack(alignment:.leading,spacing:5) {
                    Text(AppText.text(directionCardTitle(preset))).font(.callout.weight(.semibold)).foregroundColor(.primary)
                    Text(AppText.text(directionSupportSummary(preset)))
                        .font(.caption).foregroundColor(.secondary).fixedSize(horizontal:false,vertical:true)
                }.frame(maxWidth:.infinity,alignment:.leading)
                Image(systemName:"chevron.right").foregroundColor(.secondary)
            }.padding(14).frame(maxWidth:.infinity,minHeight:76,alignment:.topLeading)
                .background(selected ? InterfaceStyle.selection : InterfaceStyle.content)
                .cornerRadius(InterfaceStyle.photoRadius)
                .overlay(RoundedRectangle(cornerRadius:InterfaceStyle.photoRadius)
                    .stroke(selected ? InterfaceStyle.accent : InterfaceStyle.separator,lineWidth:selected ? 2 : 0.5))
                .contentShape(Rectangle())
        }.buttonStyle(.plain).disabled(!model.canSwitchDirection || executionActive)
            .help(AppText.text(preset.detail)).accessibilityLabel(AppText.text(preset.title))
            .accessibilityAddTraits(selected ? .isSelected : [])
            .accessibilityIdentifier("direction-card-" + preset.rawValue)
            .recordLayout("direction-card-" + preset.rawValue)
    }
    private func directionSupportSummary(_ preset: MigrationTargetPreset) -> String {
        if preset == .apple { return "实况；本地静态 HDR；荣耀/华为 JPEG 连拍。Google/三星连拍为实验支持。私有 HDR 仅部分配置。" }
        if preset == .googleToHonor { return "仅 Google 实况 JPEG → 荣耀；不支持 HEIC、相册来源或连拍。" }
        if preset == .honorToGoogle { return "仅已识别荣耀实况 JPEG → Google；不支持 HEIC、相册来源或连拍。" }
        return "Apple 实况 · " + (preset.vendor.supportsHEIC ? "JPEG / HEIC" : "JPEG") + "；静态 HDR 仅本地已支持类型；不支持连拍转换。"
    }
    private func directionCardTitle(_ preset: MigrationTargetPreset) -> String {
        if preset == .apple { return "Apple 照片" }
        if preset == .googleToHonor { return "Google → 荣耀" }
        if preset == .honorToGoogle { return "荣耀 → Google" }
        if preset == .google { return "Google / 通用" }
        return preset.vendor.title
    }
    private var directionSummary: some View {
        ScrollView {
            VStack(alignment:.leading,spacing:12) {
                Text(AppText.text(model.migrationTargetConfirmed ? "已选方向" : "先选择转换方向")).font(.headline)
                if model.migrationTargetConfirmed {
                    Text(AppText.text(model.migrationTarget.title)).font(.callout.weight(.medium))
                    Text(AppText.text(model.importScopeDescription)).font(.callout).foregroundColor(.secondary)
                } else {
                    Text(AppText.text("在左侧选择来源与目标。导入入口会按此方向开放，照片特性在导入后单独选择。"))
                        .font(.callout).foregroundColor(.secondary)
                }
                if !model.sources.isEmpty {
                    Text(AppText.text("更改方向后复用有效解析，重新检查适用范围；保留来源和勾选，不兼容项会说明原因。"))
                        .font(.caption).foregroundColor(.secondary)
                }
            }.padding(12).frame(maxWidth:.infinity,alignment:.leading)
        }
    }
    private var resultSidebar: some View {
        ScrollView {
            VStack(alignment:.leading,spacing:12) {
                Text(AppText.text(executionActive ? "正在执行" : "本轮结果")).font(.headline)
                Text(AppText.text(model.status)).font(.callout).fixedSize(horizontal:false,vertical:true)
                if let report = model.report {
                    Text(AppText.text(report.summary)).font(.caption).foregroundColor(.secondary)
                }
                if !model.deliverySummary.isEmpty {
                    Text(AppText.text(model.deliverySummary)).font(.caption).foregroundColor(.secondary)
                }
                if model.canOpenOutput { Button(AppText.text("打开输出文件夹"),action:actions.openOutput) }
                if model.report != nil { Button(AppText.text("查看完整报告"),action:actions.showReport) }
                Text(AppText.text("文件生成、原样输出与图库交付分别记录；可以从结果菜单重试交付。"))
                    .font(.caption).foregroundColor(.secondary)
            }.frame(maxWidth:.infinity,alignment:.leading).padding(12)
        }
    }
    @ViewBuilder private var primaryAction: some View {
        if model.isBusy || model.sharing {
            Button(action:model.stop) {
                Label(AppText.text(model.phase == .stopping ? "停止中" : "停止"),systemImage:"stop.fill")
                    .frame(maxWidth:.infinity)
            }.disabled(!model.canStop).recordLayout("primary-action").accessibilityIdentifier("stop-button")
        } else {
            Button(action:advance) {
                Text(AppText.text(primaryActionTitle)).lineLimit(1).frame(maxWidth:.infinity)
            }.buttonStyle(.borderedProminent).tint(InterfaceStyle.accent)
                .foregroundColor(InterfaceStyle.accentForeground)
                .disabled(!canAdvance).recordLayout("primary-action")
                .accessibilityIdentifier(model.workspaceStep == .preview ? "convert-button" : "workflow-next")
                .popover(isPresented:$showingOutputChoices,arrowEdge:.trailing) { outputLocationChoices }
        }
    }
    private var outputLocationChoices: some View {
        let allowsPhotos = model.settings.direction == .toApple && model.settings.exportMode != .auxiliaryOnly
        return VStack(alignment:.leading,spacing:4) {
            outputChoice("相册",symbol:"photo.on.rectangle",destination:.photos).disabled(!allowsPhotos)
            outputChoice("文件夹",symbol:"folder",destination:.folder)
            outputChoice("相册和文件夹",symbol:"square.stack",destination:.folderAndPhotos).disabled(!allowsPhotos)
            if !allowsPhotos {
                Text(AppText.text("当前方向或仅辅助视频模式只支持文件夹输出")).font(.caption).foregroundColor(.secondary)
            }
        }.padding(8).frame(width:240)
    }
    private func outputChoice(_ title: String, symbol: String, destination: OutputDestination) -> some View {
        Button {
            showingOutputChoices = false
            DispatchQueue.main.asyncAfter(deadline:.now()+0.15) {
                if destination == .folder && model.settings.direction != .toApple { actions.chooseOutput() }
                else { actions.chooseDestination(destination) }
            }
        } label: {
            Label(AppText.text(title),systemImage:symbol).frame(maxWidth:.infinity,alignment:.leading)
                .padding(.horizontal,8).padding(.vertical,7).contentShape(Rectangle())
        }.buttonStyle(.plain)
    }
    private var canAdvance: Bool {
        switch model.workspaceStep {
        case .direction: return model.migrationTargetConfirmed && !executionActive
        case .importPhotos: return canContinueImport
        case .conversion: return model.canReviewMigration
        case .preview: return !model.outputIsChosen ? model.canReviewMigration : model.canApproveMigration
        case .results: return !model.isBusy && !model.sharing
        }
    }
    private func advance() {
        guard canAdvance else { return }
        switch model.workspaceStep {
        case .direction: model.workspaceStep = .importPhotos
        case .importPhotos: model.workspaceStep = .conversion
        case .conversion: model.workspaceStep = .preview
        case .preview:
            if !model.outputIsChosen { showingOutputChoices = true; return }
            model.approveMigrationReview()
            if model.reviewedMigration != nil { model.workspaceStep = .results }
        case .results: model.workspaceStep = .importPhotos
        }
    }
    private var primaryActionTitle: String {
        switch model.workspaceStep {
        case .direction: return "导入照片"
        case .importPhotos: return "选择转换特性"
        case .conversion: return "预览导出"
        case .preview:
            if !model.outputIsChosen { return "选择输出位置…" }
            if model.migrationReadyItems.isEmpty && model.unconvertedExport != .none {
                return "原样导出 \(model.migrationUnconvertedItems.count) 张"
            }
            return "开始执行 \(model.migrationReadyItems.count) 张"
        case .results: return "返回照片"
        }
    }
    private var primaryActionHint: String {
        if executionActive { return model.phase == .stopping ? "等待安全停止" : "任务已冻结，可只读浏览前面的步骤" }
        if model.hasExplicitSelection && model.selectedIDs.isEmpty { return "已选 0 张，请重新勾选照片" }
        switch model.workspaceStep {
        case .direction: return model.migrationTargetConfirmed ? "方向已选，下一步导入照片" : "请选择左侧的方向卡片"
        case .importPhotos:
            if model.isBusy { return "正在识别来源，请稍候" }
            return model.items.isEmpty ? "添加照片后选择本轮范围" : "本轮 \(model.migrationScopeItems.count) 张，筛选不改变选择"
        case .conversion:
            return model.isTimeOnlyWorkspace ? "仅补缺照片时间；原照片格式保持不变" : "只选择本轮处理特性，转换方向在第一步更改"
        case .preview:
            if !model.outputIsChosen { return "请选择本轮输出位置" }
            if model.unconvertedExport == .separateLocation && model.unconvertedOutput == nil { return "请选择原样导出目录" }
            return "按本轮就绪范围执行，预估不代表实际成功"
        case .results: return "保留本轮结果，可返回查看照片"
        }
    }
    private var addButton: some View {
        SourceMenuButton(requested:$model.sourceMenuRequested,disabled:!model.canImportSources,
            title:model.sources.isEmpty ? "添加照片" : "添加来源…",details:model.sources.map(\.path).joined(separator:"\n"),
            hasSources:!model.sources.isEmpty,prominentWhenEmpty:true,allowsLibrary:model.canImportFromLibrary,
            addFiles:actions.chooseSources,addLibrary:actions.importPhotos)
            .frame(maxWidth:.infinity).frame(height:24).recordLayout("add-button")
    }
    private var resultMenu: some View {
        Menu {
            Button(AppText.text("导出结果到其他位置…"),action:actions.exportResults)
                .disabled(!model.resultExportAvailable || model.isBusy || model.sharing)
                .accessibilityIdentifier("export-results-button")
            if model.shareReady { Button(AppText.text("共享完整结果…"),action:actions.shareResults).disabled(model.isBusy || model.sharing) }
            Button(AppText.text("打开输出文件夹"),action:actions.openOutput).disabled(!model.canOpenOutput).help(AppText.text(model.outputOpenHelp))
            if model.report != nil { Button(AppText.text("处理报告…"),action:actions.showReport).disabled(model.isBusy) }
            Divider()
            Button(AppText.text("最近结果与交付…"),action:actions.recentResults).disabled(model.isBusy || model.sharing)
            if model.libraryMonitoring { Button(AppText.text("停止图库监测"),action:actions.toggleLibraryMonitoring).disabled(model.isBusy) }
            Divider()
            Link(UserGuide.title(for:locale),destination:UserGuide.url(for:locale))
        } label: { Image(systemName:"checkmark.rectangle.stack") }
            .menuStyle(.borderlessButton).fixedSize()
            .help(AppText.text(resultMenuHelp))
            .accessibilityLabel(AppText.text("结果与交付"))
            .recordLayout("workspace-more-menu")
    }
    private var resultMenuHelp: String {
        let summary = !model.deliverySummary.isEmpty ? model.deliverySummary : model.report?.summary ?? ""
        return (summary.isEmpty ? "" : summary + "\n") + "导出、共享、打开输出、处理报告与最近结果；不重新转换"
    }
    private var sourceSidebar: some View {
        ScrollView {
            VStack(alignment:.leading,spacing:10) {
              Group {
                HStack {
                    Text(AppText.text("来源")).font(.headline)
                    Spacer(minLength:0)
                    sourceMenu
                }
                addButton
                if !model.sources.isEmpty {
                    Button(role:.destructive,action:actions.clearSources) { Label(AppText.text("清空所有来源"),systemImage:"trash") }
                        .disabled(!model.canAppendSources).help(AppText.text("只清空本轮列表，不删除原片"))
                }
                if !model.sources.isEmpty {
                    Text(AppText.text("\(model.sources.count) 个入口 · \(model.items.count) 张")).font(.caption).foregroundColor(.secondary)
                }
                if model.sources.isEmpty {
                    Text(AppText.text("添加文件、文件夹或图库照片，也可直接拖进左侧照片区"))
                        .font(.caption).foregroundColor(.secondary).fixedSize(horizontal:false,vertical:true)
                } else {
                    sidebarButton("全部来源",symbol:"square.stack",selected:model.migrationSource == nil) { model.migrationSource = nil }
                }
                LazyVStack(alignment:.leading,spacing:4) {
                    ForEach(model.sources,id:\.path) { source in
                        sidebarButton(source.lastPathComponent,symbol:"folder",selected:model.migrationSource == source.path,verbatim:true) { model.migrationSource = source.path }
                            .help(source.path)
                    }
                }
              }
            }.frame(maxWidth:.infinity,alignment:.leading).padding(12)
        }
        .safeAreaInset(edge:.bottom,spacing:0) {
            if model.libraryMonitoring {
                VStack(alignment:.leading,spacing:6) {
                    Divider()
                    Label(AppText.text("正在监测图库"),systemImage:"photo.on.rectangle").font(.caption).foregroundColor(.secondary)
                    Button(AppText.text("停止监测"),action:actions.toggleLibraryMonitoring).disabled(model.isBusy).controlSize(.small)
                }.padding([.horizontal,.bottom],12).background(InterfaceStyle.content)
            }
        }
        .background(InterfaceStyle.content).recordLayout("source-sidebar")
    }
    private var sourceMenu: some View {
        Menu {
            Button(AppText.text("重新检查来源")) { model.rescan() }.disabled(!model.canRescan)
            Divider()
            Button(AppText.text("清空来源与列表"),role:.destructive,action:actions.clearSources)
                .disabled(model.sources.isEmpty || model.isBusy || model.sharing)
        } label: { Image(systemName:"ellipsis.circle") }
            .menuStyle(.borderlessButton).fixedSize().help(AppText.text("重新检查或清空来源"))
            .accessibilityLabel(AppText.text("来源操作")).accessibilityIdentifier("source-more-menu")
    }
    private func sidebarButton(_ title: String, symbol: String, selected: Bool, verbatim: Bool = false, action: @escaping () -> Void) -> some View {
        Button(action:action) {
            Label(verbatim ? title : AppText.text(title),systemImage:symbol).font(.callout).lineLimit(1).truncationMode(.middle)
                .frame(maxWidth:.infinity,alignment:.leading).padding(.horizontal,7).padding(.vertical,6)
                .background(selected ? InterfaceStyle.selection : .clear).cornerRadius(InterfaceStyle.controlRadius)
        }.buttonStyle(.plain).accessibilityAddTraits(selected ? .isSelected : [])
    }
}

private struct WorkflowStepButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label.opacity(configuration.isPressed ? 0.75 : 1)
    }
}

struct UnconvertedExportControl: View {
    @ObservedObject var model: WorkspaceModel
    let actions: WorkspaceActions
    var showsToggle = true
    var showsLocation = true
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
            if showsToggle {
                Toggle(AppText.text("输出未修改的照片"),isOn:enabled).toggleStyle(.checkbox)
                    .accessibilityIdentifier("unconverted-export-toggle")
                    .recordLayout("unconverted-export-toggle")
            }
            if showsLocation && model.unconvertedExport != .none {
                VStack(alignment:.leading,spacing:7) {
                    Picker(AppText.text("原样导出位置"),selection:location) {
                        Text(AppText.text("导出到相同位置")).tag(UnconvertedExportChoice.sameLocation)
                        Text(AppText.text("导出到不同位置")).tag(UnconvertedExportChoice.separateLocation)
                    }.labelsHidden().frame(maxWidth:.infinity,alignment:.leading).accessibilityIdentifier("unconverted-export-location")
                    if model.unconvertedExport == .separateLocation {
                        Button(AppText.text("选择目录…"),action:actions.chooseUnconvertedOutput).accessibilityIdentifier("unconverted-export-folder")
                            .recordLayout("unconverted-export-folder")
                    }
                }
                Text(AppText.text(model.unconvertedExport == .separateLocation ? model.unconvertedOutput?.path ?? "请选择不同的原文件导出目录" : "使用本次转换输出位置，另建原文件目录，不覆盖来源；原样导出不会计为转换成功。"))
                    .font(.caption).foregroundColor(.secondary).lineLimit(2).truncationMode(.middle)
                    .textSelection(.enabled).help(AppText.text(model.unconvertedOutput?.path ?? "原样导出不会计为转换成功"))
            }
        }.font(.callout).disabled(model.isBusy || model.sharing).accessibilityIdentifier("unconverted-export-options")
            .recordLayout("unconverted-export-options")
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
                            Text(AppText.text(message)).foregroundColor(hasError ? .red : .secondary)
                        } icon: {
                            Image(systemName:statusSymbol)
                                .foregroundColor(hasError ? .red : model.phase == .completed && !hasFailures ? .green : .secondary)
                        }.lineLimit(1).help(AppText.text(message))
                            .id(model.phase).transition(.opacity).subtleAnimation(model.phase)
                        Spacer(minLength:8)
                        if busy, let progress = model.progress {
                            Text(AppText.text("\(Int((min(1,max(0,progress)) * 100).rounded()))%"))
                                .monospacedDigit().foregroundColor(.secondary)
                        }
                        if hasError { Button(AppText.text("查看原因"),action:showError) }
                        else if hasFailures { Button(AppText.text("查看报告"),action:actions.showReport).disabled(model.isBusy) }
                    }.font(.callout)
                    if busy {
                        taskProgress
                        Text(AppText.text(secondaryMessage)).font(.caption).foregroundColor(.secondary)
                            .lineLimit(1).truncationMode(.middle).help(AppText.text(secondaryMessage))
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
        .accessibilityLabel(AppText.text("任务进度"))
        .recordLayout("task-progress")
    }
}
