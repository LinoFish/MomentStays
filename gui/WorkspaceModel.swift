import Foundation
import Combine

enum TaskPhase: String {
    case idle, library, scanning, projecting, ready, converting, awaitingDecision, delivering, stopping, completed, stopped, failed
    var isBusy: Bool { [.library, .scanning, .projecting, .converting, .awaitingDecision, .delivering, .stopping].contains(self) }
}

enum PhotoState: String, CaseIterable {
    case ready, inspect, converted, resumed, skipped, failed, `static`
    var title: String {
        switch self {
        case .ready: return "可转换"
        case .inspect: return "需检查"
        case .converted: return "已完成"
        case .resumed: return "已有结果"
        case .skipped: return "已跳过"
        case .static: return "静态照片"
        case .failed: return "失败"
        }
    }
    var symbol: String {
        switch self {
        case .ready: return "circle"
        case .inspect: return "exclamationmark.triangle"
        case .converted: return "checkmark.circle.fill"
        case .resumed: return "checkmark.circle"
        case .skipped: return "minus.circle"
        case .static: return "photo"
        case .failed: return "exclamationmark.circle.fill"
        }
    }
}

struct PhotoQueueItem: Identifiable {
    let source: URL
    let relative: String
    let isMotion: Bool
    let manufacturer: String
    let manufacturerDetail: String
    let hdr: String
    let cover: String
    let scanDetail: String
    let row: ScanRow
    let initialState: PhotoState
    var resultState: PhotoState?
    var resultDetail: String?
    var id: String { source.path }
    var name: String { source.lastPathComponent }
    var state: PhotoState { resultState ?? initialState }
    var detail: String { resultDetail?.isEmpty == false ? resultDetail! : scanDetail }

    func category(settings: ConversionSettings) -> PhotoState {
        if [.converted,.resumed].contains(state) { return .converted }
        if state == .static { return .static }
        if [.inspect,.failed].contains(state) { return .inspect }
        if isMotion && cover == "未记录" && settings.direction == .toApple && settings.missingTime == .error &&
            settings.exportMode != .auxiliaryOnly && settings.coverIndex(for:source) == nil { return .inspect }
        return .ready
    }

    init(_ row: ScanRow) {
        self.row = row
        source = row.source; relative = row.relative; isMotion = row.isMotion
        manufacturer = row.manufacturer; manufacturerDetail = row.manufacturerDetail
        hdr = row.hdr; cover = row.cover; scanDetail = row.detail
        initialState = row.isMotion ? .ready : row.needsInspection ? .inspect : .static
    }
}

struct ReportEntry: Identifiable {
    let id: Int
    let source: String
    let output: String?
    let state: PhotoState
    let detail: String
}

struct ConversionReport {
    let url: URL?
    let counts: [String: Int]
    let entries: [ReportEntry]
    let cancelled: Bool
    init(_ outcome: BatchOutcome) {
        url = outcome.report; counts = outcome.counts; cancelled = outcome.cancelled
        entries = ((outcome.reportData["files"] as? [[String: Any]]) ?? []).enumerated().map { index, file in
            ReportEntry(id: index, source: file["source"] as? String ?? "", output: file["output"] as? String,
                state: file["classification"] as? String == "static" ? .static : PhotoState(rawValue: file["status"] as? String ?? "") ?? .failed,
                detail: file["detail"] as? String ?? file["error"] as? String ?? "")
        }
    }
    var summary: String {
        let staticCount = entries.filter { $0.state == .static }.count
        return "成功 \(counts["converted", default:0]) · 已有 \(counts["resumed", default:0]) · 静态 \(staticCount) · 跳过 \(max(0,counts["skipped", default:0] - staticCount)) · 失败 \(counts["failed", default:0])"
    }
}

