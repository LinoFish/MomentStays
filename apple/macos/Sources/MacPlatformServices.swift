import AppKit
import SwiftUI
import UniformTypeIdentifiers
import Photos
import Combine

// macOS-only ownership of windows, permissions, imported temporary originals and
// native dialogs. Shared views/models only see WorkspaceActions and callbacks.
final class MacPlatformServices: NSObject {
    let model: WorkspaceModel
    weak var window: NSWindow?
    private var libraryPicker: PhotosLibraryPicker?
    private var ownedPhotoCopies: [URL] = []
    private var reportWindow: NSWindow?
    private var outputAlert: NSAlert?
    private var batchDecision: ExistingOutputDecision?
    private var dropRequest = UUID()
    private let deliveryStore = DeliveryStore()
    private let sharingService = SystemSharingService()
    private var activeTask: DeliveryTask?
    private var pendingOriginalExport: FileExportRecord?
    private var consumingMigration = false
    private var pendingSources: [URL] = []
    private var lastTask: DeliveryTask? {
        didSet {
            model.recentOutput = lastTask.map { URL(fileURLWithPath:$0.output) }
            model.resultExportAvailable = lastTask?.results.isEmpty == false || deliveryStore.tasks().contains { !$0.results.isEmpty }
        }
    }
    private var origins: [String: LibraryOrigin] = [:]
    private var libraryMonitor: PhotoLibraryMonitor?
    private var lastScope: LibraryScope?
    private var pendingAssets: [String: PHAsset] = [:]
    private var autoStartPending = false
    private var libraryReader: PhotoLibraryService?
    private var readerToken: StopToken?
    private var albumSelectionPending = false
    private var subscriptions: Set<AnyCancellable> = []

    init(model: WorkspaceModel) {
        self.model = model
        super.init()
        model.prepareConversion = { [weak self] settings, output in
            guard let self = self else { throw Failure("窗口已关闭") }
            var task = DeliveryTask(settings:settings,output:output.path)
            if settings.destination.usesCache { task.output = self.deliveryStore.taskFolder(task.id).appendingPathComponent("Converted").path }
            task.originalExport = self.pendingOriginalExport
            self.pendingOriginalExport = nil
            if var record = task.originalExport, record.output.isEmpty { record.output = task.output; task.originalExport = record }
            try self.deliveryStore.save(task)
            self.activeTask = task; self.model.deliverySummary = ""; self.model.shareReady = false
            return URL(fileURLWithPath:task.output)
        }
        model.onConversionFinished = { [weak self] outcome, settings in self?.converted(outcome, settings: settings) }
        model.$phase.receive(on: DispatchQueue.main).sink { [weak self] phase in
            if !phase.isBusy { self?.drainLibraryUpdates() }
        }.store(in: &subscriptions)
        NotificationCenter.default.publisher(for: NSWindow.didEndSheetNotification).receive(on: DispatchQueue.main).sink { [weak self] notification in
            guard let self = self, let window = notification.object as? NSWindow, window === self.window else { return }
            self.consumeMigration()
            self.drainLibraryUpdates()
        }.store(in: &subscriptions)
        model.$settings.map(\.direction).removeDuplicates().receive(on: DispatchQueue.main).sink { [weak self] direction in
            if let self = self, let scope = self.lastScope, scope.direction != direction { self.stopMonitoring() }
        }.store(in: &subscriptions)
        model.$reviewedMigration.receive(on: DispatchQueue.main).sink { [weak self] _ in self?.consumeMigration() }.store(in:&subscriptions)
        model.$migrationTargetConfirmed.removeDuplicates().receive(on:DispatchQueue.main).sink { [weak self] confirmed in
            guard confirmed, let self = self, !self.pendingSources.isEmpty else { return }
            let urls = self.pendingSources; self.pendingSources = []
            self.openSources(urls)
        }.store(in:&subscriptions)
        lastTask = deliveryStore.tasks().first(where: { $0.hasRecord })
        model.recentOutput = lastTask.map { URL(fileURLWithPath: $0.output) }
        model.recentTasksAvailable = lastTask != nil
        model.resultExportAvailable = deliveryStore.tasks().contains { !$0.results.isEmpty }
        model.shareReady = lastTask?.canShare == true
        model.presentExistingOutput = { [weak self] source, target, reply in
            guard let self = self else { reply(.cancel); return }
            self.presentExistingOutput(source: source, target: target, reply: reply)
        }
        model.cancelPlatformActivity = { [weak self] in self?.cancelPendingActivity() }
    }

    var actions: WorkspaceActions {
        WorkspaceActions(chooseSources: { [weak self] in self?.chooseSources() },
            chooseFolder: { [weak self] in self?.chooseSources(foldersOnly:true) },
            clearSources: { [weak self] in self?.clearSources() },
            chooseOutput: { [weak self] in self?.chooseOutput() },
            chooseDestination: { [weak self] destination in self?.chooseDestination(destination) },
            importPhotos: { [weak self] in self?.importPhotos() },
            start: { [weak self] in self?.start() },
            openOutput: { [weak self] in self?.openOutput() },
            showReport: { [weak self] in self?.showReport() },
            reveal: { NSWorkspace.shared.activateFileViewerSelecting([$0]) },
            copy: { NSPasteboard.general.clearContents(); NSPasteboard.general.setString($0, forType: .string) },
            acceptDrop: { [weak self] providers, output in self?.acceptDrop(providers, output: output) ?? false },
            shareResults: { [weak self] in self?.shareResults() },
            exportResults: { [weak self] in self?.exportResults() },
            chooseUnconvertedOutput: { [weak self] in self?.chooseUnconvertedOutput() },
            chooseAlbum: { [weak self] in self?.chooseAlbum() },
            recentResults: { [weak self] in self?.showRecentTasks() },
            toggleLibraryMonitoring: { [weak self] in self?.stopMonitoring() })
    }

