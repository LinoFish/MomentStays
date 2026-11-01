import Foundation

struct ConversionJob {
    let sources: [URL]
    let output: URL
    let options: ConversionOptions
    var scanRows: [ScanRow]? = nil
}

typealias ExistingOutputPresenter = (URL, URL, @escaping (ExistingOutputDecision) -> Void) -> Void

// Completion/progress/presentation callbacks are delivered on the main queue.
// Implementations own execution, never the lifetime of a SwiftUI View.
protocol ConversionServing {
    func scan(sources: [URL], direction: ConversionDirection, token: StopToken, rowsReady: @escaping ([ScanRow]) -> Void,
              progress: @escaping (Int, Int) -> Void, completion: @escaping (Result<[ScanRow], Error>) -> Void)
    func project(sources: [URL], direction: ConversionDirection, token: StopToken, rowsReady: @escaping ([ScanRow]) -> Void, progress: @escaping (Int,Int) -> Void, completion: @escaping (Result<[ScanRow],Error>) -> Void)
    func clearInspections()
    func run(job: ConversionJob, token: StopToken, existingOutput: @escaping ExistingOutputPresenter,
             progress: @escaping (BatchProgress) -> Void, completion: @escaping (Result<BatchOutcome, Error>) -> Void)
}

extension ConversionServing {
    func project(sources:[URL], direction:ConversionDirection, token:StopToken, rowsReady:@escaping ([ScanRow])->Void, progress:@escaping (Int,Int)->Void, completion:@escaping (Result<[ScanRow],Error>)->Void) {
        scan(sources:sources,direction:direction,token:token,rowsReady:rowsReady,progress:progress,completion:completion)
    }
    func clearInspections() {}
}

// Safe even if the user cancels while the platform sheet is still being presented.
// Cancellation can release the worker without depending on a sheet's callback.
final class OutputDecisionLatch {
    private let lock = NSLock()
    private let semaphore = DispatchSemaphore(value: 0)
    private var decision: ExistingOutputDecision?

    func resolve(_ value: ExistingOutputDecision) {
        lock.lock()
        guard decision == nil else { lock.unlock(); return }
        decision = value
        lock.unlock()
        semaphore.signal()
    }

    func wait(token: StopToken) -> ExistingOutputDecision {
        while semaphore.wait(timeout: .now() + 0.1) == .timedOut {
            if token.isCancelled { resolve(.cancel) }
        }
        lock.lock(); defer { lock.unlock() }
        return token.isCancelled ? .cancel : decision ?? .cancel
    }
}

final class ConversionService: ConversionServing {
    private let worker = DispatchQueue(label: "app.motionlive.conversion", qos: .userInitiated)
    let inspections = SourceInspectionCache()

    private func inspectSources(sources: [URL], direction: ConversionDirection, token: StopToken, rowsReady: @escaping ([ScanRow]) -> Void,
              progress: @escaping (Int, Int) -> Void, completion: @escaping (Result<[ScanRow], Error>) -> Void) {
        worker.async {
            var lastUpdate = Date.distantPast
            let result = Result {
                try self.inspections.rows(sources:sources,direction:direction,token:token,rowsReady: { rows in
                    DispatchQueue.main.async { rowsReady(rows) }
                }) { index, total in
                    let now = Date()
                    if index == total || now.timeIntervalSince(lastUpdate) >= 0.1 {
                        lastUpdate = now
                        DispatchQueue.main.async { progress(index, total) }
                    }
                }
            }
            DispatchQueue.main.async { completion(result) }
        }
    }