// All mutations are main-queue confined. No NSWindow, photo picker, or file dialog
// is owned here; a future iOS host supplies its own platform actions/presenter.
final class WorkspaceModel: ObservableObject {
    @Published private(set) var settings = ConversionSettings()
    @Published private(set) var phase: TaskPhase = .idle
    @Published private(set) var sources: [URL] = []
    @Published private(set) var sourceLabel = "尚未选择原片"
    @Published private(set) var output: URL?
    @Published private(set) var items: [PhotoQueueItem] = []
    @Published var selection: String?
    @Published private(set) var status = "选择照片或文件夹，开始转换"
    @Published private(set) var notice: String?
    @Published private(set) var currentFile = ""
    @Published private(set) var progress: Double?
    @Published private(set) var report: ConversionReport?
    @Published var errorMessage: String?
    @Published var deliverySummary = ""
    @Published var librarySummary = ""
    @Published var libraryMonitoring = false
    @Published var shareReady = false
    @Published var sharing = false
    @Published var recentTasksAvailable = false
    @Published var recentOutput: URL?
    @Published var settingsPresented = false
    @Published var advancedExpanded = false
    @Published var detailExpanded = false
    @Published var previewVisible = false
    @Published var sourceMenuRequested = false
    private(set) var isPreview = false

    var onConversionFinished: (BatchOutcome, ConversionSettings) -> Void = { _, _ in }
    var presentExistingOutput: ExistingOutputPresenter = { _, _, reply in reply(.cancel) }
    var cancelPlatformActivity: () -> Void = {}
    private let service: ConversionServing
    private var token = StopToken()
    private var operation = UUID()
    private var scanComplete = false
    private var inspectionActive = false
    private var rowIndices: [String: Int] = [:]
    private var sourceOrigins: [String: LibraryOrigin] = [:]
    private var coverVersions: [String:SourceVersion] = [:]
    private struct OutputChoice {
        let destination: OutputDestination; let confirmed: Bool?; let folder: URL?
        let albumID: String?; let albumTitle: String
    }
    private var outputChoices: [ConversionDirection:OutputChoice] = [:]

    init(service: ConversionServing = ConversionService()) { self.service = service }
    var isBusy: Bool { phase.isBusy }
    var outputIsChosen: Bool { settings.destination != .ask && (settings.direction != .toApple || settings.outputConfirmed == true) && (settings.destination.usesCache || output != nil) }
    var canStart: Bool { !isBusy && !sharing && scanComplete && outputIsChosen && items.contains { $0.isMotion } }
    var canSwitchDirection: Bool { !sharing && (!isBusy || phase == .scanning || phase == .projecting) }
    var canAppendSources: Bool { !sharing && !isPreview && (!isBusy || inspectionActive && [.scanning,.projecting,.stopping].contains(phase)) }
    var canRescan: Bool { !isBusy && !sharing && !sources.isEmpty && !isPreview }
    var outputToOpen: URL? { settings.destination.usesCache ? recentOutput : output }
    var canOpenOutput: Bool {
        guard !isPreview, let url = outputToOpen else { return false }
        return (try? url.resourceValues(forKeys: [.isDirectoryKey]).isDirectory) == true
    }
    var outputOpenHelp: String { canOpenOutput ? "打开输出文件夹" : "输出文件夹尚未创建，转换后可打开" }
    var canStop: Bool { isBusy && phase != .stopping && !isPreview }
    var selectedItem: PhotoQueueItem? { items.first { $0.id == selection } }
    var motionCount: Int { items.filter { $0.isMotion }.count }
    func category(_ item: PhotoQueueItem) -> PhotoState { item.category(settings:settings) }
    var issueCount: Int { items.filter { category($0) == .inspect }.count }

    func selectCover(_ index: Int?, for source: URL) {
        guard !isBusy, !sharing, settings.direction == .toApple, index == nil || index! >= 0 else { return }
        let key = NativeBatch.normalized(source).path
        coverVersions[key] = index == nil ? nil : SourceVersion.read(source)
        updateSettings {
            var choices = $0.coverSelections ?? [:]
            choices[key] = index
            $0.coverSelections = choices.isEmpty ? nil : choices
            if index != nil { $0.track = .primary }
        }
    }

    func updateSettings(_ change: (inout ConversionSettings) -> Void) {
        guard canSwitchDirection else { return }
        notice = nil
        var next = settings; change(&next)
        let directionChanged = next.direction != settings.direction
        guard !isBusy || directionChanged else { return }
        if directionChanged {
            outputChoices[settings.direction] = OutputChoice(destination:settings.destination,confirmed:settings.outputConfirmed,folder:output,albumID:settings.albumID,albumTitle:settings.albumTitle)
            if let saved = outputChoices[next.direction] {
                next.destination = saved.destination; next.outputConfirmed = saved.confirmed
                next.albumID = saved.albumID; next.albumTitle = saved.albumTitle; output = saved.folder
            } else {
                next.destination = next.direction == .toApple ? .ask : .folder
                next.outputConfirmed = false
            }
        }
        next.normalize()
        let changed = next.options.json as NSDictionary != settings.options.json as NSDictionary || next.coverSelections != settings.coverSelections
        settings = next
        if changed {
            report = nil
            for index in items.indices { items[index].resultState = nil; items[index].resultDetail = nil }
        }
        if directionChanged && !sources.isEmpty && !isPreview {
            token.cancel(); refresh(projection:true,preservingResults:false)
        }
    }