    func openSources(_ urls: [URL]) {
        guard model.canAppendSources else { return }
        guard model.migrationTargetConfirmed else {
            pendingSources = Array(Set(pendingSources + urls))
            model.workspaceStep = .direction
            return
        }
        stopMonitoring(); model.librarySummary = ""
        model.appendSources(urls)
        model.workspaceStep = .importPhotos
    }

    func clearSources() {
        guard !model.isBusy, !model.sharing else { return }
        dropRequest = UUID(); stopMonitoring(); origins = [:]; lastScope = nil
        autoStartPending = false
        pendingSources = []
        model.clearSources()
    }

    func chooseSources(foldersOnly: Bool = false) {
        guard model.migrationTargetConfirmed else { model.workspaceStep = .direction; return }
        guard model.canImportSources, let window = window, window.attachedSheet == nil else { return }
        let panel = NSOpenPanel()
        panel.title = AppText.text(foldersOnly ? "从文件夹导入" : model.settings.direction == .toAndroid ? "选择含完整实况配对的图片或文件夹" : "选择照片或来源文件夹")
        panel.canChooseDirectories = true; panel.canChooseFiles = !foldersOnly; panel.allowsMultipleSelection = true
        let extensions = model.settings.direction == .androidToAndroid || model.settings.direction == .honorToGoogle ? ["jpg","jpeg"] : ["jpg","jpeg","heic","heif"]
        panel.allowedContentTypes = extensions.compactMap { UTType(filenameExtension:$0) }
        panel.prompt = AppText.text("选择并扫描")
        panel.beginSheetModal(for: window) { [weak self] response in
            guard response == .OK else { return }
            self?.stopMonitoring(); self?.model.librarySummary = ""
            self?.model.appendSources(panel.urls)
            self?.model.workspaceStep = .importPhotos
        }
    }

    // Output controls live in the inspector or its SwiftUI sheet. Attach their
    // native dialogs to that sheet when present, instead of silently rejecting
    // every request just because the main window already has a settings sheet.
    private var settingsDialogParent: NSWindow? {
        guard let window = window else { return nil }
        if let sheet = window.attachedSheet {
            guard model.settingsPresented || model.migrationReviewPresented, sheet.attachedSheet == nil else { return nil }
            return sheet
        }
        return window
    }

    func chooseOutput() {
        if model.settings.direction == .toApple { chooseDestination(.folder); return }
        guard !model.isBusy, !model.sharing, !model.isPreview, let parent = settingsDialogParent else { return }
        let panel = NSOpenPanel(); panel.title = AppText.text("选择输出位置"); panel.prompt = "选择"
        panel.canChooseDirectories = true; panel.canChooseFiles = false; panel.canCreateDirectories = true
        panel.directoryURL = model.output?.deletingLastPathComponent()
        panel.beginSheetModal(for: parent) { [weak self] response in
            if response == .OK, let url = panel.url { self?.model.setOutput(url) }
        }
    }

    func importPhotos() {
        guard model.migrationTargetConfirmed else { model.workspaceStep = .direction; return }
        guard model.settings.direction != .androidToAndroid && model.settings.direction != .honorToGoogle else { return }
        guard let window = window, window.attachedSheet == nil, let id = model.beginLibraryImport() else { return }
        dropRequest = UUID(); stopMonitoring()
        let direction = model.settings.direction
        let picker = PhotosLibraryPicker(direction: direction, version: model.settings.libraryVersion, store: deliveryStore) { [weak self] urls, error in
            guard let self = self else { return }
            let picker = self.libraryPicker
            self.libraryPicker = nil
            if picker?.wasCancelled == false, error == nil, !urls.isEmpty { self.origins.merge(picker?.origins ?? [:],uniquingKeysWith:{ _,new in new }) }
            self.lastScope = picker?.selectedScope
            self.model.librarySummary = picker?.summary ?? ""
            if let first = urls.first { self.ownedPhotoCopies.append(first.deletingLastPathComponent().deletingLastPathComponent()) }
            let output = FileManager.default.urls(for: .picturesDirectory, in: .userDomainMask).first!.appendingPathComponent("MomentStays Exports")
            self.model.finishLibraryImport(id: id, urls: urls, error: error, output: output, direction: direction, origins:picker?.origins ?? [:])
            if picker?.wantsMonitoring == true, picker?.wasCancelled == false, error == nil, let scope = self.lastScope {
                self.startMonitoring(scope)
            }
        }
        picker.onProgress = { [weak self] message in self?.model.updateLibraryStatus(message) }
        libraryPicker = picker; picker.present(window)
    }

    func start() {
        guard window?.attachedSheet == nil, !model.sharing else { return }
        guard model.workspaceStep == .preview else {
            if model.canReviewMigration { model.workspaceStep = .preview }
            return
        }
        model.approveMigrationReview()
        if model.reviewedMigration != nil { model.workspaceStep = .results }
    }