    func scan(sources:[URL], direction:ConversionDirection, token:StopToken, rowsReady:@escaping ([ScanRow])->Void, progress:@escaping (Int,Int)->Void, completion:@escaping (Result<[ScanRow],Error>)->Void) {
        inspectSources(sources:sources,direction:direction,token:token,rowsReady:rowsReady,progress:progress,completion:completion)
    }
    func project(sources:[URL], direction:ConversionDirection, token:StopToken, rowsReady:@escaping ([ScanRow])->Void, progress:@escaping (Int,Int)->Void, completion:@escaping (Result<[ScanRow],Error>)->Void) {
        inspectSources(sources:sources,direction:direction,token:token,rowsReady:rowsReady,progress:progress,completion:completion)
    }
    func clearInspections() { worker.async { self.inspections.clear() } }

    func run(job: ConversionJob, token: StopToken, existingOutput: @escaping ExistingOutputPresenter,
             progress: @escaping (BatchProgress) -> Void, completion: @escaping (Result<BatchOutcome, Error>) -> Void) {
        worker.async {
            let result = Result {
                try NativeBatch.run(sources: job.sources, output: job.output, options: job.options, token: token, scanRows: job.scanRows,
                    existingOutput: { source, target in
                        let latch = OutputDecisionLatch()
                        DispatchQueue.main.async {
                            guard !token.isCancelled else { latch.resolve(.cancel); return }
                            existingOutput(source, target) { latch.resolve($0) }
                        }
                        return latch.wait(token: token)
                    }) { update in
                        // Every per-file result matters; don't drop terminal rows when batching progress.
                        DispatchQueue.main.async { progress(update) }
                    }
            }
            DispatchQueue.main.async { completion(result) }
        }
    }
}