    func chooseSources(_ urls: [URL], label: String? = nil, defaultOutput: URL? = nil) {
        guard !isBusy, !sharing, !isPreview, let first = urls.first else { return }
        sources = Array(Set(urls.filter { $0.isFileURL }.map { NativeBatch.normalized($0) })).sorted { $0.path < $1.path }
        guard !sources.isEmpty else { return }
        sourceLabel = label ?? (sources.count == 1 ? sources[0].lastPathComponent : "\(sources.count) 个文件／文件夹")
        let base = NativeBatch.root(NativeBatch.normalized(first))
        output = defaultOutput ?? base.deletingLastPathComponent().appendingPathComponent(base.lastPathComponent + (settings.direction.exportsAndroid ? " Motion Photos" : " Live Photos"))
        items = []; selection = nil; report = nil; sourceOrigins = [:]
        settings.coverSelections = nil
        coverVersions = [:]
        rescan()
    }

    // Manual additions extend the current batch. Automatic monitoring keeps its
    // existing replacement semantics through chooseSources(_:).
    func appendSources(_ urls: [URL], label: String? = nil, defaultOutput: URL? = nil, origins: [String: LibraryOrigin] = [:]) {
        guard canAppendSources else { return }
        func identity(_ origin: LibraryOrigin) -> String { "\(origin.assetID)|\(origin.version)|\(origin.edited)" }
        var known = Set(sourceOrigins.values.map(identity))
        let incoming = Set(urls.filter { $0.isFileURL }.map { NativeBatch.normalized($0) }.filter { url in
            guard let origin = origins[url.path] else { return true }
            return known.insert(identity(origin)).inserted
        })
        let merged = Set(sources).union(incoming).sorted { $0.path < $1.path }
        guard !merged.isEmpty else { return }
        guard merged != sources else { let message = "这些来源已在列表中"; notice = message; status = message; return }
        if let output = output, !settings.destination.usesCache {
            do { try NativeBatch.validateOutput(sources:merged,output:output) }
            catch { errorMessage = error.localizedDescription; return }
        }
        let wasEmpty = sources.isEmpty
        sources = merged
        sourceOrigins.merge(origins.filter { incoming.contains(URL(fileURLWithPath:$0.key)) },uniquingKeysWith:{ _,new in new })
        sourceLabel = wasEmpty ? (label ?? (merged.count == 1 ? merged[0].lastPathComponent : "\(merged.count) 个文件／文件夹")) : "多处来源 · \(merged.count) 项"
        if output == nil, let first = merged.first {
            let base = NativeBatch.root(first)
            output = defaultOutput ?? base.deletingLastPathComponent().appendingPathComponent(base.lastPathComponent + (settings.direction.exportsAndroid ? " Motion Photos" : " Live Photos"))
        }
        shareReady = false; deliverySummary = ""
        token.cancel()
        refresh(projection:false,preservingResults:true)
    }

    func clearSources() {
        guard !isBusy, !sharing else { return }
        operation = UUID(); token.cancel(); service.clearInspections()
        sources = []; items = []; selection = nil; rowIndices = [:]; sourceOrigins = [:]; scanComplete = false
        sourceLabel = "尚未选择原片"; settings.coverSelections = nil
        coverVersions = [:]
        phase = .idle; status = "选择照片或文件夹，开始转换"
        report = nil; errorMessage = nil; notice = nil; currentFile = ""; progress = nil
        deliverySummary = ""; librarySummary = ""; shareReady = false; detailExpanded = false
    }