    private func consumeMigration() {
        guard !consumingMigration, let review = model.reviewedMigration,
              window?.attachedSheet == nil, !model.isBusy, !model.sharing else { return }
        consumingMigration = true
        model.reviewedMigration = nil
        defer { consumingMigration = false }
        guard model.validateReviewedMigration(review) else { return }
        batchDecision = nil
        do {
            pendingOriginalExport = nil
            if review.choice != .none {
                let candidates = model.items.filter { review.readyIDs.contains($0.id) }.map(\.row) + review.originalRows
                let request = FileExportRequest.originals(candidates,direction:review.settings.direction,photos:Set(origins.keys))
                pendingOriginalExport = FileExportRecord(kind:"originals",output:review.choice == .sameLocation ? "" : review.originalsOutput?.path ?? "",
                    request:try JSONSerialization.data(withJSONObject:request,options:.sortedKeys))
            }
            if !review.readyIDs.isEmpty {
                model.startReviewedMigration(review,outputOverride:model.output ?? deliveryStore.root)
                pendingOriginalExport = nil
            } else if let record = pendingOriginalExport {
                pendingOriginalExport = nil
                var task = DeliveryTask(settings:review.settings,output:model.output?.path ?? deliveryStore.root.path)
                if review.settings.destination.usesCache { task.output = deliveryStore.taskFolder(task.id).appendingPathComponent("Converted").path }
                var record = record
                if record.output.isEmpty { record.output = task.output }
                if review.choice == .separateLocation { task.output = record.output }
                task.originalExport = record
                try deliveryStore.save(task)
                finishTaskFiles(task)
            }
        } catch { pendingOriginalExport = nil; model.errorMessage = error.localizedDescription }
    }

    func chooseUnconvertedOutput() {
        guard !model.isBusy, !model.sharing, let parent = settingsDialogParent else { return }
        let panel = NSOpenPanel(); panel.title = AppText.text("选择未转换照片的导出位置"); panel.prompt = "选择"
        panel.canChooseDirectories = true; panel.canChooseFiles = false; panel.canCreateDirectories = true
        panel.directoryURL = model.unconvertedOutput
        panel.beginSheetModal(for:parent) { [weak self] response in
            if response == .OK, let url = panel.url { self?.model.setUnconvertedOutput(url) }
        }
    }

    private func converted(_ outcome: BatchOutcome, settings: ConversionSettings) {
        guard var task = activeTask else { return }; activeTask = nil
        task.settings = settings
        task.processingCounts = ConversionReport(outcome).counts
        let capturedOrigins = origins
        let token = model.beginDelivery("正在校验和保存交付记录…")
        DispatchQueue.global(qos:.userInitiated).async {
            let result = Result { () -> DeliveryTask in
                if let record = task.originalExport {
                    task.originalExport = try FileExportRequest.remainingOriginals(record,outcome:outcome)
                    try self.deliveryStore.save(task)
                }
                task.results = try DeliveryStore.collect(outcome,settings:settings,origins:capturedOrigins)
                task.cancelled = outcome.cancelled
                try self.deliveryStore.save(task)
                return task
            }
            DispatchQueue.main.async {
                switch result {
                case .failure(let error): self.model.finishDelivery("转换结果保留，但交付准备失败",error:error)
                case .success(let task):
                    let stopped = token.isCancelled || outcome.cancelled
                    self.finishTaskFiles(task,cancelled:stopped)
                }
            }
        }
    }

    // Runs original export after ready conversions, never blocks those conversions
    // on a manual/unsupported item and never imports originals into Photos.
    private func finishTaskFiles(_ initial: DeliveryTask, cancelled: Bool = false, deliverResults: Bool = true) {
        let token = model.beginDelivery("正在处理原样导出与任务记录…")
        if cancelled { token.cancel() }
        DispatchQueue.global(qos:.userInitiated).async {
            var task = initial
            if var record = task.originalExport {
                if token.isCancelled { record.state = "待继续（本轮已停止）" }
                else {
                    do {
                        try self.runExport(&record,token:token) { checkpoint in
                            var saved = task; saved.originalExport = checkpoint; try self.deliveryStore.save(saved)
                        }
                    } catch { record.state = token.isCancelled ? "已停止" : "未完成"; record.detail = error.localizedDescription }
                }
                task.originalExport = record
            }
            let save = Result { try self.deliveryStore.save(task) }
            DispatchQueue.main.async {
                self.lastTask = task; self.model.recentTasksAvailable = task.hasRecord || self.model.recentTasksAvailable
                self.model.shareReady = task.canShare
                var summary = "生成结果：" + task.resultSummary
                if !task.processingSummary.isEmpty { summary += "；" + task.processingSummary }
                if let record = task.originalExport {
                    summary += "；" + record.summary + (record.detail.map { "；" + $0 } ?? "")
                    self.model.recordOriginalExport(record.report)
                }
                switch save {
                case .failure(let error): self.model.finishDelivery("文件保留，但任务记录保存失败",error:error)
                case .success:
                    self.model.finishDelivery(summary)
                    guard deliverResults, !token.isCancelled, !task.cancelled, !task.results.isEmpty else { return }
                    if task.settings.destination.importsPhotos { self.importTask(task) }
                    else if task.settings.destination.shares { self.shareResults() }
                }
            }
        }
    }

    private func runExport(_ record: inout FileExportRecord, token: StopToken,
                           checkpoint: (FileExportRecord) throws -> Void) throws {
        try FileExportRequest.run(&record,token:token,progress:{ update in
            DispatchQueue.main.async { self.model.deliveryProgress("导出 \(update.index) / \(update.total) · " + update.relative) }
        },existing:{ source,target in
            let latch = OutputDecisionLatch()
            DispatchQueue.main.async {
                guard !token.isCancelled else { latch.resolve(.cancel); return }
                self.model.awaitingExportDecision(true)
                self.presentExistingOutput(source:source,target:target) { decision in
                    self.model.awaitingExportDecision(false); latch.resolve(decision)
                }
            }
            return latch.wait(token:token)
        },checkpoint:checkpoint)
    }