// Worker-confined source inspections. Payload snapshots are private temporary Data,
// rather than retaining every photo/video in RAM. Direction switches only enumerate
// file versions and fill missing projections; they never reparse a valid source.
final class SourceInspectionCache {
    private struct Stamp: Equatable { let url: URL; let version: SourceVersion? }
    private struct Snapshot: Codable {
        let still: Data; let video: Data; let timestamp: Int?; let timestampTag: String?
        let ultra: Bool; let coverFrame: Int?; let format: String
        init(_ photo:NativeMotionPhoto) {
            still = Data(photo.still); video = Data(photo.video); timestamp = photo.timestamp
            timestampTag = photo.timestampTag; ultra = photo.ultra; coverFrame = photo.coverFrame; format = photo.format
        }
        var photo: NativeMotionPhoto { NativeMotionPhoto(still:Array(still),video:Array(video),timestamp:timestamp,timestampTag:timestampTag,ultra:ultra,coverFrame:coverFrame,format:format) }
    }
    private final class Entry {
        let version: SourceVersion
        var payload: URL?
        var parseError: String?
        var neutralReady = false
        var projections: [ConversionDirection:ScanRow] = [:]
        var pairVersions: [Stamp]?
        init(_ version:SourceVersion) { self.version = version }
    }
    private let folder = FileManager.default.temporaryDirectory.appendingPathComponent("MomentStays-inspections-" + UUID().uuidString)
    private var entries: [URL:Entry] = [:]
    private struct Inventory { let directories:[Stamp]; let files:[URL] }
    private var inventories: [URL:Inventory] = [:]
    private var directoryListings: [URL:(version:SourceVersion,files:[URL])] = [:]
    private(set) var enumerationCount = 0
    private var movieSelections: [URL:(versions:[Stamp],images:[URL])] = [:]
    private(set) var parseCount = 0
    private(set) var pairValidationCount = 0
    private(set) var transferValidationCount = 0
    private(set) var sourceReadCount = 0
    private(set) var projectionHitCount = 0
    deinit { try? FileManager.default.removeItem(at:folder) }
    func clear() {
        entries.removeAll(); movieSelections.removeAll(); inventories.removeAll(); directoryListings.removeAll()
        try? FileManager.default.removeItem(at:folder)
    }
    private func directoryFiles(_ directory:URL) throws -> [URL] {
        guard let version = SourceVersion.readDirectory(directory) else { throw Failure("来源文件夹已删除或不可读取") }
        if let previous = directoryListings[directory], previous.version == version { return previous.files }
        let files = try FileManager.default.contentsOfDirectory(at:directory,includingPropertiesForKeys:nil)
        directoryListings[directory] = (version,files)
        return files
    }
    private func siblings(_ file:URL, images:Bool = false) throws -> [Stamp] {
        let extensions = images ? ["jpg","jpeg","heic","heif","mov","mp4"] : ["mov","mp4"]
        return try directoryFiles(file.deletingLastPathComponent())
            .filter { extensions.contains($0.pathExtension.lowercased()) }.sorted { $0.path < $1.path }
            .map { Stamp(url:NativeBatch.normalized($0),version:SourceVersion.read($0)) }
    }
    private func sourceFiles(_ source:URL, token:StopToken) throws -> [URL] {
        guard let rootVersion = SourceVersion.readDirectory(source) else { return try NativeBatch.files(source) }
        if let previous = inventories[source], previous.directories.allSatisfy({ $0.version != nil && SourceVersion.readDirectory($0.url) == $0.version }) { return previous.files }
        enumerationCount += 1
        var directories = [Stamp(url:source,version:rootVersion)], files: [URL] = [], errors: [Error] = []
        guard let iterator = FileManager.default.enumerator(at:source,includingPropertiesForKeys:[.isRegularFileKey,.isDirectoryKey,.isSymbolicLinkKey],options:[.skipsHiddenFiles,.skipsPackageDescendants],errorHandler:{ _,error in errors.append(error);return true }) else { throw Failure("无法读取照片文件夹") }
        for case let url as URL in iterator {
            if token.isCancelled { return [] }
            let info = try url.resourceValues(forKeys:[.isRegularFileKey,.isDirectoryKey,.isSymbolicLinkKey])
            if info.isSymbolicLink == true { continue }
            if info.isDirectory == true { directories.append(Stamp(url:url,version:SourceVersion.readDirectory(url))) }
            if info.isRegularFile == true && ["jpg","jpeg","heic","heif"].contains(url.pathExtension.lowercased()) { files.append(NativeBatch.normalized(url)) }
        }
        if let error = errors.first { throw error }
        guard directories.allSatisfy({ $0.version != nil && SourceVersion.readDirectory($0.url) == $0.version }) else { throw Failure("读取期间文件夹发生变化，请重试") }
        inventories[source] = Inventory(directories:directories,files:files)
        return files
    }
    private func selection(_ sources:[URL], token:StopToken) throws -> (base:URL,entries:[URL]) {
        var images: [URL] = []
        for source in sources.map({ NativeBatch.normalized($0) }) {
            guard FileManager.default.fileExists(atPath:source.path) else { continue }
            if ["mov","mp4"].contains(source.pathExtension.lowercased()) {
                let versions = try siblings(source,images:true)
                if let previous = movieSelections[source], previous.versions == versions { images += previous.images }
                else {
                    let selected = try AndroidExport.selection([source]).entries
                    movieSelections[source] = (versions,selected); images += selected
                }
            } else { images += try sourceFiles(source,token:token) }
        }
        if images.isEmpty { return (sources.first.map { NativeBatch.root($0) } ?? folder,[]) }
        return try NativeBatch.selection(images)
    }
    private func rebase(_ row:ScanRow, relative:String, version:SourceVersion) -> ScanRow {
        ScanRow(source:row.source,relative:relative,isMotion:row.isMotion,hdr:row.hdr,cover:row.cover,detail:row.detail,
            manufacturer:row.manufacturer,manufacturerDetail:row.manufacturerDetail,confirmedStatic:row.confirmedStatic,version:version)
    }
    private func neutral(_ file:URL, relative:String, entry:Entry, bytes:[UInt8]) throws -> Result<NativeMotionPhoto,Error> {
        if entry.neutralReady {
            if let message = entry.parseError { return .failure(Failure(message)) }
            if let url = entry.payload, let data = try? Data(contentsOf:url), let snapshot = try? PropertyListDecoder().decode(Snapshot.self,from:data) { return .success(snapshot.photo) }
            entry.neutralReady = false // An externally removed cache is no longer valid.
        }
        parseCount += 1
        let result = Result { try PhotoFormat.parse(bytes) }
        guard SourceVersion.read(file) == entry.version else { throw Failure("检查期间来源发生变化，请重试") }
        entry.projections[.toApple] = NativeBatch.scanFile(file,relative:relative,bytes:bytes,parsed:result)
        switch result {
        case .success(let photo):
            try FileManager.default.createDirectory(at:folder,withIntermediateDirectories:true,attributes:[.posixPermissions:0o700])
            let url = folder.appendingPathComponent(UUID().uuidString + ".plist")
            let encoder = PropertyListEncoder(); encoder.outputFormat = .binary
            try encoder.encode(Snapshot(photo)).write(to:url,options:.atomic)
            entry.payload = url
        case .failure(let error): entry.parseError = error.localizedDescription
        }
        entry.neutralReady = true
        return result
    }
    func rows(sources:[URL], direction:ConversionDirection, token:StopToken, rowsReady:([ScanRow])->Void,
              progress:(Int,Int)->Void) throws -> [ScanRow] {
        let selected = try selection(sources,token:token), active = Set(selected.entries)
        // A cancelled inventory is incomplete and must never evict valid entries.
        if token.isCancelled { return [] }
        for file in Array(entries.keys) where !active.contains(file) {
            if let payload = entries.removeValue(forKey:file)?.payload { try? FileManager.default.removeItem(at:payload) }
        }
        var result: [ScanRow] = []
        var pending: [ScanRow] = []
        var lastFlush = ProcessInfo.processInfo.systemUptime
        defer { if !pending.isEmpty { rowsReady(pending) } }
        for (index,file) in selected.entries.enumerated() {
            if token.isCancelled { break }
            let relative = try NativeBatch.relative(file,root:selected.base)
            guard let version = SourceVersion.read(file) else {
                if let payload = entries.removeValue(forKey:file)?.payload { try? FileManager.default.removeItem(at:payload) }
                progress(index + 1,selected.entries.count); continue
            }
            let entry: Entry
            if let previous = entries[file], previous.version == version { entry = previous }
            else {
                if let payload = entries[file]?.payload { try? FileManager.default.removeItem(at:payload) }
                entry = Entry(version); entries[file] = entry
            }
            if direction == .toAndroid {
                let versions = try siblings(file)
                if entry.pairVersions != versions { entry.projections[.toAndroid] = nil; entry.pairVersions = versions }
            }
            let row: ScanRow
            if let cached = entry.projections[direction] { projectionHitCount += 1; row = rebase(cached,relative:relative,version:version) }
            else if direction == .toAndroid {
                pairValidationCount += 1
                row = rebase(AndroidExport.scanFile(file,relative:relative),relative:relative,version:version)
            } else {
                sourceReadCount += 1
                let bytes = Array(try Data(contentsOf:file))
                let parsed = try neutral(file,relative:relative,entry:entry,bytes:bytes)
                if direction == .toApple { row = rebase(entry.projections[.toApple]!,relative:relative,version:version) }
                else {
                    transferValidationCount += 1
                    row = AndroidTransfer.scanFile(file,relative:relative,bytes:bytes,parsed:parsed)
                }
            }
            guard SourceVersion.read(file) == version else {
                entries[file] = nil
                throw Failure("检查期间来源发生变化，请重试")
            }
            if token.isCancelled { break }
            entry.projections[direction] = row
            result.append(row); pending.append(row)
            let now = ProcessInfo.processInfo.systemUptime
            if pending.count >= 64 || now - lastFlush >= 0.05 {
                rowsReady(pending); pending.removeAll(keepingCapacity:true); lastFlush = now
            }
            progress(index + 1,selected.entries.count)
        }
        return result
    }
}