    func setOutput(_ url: URL) {
        guard !isBusy, !sharing, !isPreview else { return }
        do {
            let destination = NativeBatch.normalized(url)
            try NativeBatch.validateOutput(sources: sources, output: destination)
            guard (try? destination.resourceValues(forKeys: [.isDirectoryKey]).isDirectory) == true else {
                throw Failure("请选择一个输出文件夹")
            }
            if output != destination {
                report = nil
                for index in items.indices { items[index].resultState = nil; items[index].resultDetail = nil }
            }
            output = destination; notice = nil
            if settings.direction == .toApple {
                if settings.destination == .ask { settings.destination = .folder }
                settings.outputConfirmed = true
            }
        } catch { errorMessage = error.localizedDescription }
    }

    func confirmOutput(destination: OutputDestination, folder: URL?, albumID: String?, albumTitle: String?) {
        guard !isBusy, !sharing, settings.direction == .toApple,
              [.folder,.photos,.folderAndPhotos].contains(destination),
              settings.exportMode != .auxiliaryOnly || destination == .folder else { return }
        if destination != .photos {
            guard let folder = folder else { return }
            do {
                try NativeBatch.validateOutput(sources:sources,output:folder)
                guard try folder.resourceValues(forKeys:[.isDirectoryKey]).isDirectory == true else { throw Failure("请选择一个输出文件夹") }
            } catch { errorMessage = error.localizedDescription; return }
        }
        if destination.importsPhotos, albumTitle?.trimmingCharacters(in:.whitespacesAndNewlines).isEmpty != false { return }
        updateSettings {
            $0.destination = destination; $0.outputConfirmed = true
            if let albumTitle = albumTitle { $0.albumTitle = albumTitle; $0.albumID = albumID }
        }
        if let folder = folder { output = NativeBatch.normalized(folder) }
    }

    @discardableResult private func begin(_ phase: TaskPhase, message: String) -> UUID {
        operation = UUID(); token = StopToken()
        self.phase = phase; status = message; currentFile = ""; progress = nil; errorMessage = nil; notice = nil
        return operation
    }

    func rescan(preservingResults: Bool = false) {
        guard canRescan else { return }
        refresh(projection:false,preservingResults:preservingResults)
    }
    private func invalidateCoverSelections(_ sources:Set<String>) {
        for source in sources { coverVersions.removeValue(forKey:source) }
        guard var choices = settings.coverSelections else { return }
        let count = choices.count
        for source in sources { choices.removeValue(forKey:source) }
        if choices.count != count { settings.coverSelections = choices.isEmpty ? nil : choices }
    }
    private func refresh(projection:Bool, preservingResults:Bool) {
        let versions = Dictionary(items.compactMap { item in item.row.version.map { (item.id,$0) } },uniquingKeysWith:{ first,_ in first }).merging(coverVersions,uniquingKeysWith:{ _,cover in cover })
        let changed = Set(versions.compactMap { SourceVersion.read(URL(fileURLWithPath:$0.key)) != $0.value ? $0.key : nil })
        invalidateCoverSelections(changed)
        func changedSources(_ rows:[ScanRow]) -> Set<String> {
            Set(rows.compactMap { row in
                guard let previous = versions[row.source.path], previous != row.version else { return nil }
                return row.source.path
            })
        }
        let retained = preservingResults ? Dictionary(items.map { ($0.id,$0) },uniquingKeysWith:{ first,_ in first }) : [:]
        func queueItem(_ row: ScanRow) -> PhotoQueueItem {
            var item = PhotoQueueItem(row)
            if let old = retained[item.id], old.row.version != nil, old.row.version == row.version { item.resultState = old.resultState; item.resultDetail = old.resultDetail }
            return item
        }
        let phase: TaskPhase = projection ? .projecting : .scanning
        let id = begin(phase, message: projection ? "正在切换转换方向…" : "正在检查照片…"), currentToken = token
        report = nil; scanComplete = false; inspectionActive = true; rowIndices = [:]
        if !preservingResults { items = [] }
        else { items.removeAll { changed.contains($0.id) } }
        var streamed = Dictionary(items.map { ($0.id,$0) },uniquingKeysWith:{ first,_ in first })
        func merge(_ rows:[ScanRow]) -> [PhotoQueueItem] {
            for row in rows { streamed[row.source.path] = queueItem(row) }
            return streamed.values.sorted { $0.relative.localizedStandardCompare($1.relative) == .orderedAscending }
        }
        let inspect = projection ? service.project : service.scan
        inspect(sources,settings.direction,currentToken, { [weak self] rows in
            guard let self = self, self.operation == id, self.phase == phase else { return }
            self.invalidateCoverSelections(changedSources(rows))
            self.items = merge(rows)
        }, { [weak self] index, total in
            guard let self = self, self.operation == id, self.phase == phase else { return }
            self.status = projection ? "正在检查来源变化 \(index) / \(total)" : "正在检查 \(index) / \(total)"
            self.progress = total > 0 ? Double(index) / Double(total) : nil
        }, { [weak self] result in
            guard let self = self, self.operation == id else { return }
            self.inspectionActive = false
            switch result {
            case .success(let rows):
                self.invalidateCoverSelections(changedSources(rows))
                if !currentToken.isCancelled { self.invalidateCoverSelections(Set(versions.keys).subtracting(rows.map { $0.source.path })) }
                self.scanComplete = !currentToken.isCancelled
                self.items = currentToken.isCancelled ? merge(rows).filter { $0.row.version == nil || SourceVersion.read($0.source) == $0.row.version } : rows.map(queueItem)
                self.rowIndices = Dictionary(self.items.enumerated().map { ($0.element.relative, $0.offset) }, uniquingKeysWith: { first, _ in first })
                if !self.items.contains(where: { $0.id == self.selection }) { self.selection = nil }
                self.phase = currentToken.isCancelled ? .stopped : .ready
                self.status = currentToken.isCancelled ? "扫描已停止，可重新扫描" : self.motionCount > 0 ? "已就绪 · \(self.motionCount) 张可转换" : "没有可转换的动态照片"
                self.progress = nil
            case .failure(let error): self.fail(error, cancelled: currentToken.isCancelled)
            }
        })
    }