    func exportResults(_ selected: DeliveryTask? = nil) {
        guard !model.isBusy, !model.sharing, let window = window, window.attachedSheet == nil,
              let task = selected ?? (lastTask?.results.isEmpty == false ? lastTask : deliveryStore.tasks().first { !$0.results.isEmpty }), !task.results.isEmpty else { return }
        let panel = NSOpenPanel(); panel.title = AppText.text("导出完整转换结果"); panel.prompt = "导出到此位置"
        panel.canChooseDirectories = true; panel.canChooseFiles = false; panel.canCreateDirectories = true
        panel.beginSheetModal(for:window) { [weak self] response in
            guard let self = self, response == .OK, let output = panel.url else { return }
            do {
                try NativeBatch.validateOutput(sources:self.model.sources,output:output)
                let request = try FileExportRequest.results(task.results)
                let record = FileExportRecord(kind:"results",output:output.path,request:try JSONSerialization.data(withJSONObject:request,options:.sortedKeys))
                self.runResultExport(task,record:record)
            } catch { self.model.errorMessage = error.localizedDescription }
        }
    }

    private func runResultExport(_ initial: DeliveryTask, record: FileExportRecord) {
        batchDecision = nil
        let token = model.beginDelivery("正在校验完整转换结果…")
        DispatchQueue.global(qos:.userInitiated).async {
            var task = initial, record = record
            do {
                try self.runExport(&record,token:token) { checkpoint in
                    var saved = task; saved.resultExport = checkpoint; try self.deliveryStore.save(saved)
                }
            } catch { record.state = token.isCancelled ? "已停止" : "未完成"; record.detail = error.localizedDescription }
            task.resultExport = record
            let save = Result { try self.deliveryStore.save(task) }
            DispatchQueue.main.async {
                self.lastTask = task
                switch save {
                case .failure(let error): self.model.finishDelivery("文件保留，导出记录保存失败",error:error)
                case .success: self.model.finishDelivery(record.summary + (record.detail.map { "；" + $0 } ?? ""))
                }
            }
        }
    }

    private func importTask(_ task: DeliveryTask) {
        guard !model.isBusy, !model.sharing else { return }
        let token = model.beginDelivery("正在准备写入相册…")
        PhotoLibraryService.authorize { [weak self] result in
            guard let self = self else { return }
            switch result {
            case .failure(let error): self.model.finishDelivery("未导入；转换文件保留", error: error)
            case .success:
                if token.isCancelled { self.model.finishDelivery("入册已停止；文件保留"); return }
                let service = PhotoLibraryService(); self.libraryReader = service
                DispatchQueue.global(qos: .userInitiated).async {
                    var task = task
                    let result = Result { try service.importResults(&task, store: self.deliveryStore, token: token) { message in
                        DispatchQueue.main.async { self.model.deliveryProgress(message) }
                    } }
                    DispatchQueue.main.async {
                        self.libraryReader = nil; self.lastTask = task
                        switch result {
                        case .success(let summary): self.model.finishDelivery(summary + (task.originalExport.map { "；" + $0.summary } ?? ""))
                        case .failure(let error): self.model.finishDelivery("入册未完成；可在最近结果中重试", error: error)
                        }
                    }
                }
            }
        }
    }

    func shareResults() {
        guard !model.isBusy, !model.sharing, let window = window, window.attachedSheet == nil,
              var task = lastTask, task.canShare else { return }
        let token = model.beginDelivery("正在准备完整文件共享…")
        DispatchQueue.global(qos: .userInitiated).async {
            let result = Result { try self.deliveryStore.stageShare(&task, token: token) }
            DispatchQueue.main.async {
                self.lastTask = task
                self.model.finishDelivery("文件已就绪；请在系统菜单中选择共享应用")
                guard !token.isCancelled else { return }
                switch result {
                case .failure(let error): self.model.errorMessage = error.localizedDescription
                case .success(let urls):
                    self.model.sharing = true
                    self.sharingService.present(urls, window: window) { [weak self] message, active in
                        guard let self = self else { return }
                        self.model.sharing = active; self.model.deliverySummary = message
                        if var task = self.lastTask { task.shareState = message; self.lastTask = task; try? self.deliveryStore.save(task) }
                        if !active { self.drainLibraryUpdates() }
                    }
                }
            }
        }
    }