    func start(outputOverride: URL? = nil) {
        guard canStart, !isPreview, let output = outputOverride ?? output else { return }
        invalidateCoverSelections(Set(coverVersions.compactMap { SourceVersion.read(URL(fileURLWithPath:$0.key)) != $0.value ? $0.key : nil }))
        do { try NativeBatch.validateOutput(sources: sources, output: output) }
        catch { errorMessage = error.localizedDescription; return }
        let capturedSettings = settings
        let job = ConversionJob(sources: sources, output: output, options: capturedSettings.options, scanRows: items.map(\.row))
        let id = begin(.converting, message: "正在准备转换…"), currentToken = token
        report = nil; progress = 0
        for index in items.indices { items[index].resultState = nil; items[index].resultDetail = nil }
        service.run(job: job, token: currentToken, existingOutput: { [weak self] source, target, reply in
            guard let self = self, self.operation == id, !currentToken.isCancelled else { reply(.cancel); return }
            self.phase = .awaitingDecision; self.status = "等待处理已有输出"
            self.presentExistingOutput(source, target) { [weak self] decision in
                if let self = self, self.operation == id, self.phase == .awaitingDecision {
                    self.phase = .converting; self.status = "正在转换…"
                }
                reply(currentToken.isCancelled ? .cancel : decision)
            }
        }, progress: { [weak self] update in
            guard let self = self, self.operation == id else { return }
            if self.phase != .stopping && self.phase != .awaitingDecision {
                self.status = "转换 \(update.index) / \(update.total)；\(self.items.filter { $0.state == .static }.count) 张静态已跳过"
                self.currentFile = update.relative
            }
            self.progress = Double(update.index) / Double(max(update.total, 1))
            if let index = self.rowIndices[update.relative], self.items.indices.contains(index) {
                self.items[index].resultState = PhotoState(rawValue: update.status) ?? .failed
                self.items[index].resultDetail = update.detail
            }
        }, completion: { [weak self] result in
            guard let self = self, self.operation == id else { return }
            self.cancelPlatformActivity()
            self.currentFile = ""; self.progress = nil
            switch result {
            case .success(let outcome):
                let report = ConversionReport(outcome); self.report = report
                self.phase = outcome.cancelled ? .stopped : .completed
                self.status = outcome.cancelled ? "任务已停止" : report.counts["failed", default: 0] > 0 ? "已完成，部分文件需要检查" : "转换完成"
                self.onConversionFinished(outcome, capturedSettings)
            case .failure(let error): self.fail(error, cancelled: currentToken.isCancelled)
            }
        })
    }

    func stop() {
        guard canStop else { return }
        token.cancel(); phase = .stopping
        status = "正在停止，等待资源清理…"
        cancelPlatformActivity()
    }

    func beginLibraryImport() -> UUID? {
        guard settings.direction != .androidToAndroid, !isBusy, !sharing, !isPreview else { return nil }
        return begin(.library, message: "正在选择图库资源…")
    }

    func finishLibraryImport(id: UUID, urls: [URL], error: Error?, output: URL, direction: ConversionDirection = .toAndroid, appending: Bool = true, origins: [String: LibraryOrigin] = [:]) {
        guard operation == id else { return }
        let cancelled = token.isCancelled
        phase = items.isEmpty ? .idle : .ready; progress = nil
        guard !cancelled else { phase = .stopped; status = "图库读取已停止"; return }
        if let error = error { fail(error, cancelled: false); return }
        guard !urls.isEmpty else { status = items.isEmpty ? "选择照片或文件夹，开始转换" : "已取消图库选择"; return }
        // Commit the direction only after import succeeds; cancel preserves the current queue.
        var next = settings; next.direction = direction; next.normalize(); settings = next
        if appending { appendSources(urls,label:"系统图库 · \(urls.count) 张",defaultOutput:output,origins:origins) }
        else { chooseSources(urls,label:"系统图库 · \(urls.count) 张",defaultOutput:output); sourceOrigins = origins }
    }

    func updateLibraryStatus(_ message: String) {
        guard phase == .library else { return }
        status = message
    }

    func beginDelivery(_ message: String) -> StopToken {
        phase = .delivering; token = StopToken(); status = message; progress = nil; currentFile = ""; errorMessage = nil; notice = nil
        return token
    }
    func deliveryProgress(_ message: String) { if phase == .delivering { status = message } }
    func finishDelivery(_ message: String, error: Error? = nil) {
        deliverySummary = message; status = token.isCancelled ? "任务已停止，已完成结果保留" : (report?.counts["failed", default: 0] ?? 0) > 0 ? "已完成，部分文件需要检查" : "转换及交付处理完成"
        phase = token.isCancelled ? .stopped : .completed; progress = nil
        if let error = error { errorMessage = error.localizedDescription }
    }

    private func fail(_ error: Error, cancelled: Bool) {
        phase = cancelled ? .stopped : .failed; progress = nil; currentFile = ""
        status = cancelled ? "任务已停止" : "任务未完成"
        if !cancelled { errorMessage = error.localizedDescription }
    }

    #if UI_DIAGNOSTICS
    func loadPreviewLocations() {
        sourceLabel = "旅行照片与家庭相册_用于验证长来源名称截断显示"
        output = URL(fileURLWithPath:"/转换结果/旅行照片与家庭相册_用于验证长输出文件夹名称")
    }
    func loadPreview(count: Int, progressPreview: Bool) {
        isPreview = true
        items = (0..<count).map { index in
            let names = ["海边日落", "城市夜景", "山间清晨", "街角咖啡", "一张用于检查中文长文件名显示与截断方式的动态照片"]
            let row = ScanRow(source: URL(fileURLWithPath: "/示例照片/\(names[index % names.count])_\(index + 1).jpg"), relative: "示例_\(index + 1).jpg", isMotion: true,
                hdr: index % 3 == 0 ? "Ultra HDR" : "SDR", cover: "0.400000 秒", detail: "示例数据 · 原始图像与视频保持配对", manufacturer: ["Google", "小米", "三星"][index % 3])
            return PhotoQueueItem(row)
        }
        sources = count == 0 ? [] : [URL(fileURLWithPath: "/示例照片")]
        sourceLabel = count == 0 ? "尚未选择原片" : "示例照片 · \(count) 张"
        output = count == 0 ? nil : URL(fileURLWithPath: settings.direction.exportsAndroid ? "/转换结果/Motion Photos" : "/转换结果/Live Photos")
        phase = progressPreview ? .converting : count == 0 ? .idle : .ready
        status = progressPreview ? "已处理 12 / 32" : count == 0 ? "选择照片或文件夹，开始转换" : "已就绪 · \(count) 张可转换"
        progress = progressPreview ? 0.375 : nil
        currentFile = progressPreview ? "城市夜景_12.jpg" : ""
    }
    #endif
}