    private var outputChoiceRequest = UUID()
    func chooseDestination(_ destination: OutputDestination) {
        guard model.settings.direction == .toApple, !model.isBusy, !model.sharing, !model.isPreview,
              [.folder,.photos,.folderAndPhotos].contains(destination),
              model.settings.exportMode != .auxiliaryOnly || destination == .folder,
              let parent = settingsDialogParent else { return }
        let request = UUID(); outputChoiceRequest = request
        let snapshot = model.settings, sources = model.sources
        func commit(_ folder: URL?, _ albumID: String?, _ title: String?) {
            guard self.outputChoiceRequest == request, self.model.settings == snapshot, self.model.sources == sources else { return }
            self.model.confirmOutput(destination:destination,folder:folder,albumID:albumID,albumTitle:title)
        }
        func album(_ folder: URL?) {
            self.pickAlbum(parent:parent) { id,title in commit(folder,id,title) }
        }
        if destination == .photos { album(nil); return }
        let panel = NSOpenPanel(); panel.title = AppText.text("选择输出文件夹"); panel.prompt = "选择"
        panel.canChooseDirectories = true; panel.canChooseFiles = false; panel.canCreateDirectories = true
        panel.directoryURL = model.output
        panel.beginSheetModal(for:parent) { response in
            guard response == .OK, let folder = panel.url else { return }
            if destination == .folder { commit(folder,nil,nil) }
            else { DispatchQueue.main.async { album(folder) } }
        }
    }
    func chooseAlbum() {
        guard let parent = settingsDialogParent else { return }
        pickAlbum(parent:parent) { [weak self] id,title in
            self?.model.updateSettings { $0.albumID = id; $0.albumTitle = title }
        }
    }
    private func pickAlbum(parent: NSWindow, completion: @escaping (String?,String) -> Void) {
        guard !model.isBusy, !model.sharing, !model.isPreview, !albumSelectionPending else { return }
        albumSelectionPending = true
        PhotoLibraryService.authorize { [weak self, weak parent] result in
            guard let self = self else { return }
            defer { self.albumSelectionPending = false }
            guard let parent = parent, self.settingsDialogParent === parent, !self.model.isBusy, !self.model.sharing else { return }
            if case .failure(let error) = result { self.model.errorMessage = error.localizedDescription; return }
            let albums = PhotoLibraryService.albums()
            let alert = NSAlert(); alert.messageText = AppText.text("输出相册")
            alert.informativeText = AppText.text("选择已有用户相册，或输入新相册名称。仅在转换成功后创建相册。")
            let popup = NSPopUpButton(frame:NSRect(x:0,y:38,width:300,height:28),pullsDown:false)
            popup.addItem(withTitle:AppText.text("按下方名称创建／查找"))
            popup.addItems(withTitles:albums.map { $0.localizedTitle ?? "未命名相册" })
            if let index = albums.firstIndex(where:{ $0.localIdentifier == self.model.settings.albumID }) { popup.selectItem(at:index + 1) }
            let name = NSTextField(frame:NSRect(x:0,y:0,width:300,height:26)); name.stringValue = self.model.settings.albumTitle
            let view = NSView(frame:NSRect(x:0,y:0,width:300,height:70)); view.addSubview(popup); view.addSubview(name); alert.accessoryView = view
            alert.addButton(withTitle:AppText.text("选择")); alert.addButton(withTitle:AppText.text("取消"))
            alert.beginSheetModal(for:parent) { response in
                guard response == .alertFirstButtonReturn else { return }
                let index = popup.indexOfSelectedItem
                let title = (index > 0 ? albums[index - 1].localizedTitle ?? "未命名相册" : name.stringValue).trimmingCharacters(in:.whitespacesAndNewlines)
                guard !title.isEmpty else { self.model.errorMessage = "请输入相册名称"; return }
                completion(index > 0 ? albums[index - 1].localIdentifier : nil,title)
            }
        }
    }

    func stopMonitoring() {
        libraryMonitor = nil; pendingAssets = [:]; model.libraryMonitoring = false
    }
    private func startMonitoring(_ scope: LibraryScope) {
        stopMonitoring()
        let monitor = PhotoLibraryMonitor(scope: scope)
        monitor.onChange = { [weak self] assets in
            guard let self = self, self.model.libraryMonitoring, self.lastScope == scope else { return }
            let excluded = self.deliveryStore.generatedAssetIDs
            for asset in assets where !excluded.contains(asset.localIdentifier) {
                if self.deliveryStore.inspectionVersion(asset.localIdentifier, scope: scope.key) != PhotoLibraryService.version(asset, edited: scope.version == .edited) {
                    self.pendingAssets[asset.localIdentifier] = asset
                }
            }
            self.drainLibraryUpdates()
        }
        libraryMonitor = monitor; lastScope = scope; model.libraryMonitoring = true
    }
    private func drainLibraryUpdates() {
        guard model.libraryMonitoring, !model.manualDecisionPending, !model.isBusy, !model.sharing, !autoStartPending, libraryPicker == nil,
              window?.attachedSheet == nil, let scope = lastScope, scope.direction == model.settings.direction,
              !pendingAssets.isEmpty else { return }
        guard PHPhotoLibrary.authorizationStatus(for: .readWrite) == .authorized else {
            stopMonitoring(); model.librarySummary = "图库监测已停止：权限不可用"; return
        }
        guard let id = model.beginLibraryImport() else { return }
        let assets = Array(pendingAssets.values); pendingAssets = [:]
        let reader = PhotoLibraryService(), token = StopToken(); libraryReader = reader; readerToken = token
        DispatchQueue.global(qos: .userInitiated).async {
            let result = Result { try reader.export(assets, scope: scope, store: self.deliveryStore, token: token) { _, _, message in
                DispatchQueue.main.async { self.model.updateLibraryStatus(message) }
            } }
            DispatchQueue.main.async {
                self.libraryReader = nil; self.readerToken = nil
                switch result {
                case .failure(let error): self.model.finishLibraryImport(id: id, urls: [], error: error, output: self.model.output ?? self.deliveryStore.root, direction: scope.direction)
                case .success(let read):
                    if !token.isCancelled, !read.urls.isEmpty { self.origins = read.origins }
                    if !read.urls.isEmpty { self.ownedPhotoCopies.append(read.root) }
                    self.model.librarySummary = "自动识别 \(read.urls.count) · 静态跳过 \(read.skipped) · 待读取 \(read.deferred)"
                    let output = self.model.output ?? FileManager.default.urls(for: .picturesDirectory, in: .userDomainMask).first!.appendingPathComponent("MomentStays Exports")
                    self.autoStartPending = !token.isCancelled && !read.urls.isEmpty
                    self.model.finishLibraryImport(id: id, urls: read.urls, error: nil, output: output, direction: scope.direction, appending:false, origins:read.origins)
                    if !token.isCancelled, !read.urls.isEmpty {
                        // The worker starts only after the queued scan reaches ready.
                        var subscription: AnyCancellable?
                        subscription = self.model.$phase.receive(on: DispatchQueue.main).sink { phase in
                            if phase == .ready { subscription?.cancel(); self.autoStartPending = false; self.start() }
                            else if phase == .failed || phase == .stopped { subscription?.cancel(); self.autoStartPending = false; self.drainLibraryUpdates() }
                        }
                        if let subscription = subscription { self.subscriptions.insert(subscription) }
                    }
                }
            }
        }
    }

    func showRecentTasks() {
        guard !model.isBusy, !model.sharing, let window = window, window.attachedSheet == nil else { return }
        let panel = NSWindow(contentRect: NSRect(x: 0, y: 0,
            width: min(760, max(WindowSizing.recent.width, window.contentLayoutRect.width - 24)),
            height: min(580, max(WindowSizing.recent.height, window.contentLayoutRect.height - 24))),
            styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
        panel.title = AppText.text("最近转换结果")
        panel.contentMinSize = WindowSizing.recent
        panel.isReleasedWhenClosed = false
        var retryTask: DeliveryTask?
        var exportTask: DeliveryTask?
        var originalsTask: DeliveryTask?
        panel.contentView = NSHostingView(rootView: RecentTasksView(
            model: model, initialTasks: deliveryStore.tasks().filter { $0.hasRecord },
            close: { window.endSheet(panel) },
            retry: { task in retryTask = task; window.endSheet(panel) },
            export: { task in exportTask = task; window.endSheet(panel) },
            originals: { task in originalsTask = task; window.endSheet(panel) },
            remove: { [weak self] task in
                guard let self = self, !self.model.isBusy, !self.model.sharing else { throw Failure("请先完成当前任务或共享") }
                try self.deliveryStore.removeTask(task, active: [])
                let tasks = self.deliveryStore.tasks().filter { $0.hasRecord }
                self.lastTask = tasks.first
                self.model.shareReady = self.lastTask?.canShare == true
                self.model.recentTasksAvailable = !tasks.isEmpty
                return tasks
            }))
        window.beginSheet(panel) { [weak self] _ in
            panel.orderOut(nil)
            panel.contentView = nil
            guard let self = self, !self.model.isBusy, !self.model.sharing else { return }
            if let task = exportTask { self.exportResults(task); return }
            if let task = originalsTask { self.batchDecision = nil; self.finishTaskFiles(task,deliverResults:false); return }
            guard let task = retryTask else { return }
            self.lastTask = task
            self.model.shareReady = task.canShare
            if task.settings.direction.exportsAndroid { self.shareResults() }
            else { self.importTask(task) }
        }
    }

    func openOutput() {
        guard model.canOpenOutput, let output = model.outputToOpen else { return }
        NSWorkspace.shared.open(output)
    }

    func showReport() {
        guard let report = model.report else { return }
        if let old = reportWindow { old.close() }
        let panel = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 720, height: 520),
            styleMask: [.titled, .closable, .resizable, .miniaturizable], backing: .buffered, defer: false)
        panel.title = AppText.text("处理报告"); panel.isReleasedWhenClosed = false
        panel.contentMinSize = WindowSizing.report
        panel.contentView = NSHostingView(rootView:ReportView(report:report,actions:actions)
            .frame(minWidth:WindowSizing.report.width,minHeight:WindowSizing.report.height))
        panel.contentMinSize = WindowSizing.report
        panel.center(); panel.makeKeyAndOrderFront(nil); reportWindow = panel
    }

    private func presentExistingOutput(source: URL, target: URL, reply: @escaping (ExistingOutputDecision) -> Void) {
        guard let window = window, model.phase == .awaitingDecision else { reply(.cancel); return }
        if let decision = batchDecision { reply(decision); return }
        let alert = NSAlert(); outputAlert = alert
        alert.messageText = AppText.text("已有输出")
        alert.informativeText = AppText.text("照片：\(source.lastPathComponent)\n已有输出：\(target.path)\n\n校验并复用会核对来源、范围、设置和输出文件，匹配时继续且不重新处理。跳过会保留已有文件。覆盖会在新结果完成并通过校验后替换输出文件夹；失败时保留旧结果。")
        alert.alertStyle = .warning
        alert.addButton(withTitle: AppText.text("校验并复用"))
        alert.addButton(withTitle: AppText.text("覆盖"))
        alert.addButton(withTitle: AppText.text("跳过"))
        alert.addButton(withTitle: AppText.text("停止本次任务")).keyEquivalent = "\u{1b}"
        alert.showsSuppressionButton = true
        alert.suppressionButton?.title = AppText.text("将此选择应用于本次所有已有输出")
        alert.beginSheetModal(for: window) { [weak self, weak alert] response in
            let choice: ExistingOutputDecision
            switch response {
            case .alertFirstButtonReturn: choice = .resume
            case .alertSecondButtonReturn: choice = .overwrite
            case .alertThirdButtonReturn: choice = .skip
            default: choice = .cancel
            }
            if choice != .cancel && alert?.suppressionButton?.state == .on { self?.batchDecision = choice }
            if self?.outputAlert === alert { self?.outputAlert = nil }
            reply(choice)
        }
    }

    func cancelPendingActivity() {
        libraryPicker?.cancel(); readerToken?.cancel(); libraryReader?.cancel()
        if let alert = outputAlert {
            outputAlert = nil
            window?.endSheet(alert.window, returnCode: .abort)
            alert.window.orderOut(nil)
        }
    }

    private func acceptDrop(_ providers: [NSItemProvider], output: Bool) -> Bool {
        guard output ? (!model.isBusy && !model.sharing && !model.isPreview) : model.canAppendSources else { return false }
        let providers = providers.filter { $0.hasItemConformingToTypeIdentifier(UTType.fileURL.identifier) }
        guard !providers.isEmpty else { return false }
        // Independent additions share the current clear/import generation; a
        // second drop must not discard the first drop's asynchronous URL load.
        let request = dropRequest
        let group = DispatchGroup(), lock = NSLock()
        var urls: [(Int, URL)] = []
        for (index, provider) in providers.enumerated() {
            group.enter()
            provider.loadItem(forTypeIdentifier: UTType.fileURL.identifier, options: nil) { item, _ in
                defer { group.leave() }
                let url: URL?
                if let data = item as? Data { url = URL(dataRepresentation: data, relativeTo: nil) }
                else if let value = item as? URL { url = value }
                else if let value = item as? String { url = URL(string: value) }
                else { url = nil }
                if let url = url, url.isFileURL { lock.lock(); urls.append((index, url)); lock.unlock() }
            }
        }
        group.notify(queue: .main) { [weak self] in
            guard let self = self, self.dropRequest == request,
                  output ? !self.model.isBusy : self.model.canAppendSources else { return }
            let ordered = urls.sorted { $0.0 < $1.0 }.map { $0.1 }
            if output {
                if ordered.count == 1, let url = ordered.first { self.model.setOutput(url) }
                else { self.model.errorMessage = "请拖入一个输出文件夹。" }
            } else if !ordered.isEmpty { self.openSources(ordered) }
        }
        return true
    }

    func cleanup() {
        stopMonitoring(); readerToken?.cancel(); libraryReader?.cancel()
        for root in ownedPhotoCopies { try? FileManager.default.removeItem(at: root) }
        ownedPhotoCopies.removeAll()
    }
}

// Presentation only: retry and cache removal keep the existing delivery/store paths.
struct RecentTasksView: View {
    @ObservedObject var model: WorkspaceModel
    @State private var tasks: [DeliveryTask]
    @State private var selection: String?
    @State private var cleanupPresented = false
    @State private var cleanupError: String?
    let close: () -> Void
    let retry: (DeliveryTask) -> Void
    let export: (DeliveryTask) -> Void
    let originals: (DeliveryTask) -> Void
    let remove: (DeliveryTask) throws -> [DeliveryTask]

    init(model: WorkspaceModel, initialTasks: [DeliveryTask], close: @escaping () -> Void,
         retry: @escaping (DeliveryTask) -> Void, export: @escaping (DeliveryTask) -> Void, originals: @escaping (DeliveryTask) -> Void, remove: @escaping (DeliveryTask) throws -> [DeliveryTask]) {
        self.model = model
        _tasks = State(initialValue: initialTasks)
        _selection = State(initialValue: initialTasks.first?.id)
        self.close = close; self.retry = retry; self.export = export; self.originals = originals; self.remove = remove
    }

    private var selectedTask: DeliveryTask? { tasks.first { $0.id == selection } }
    private func existingFiles(_ task: DeliveryTask) -> [URL] {
        let paths = task.results.flatMap(\.files) + (task.originalExport?.outputs ?? []) + (task.resultExport?.outputs ?? [])
        return paths.filter { FileManager.default.fileExists(atPath: $0) }
            .map { URL(fileURLWithPath: $0) }
    }
    private func filesAvailable(_ task: DeliveryTask) -> Bool {
        !task.results.isEmpty && task.results.allSatisfy { result in
            !result.files.isEmpty && result.files.allSatisfy { FileManager.default.fileExists(atPath: $0) }
        }
    }
    private func canRedeliver(_ task: DeliveryTask) -> Bool {
        !task.results.isEmpty && (task.settings.direction.exportsAndroid || task.settings.exportMode != .auxiliaryOnly)
    }
    private func deliveryStatus(_ task: DeliveryTask) -> String {
        if task.results.isEmpty, let record = task.originalExport { return record.summary }
        if task.settings.direction.exportsAndroid { return task.shareState }
        if !task.importSummary.isEmpty { return task.importSummary }
        return task.cancelled ? "任务已停止，已完成结果保留" : "转换结果已保存"
    }

    var body: some View {
        VStack(alignment:.leading,spacing:0) {
            HStack {
                VStack(alignment:.leading,spacing:4) {
                    Text(AppText.text("最近结果")).font(.title3.weight(.semibold))
                    Text(AppText.text(tasks.isEmpty ? "处理与导出任务会保留在这里" : "\(tasks.count) 次任务"))
                        .font(.callout).foregroundColor(.secondary)
                }
                Spacer()
                Button(AppText.text("完成"),action:close).keyboardShortcut(.defaultAction)
            }.padding(16)
            Divider()
            if tasks.isEmpty {
                VStack(spacing:12) {
                    Image(systemName:"clock.arrow.circlepath").font(.title).foregroundColor(.secondary)
                    Text(AppText.text("暂无转换结果")).font(.headline)
                    Text(AppText.text("完成转换后，可在这里查看文件、再次导入或共享。"))
                        .font(.callout).foregroundColor(.secondary).multilineTextAlignment(.center)
                }.padding(24).frame(maxWidth:.infinity,maxHeight:.infinity)
            } else {
                history
                Divider()
                if let task = selectedTask {
                    taskDetails(task)
                    Divider()
                    taskActions(task)
                } else {
                    Text(AppText.text("选择一条记录查看结果")).foregroundColor(.secondary)
                        .frame(maxWidth:.infinity,maxHeight:.infinity)
                }
            }
        }
        .background(InterfaceStyle.canvas)
        .tint(InterfaceStyle.accent).accentColor(InterfaceStyle.accent)
        .frame(minWidth:WindowSizing.recent.width,minHeight:WindowSizing.recent.height)
        .dismissOnEscape(close)
        .alert(AppText.text("移除记录与缓存？"),isPresented:$cleanupPresented) {
            Button(AppText.text("取消"),role:.cancel) {}
            Button(AppText.text("移除"),role:.destructive) {
                guard let task = selectedTask else { return }
                do { tasks = try remove(task); selection = tasks.first?.id; cleanupError = nil }
                catch { cleanupError = error.localizedDescription }
            }
        } message: {
            Text(AppText.text("移除此任务的记录和应用缓存。用户输出文件夹与图库照片会保留；缓存清理后无法再次导入或共享。"))
        }
    }

    private var history: some View {
        List(selection:$selection) {
            ForEach(tasks) { task in
                historyRow(task).tag(task.id)
            }
        }.listStyle(.plain).frame(minHeight:100,idealHeight:150,maxHeight:180)
            .accessibilityLabel(AppText.text("转换历史"))
    }

    private func historyRow(_ task: DeliveryTask) -> some View {
        VStack(alignment:.leading,spacing:5) {
            HStack(spacing:8) {
                Text(task.created,style:.date)
                Text(task.created,style:.time).foregroundColor(.secondary)
            }
            Text(AppText.text((task.results.isEmpty ? "原样导出任务" : task.resultSummary) + " · " + MigrationTargetPreset.current(task.settings).title))
                .font(.callout).foregroundColor(.secondary)
        }.padding(.vertical,6)
    }

    private func taskDetails(_ task: DeliveryTask) -> some View {
        ScrollView {
            VStack(alignment:.leading,spacing:12) {
              Group {
                Label(AppText.text(deliveryStatus(task)),systemImage:task.cancelled ? "stop.circle" : "tray.full")
                    .font(.headline).textSelection(.enabled)
                Text(AppText.text(task.settings.direction.title)).font(.callout).foregroundColor(.secondary)
                if !task.processingSummary.isEmpty { Text(AppText.text(task.processingSummary)).font(.caption).foregroundColor(.secondary) }
                Divider()
                Text(AppText.text("保存位置")).font(.callout.weight(.semibold))
                Text(AppText.text(task.output)).font(.callout).textSelection(.enabled).fixedSize(horizontal:false,vertical:true)
                if task.settings.destination.importsPhotos { Text(AppText.text("相册：") + task.settings.albumTitle).font(.callout) }
                if task.cancelled { Text(AppText.text("仅保留已完成的结果。")).font(.callout).foregroundColor(.secondary) }
              }
              Group {
                if let record = task.originalExport { exportDetails(record) }
                if let record = task.resultExport { exportDetails(record) }
                if !task.results.isEmpty && !filesAvailable(task) {
                    Label(AppText.text("部分文件已移走或清理，请重新转换后重试。"),systemImage:"exclamationmark.triangle")
                        .font(.callout).foregroundColor(.secondary)
                }
                if let error = cleanupError { Text(AppText.text(error)).foregroundColor(.red).textSelection(.enabled) }
              }
            }.frame(maxWidth:.infinity,alignment:.leading).padding(16)
        }.frame(maxHeight:.infinity)
    }

    private func exportDetails(_ record: FileExportRecord) -> some View {
        VStack(alignment:.leading,spacing:6) {
            Divider()
            Text(AppText.text(record.summary)).font(.callout.weight(.semibold))
            Text(AppText.text(record.output)).font(.caption).textSelection(.enabled)
            if let detail = record.detail { Text(AppText.text(detail)).font(.callout).foregroundColor(.secondary).textSelection(.enabled) }
            ForEach(Array(exportLines(record).enumerated()),id:\.offset) { line in
                Text(AppText.text(line.element)).font(.caption).foregroundColor(.secondary).textSelection(.enabled)
            }
        }
    }
    private func exportLines(_ record: FileExportRecord) -> [String] {
        guard let data = record.report,
              let report = try? JSONSerialization.jsonObject(with:data) as? [String:Any],
              let files = report["files"] as? [[String:Any]] else { return [] }
        let titles = ["copied":"已生成副本","resumed":"导出复用","skipped":"已跳过","failed":"失败"]
        return files.map { file in
            let source = file["source"] as? String ?? "项目"
            let state = file["status"] as? String ?? "未完成"
            let detail = file["detail"] as? String ?? ""
            return source + " · " + (titles[state] ?? state) + " · " + detail
        }
    }

    private func taskActions(_ task: DeliveryTask) -> some View {
        HStack(spacing:12) {
            Menu(AppText.text("更多")) {
                Button(AppText.text("移除记录与缓存…"),role:.destructive) { cleanupPresented = true }
                    .disabled(model.isBusy || model.sharing)
            }.fixedSize()
            Spacer(minLength:0)
            Button(AppText.text("查看文件")) { NSWorkspace.shared.activateFileViewerSelecting(existingFiles(task)) }
                .disabled(existingFiles(task).isEmpty)
            Button(AppText.text("导出转换结果…")) { export(task) }
                .disabled(task.results.isEmpty || model.isBusy || model.sharing)
            if task.originalExport != nil {
                Button(AppText.text("继续原样导出")) { originals(task) }.disabled(model.isBusy || model.sharing)
            }
            if canRedeliver(task) {
                Button(AppText.text(task.settings.direction.exportsAndroid ? "再次共享…" : "再次导入图库…")) { retry(task) }
                    .disabled(!filesAvailable(task) || model.isBusy || model.sharing)
            }
        }.padding(16).recordLayout("recent-actions")
    }
}
