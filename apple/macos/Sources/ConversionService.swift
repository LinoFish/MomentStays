import Foundation
import ImageIO

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
                let outputTimes = try PhotoTimeOutputProcessor.make(options:job.options,sources:job.sources,token:token)
                return try NativeBatch.run(sources: job.sources, output: job.output, options: job.options, token: token, scanRows: job.scanRows,
                    existingOutput: { source, target in
                        let latch = OutputDecisionLatch()
                        DispatchQueue.main.async {
                            guard !token.isCancelled else { latch.resolve(.cancel); return }
                            existingOutput(source, target) { latch.resolve($0) }
                        }
                        return latch.wait(token: token)
                    },outputTimes:outputTimes) { update in
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
        var timeChecked = false
        var captureTimeMissing: Bool?
        var privateHDRSupported: Bool?
        var privateHDRReason: String?
        var hdrChecked = false
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
            manufacturer:row.manufacturer,manufacturerDetail:row.manufacturerDetail,confirmedStatic:row.confirmedStatic,version:version,burst:row.burst,captureTimeMissing:row.captureTimeMissing,privateHDRSupported:row.privateHDRSupported,privateHDRReason:row.privateHDRReason)
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
            if version.size > 256 * 1024 * 1024,
               (try RustCore.engine(["mode":"burst-filename","filename":file.lastPathComponent])["candidate"] as? Bool) == true {
                let limited = rebase(NativeBatch.scanFile(file,relative:relative),relative:relative,version:version)
                entry.projections[direction] = limited; result.append(limited); pending.append(limited)
                progress(index + 1,selected.entries.count); continue
            }
            if direction == .toAndroid {
                let versions = try siblings(file)
                if entry.pairVersions != versions { entry.projections[.toAndroid] = nil; entry.pairVersions = versions }
            }
            var row: ScanRow
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
                    row = AndroidTransfer.scanFile(file,relative:relative,bytes:bytes,parsed:parsed,direction:direction.rawValue)
                }
            }
            if !entry.timeChecked {
                entry.captureTimeMissing = SourcePhotoTimeFacts.missing(at:file)
                entry.timeChecked = true
            }
            if row.hdr == "荣耀私有 HDR" && !entry.hdrChecked {
                let facts = try? RustCore.engine(["mode":"honor-hdr-preflight","source":file.path])
                entry.privateHDRSupported = facts?["supported"] as? Bool ?? false
                entry.privateHDRReason = facts?["reason"] as? String ?? "无法验证荣耀私有 HDR 标定配置"
                entry.hdrChecked = true
            }
            row.privateHDRSupported = entry.privateHDRSupported
            row.privateHDRReason = entry.privateHDRReason
            row.captureTimeMissing = entry.captureTimeMissing
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

// Source facts only. Never infer a date or write the input while scanning.
enum SourcePhotoTimeFacts {
    static func missing(at url: URL) -> Bool? {
        guard let image = CGImageSourceCreateWithURL(url as CFURL,nil),
              CGImageSourceGetCount(image) == 1,
              let properties = CGImageSourceCopyPropertiesAtIndex(image,0,nil) as? [String:Any],
              (properties[kCGImagePropertyPixelWidth as String] as? Int ?? 0) > 0,
              (properties[kCGImagePropertyPixelHeight as String] as? Int ?? 0) > 0 else { return nil }
        let exif = properties[kCGImagePropertyExifDictionary as String] as? [String:Any] ?? [:]
        let tiff = properties[kCGImagePropertyTIFFDictionary as String] as? [String:Any] ?? [:]
        let iptc = properties[kCGImagePropertyIPTCDictionary as String] as? [String:Any] ?? [:]
        var dates = [iptc["DateCreated"] as? String,iptc["DigitalCreationDate"] as? String,
                     exif[kCGImagePropertyExifDateTimeOriginal as String] as? String,
                     exif[kCGImagePropertyExifDateTimeDigitized as String] as? String,
                     tiff[kCGImagePropertyTIFFDateTime as String] as? String]
        if let metadata = CGImageSourceCopyMetadataAtIndex(image,0,nil),
           let tag = CGImageMetadataCopyTagMatchingImageProperty(metadata,kCGImagePropertyExifDictionary,kCGImagePropertyExifDateTimeOriginal) {
            dates.append(CGImageMetadataTagCopyValue(tag) as? String)
        }
        return !dates.compactMap { $0 }.contains { !$0.trimmingCharacters(in:.whitespacesAndNewlines).isEmpty }
    }
}

// Runs synchronously on Rust workers, exclusively inside an unpublished resource group.
enum PhotoTimeOutputProcessor {
    static func make(options: ConversionOptions, sources: [URL], token: StopToken) throws -> OutputTimeProcessor? {
        guard let settings = options.photoTime,
              let zoneID = settings["zone"] as? String, let zone = TimeZone(identifier:zoneID),
              let reference = settings["epoch"] as? Double, reference > 0 else { return nil }
        let synthetic = settings["synthetic"] as? Bool ?? false
        let base = Date(timeIntervalSince1970:reference)
        let originalNames = options.photoTimeOriginalNames
        let originalName: (URL) -> String = { originalNames[$0.standardizedFileURL.path] ?? $0.lastPathComponent }
        let files = try NativeBatch.selection(sources).entries.sorted {
            let order = originalName($0).compare(originalName($1),options:[.numeric,.caseInsensitive],locale:Locale(identifier:"en_US_POSIX"))
            return order == .orderedSame ? $0.path < $1.path : order == .orderedAscending
        }
        var fallback: [String:Date] = [:]
        var sourceMissing: [String:Bool] = [:]
        for file in files {
            let key = file.standardizedFileURL.path
            if let missing = SourcePhotoTimeFacts.missing(at:file) {
                sourceMissing[key] = missing
                if missing && PhotoTimeRepair.inferredTime(filename:originalName(file),zone:zone) == nil {
                    fallback[key] = base.addingTimeInterval(Double(fallback.count))
                }
            }
        }
        let frozenFallback = fallback
        let frozenSourceMissing = sourceMissing
        return { stage,source,manifest in
            guard !token.isCancelled else { throw Failure("已停止，时间补全未发布") }
            guard let names = manifest["output_files"] as? [String:String] else { throw Failure("时间补全缺少完整输出资源") }
            let members = (manifest["burst"] as? [String:Any])?["members"] as? [[String:Any]] ?? []
            var records: [[String:Any]] = []
            var dates: [String:Date] = [:]
            for (role,name) in names.sorted(by:{$0.key < $1.key}) {
                guard !name.contains("/"), !name.contains("\\"), name != ".", name != ".." else { throw Failure("时间补全输出路径无效") }
                let output = stage.appendingPathComponent(name)
                guard ["jpg","jpeg","heic","heif"].contains(output.pathExtension.lowercased()) else { continue }
                let origin = members.first(where:{$0["file"] as? String == name}).flatMap { $0["source"] as? String }.map { URL(fileURLWithPath:$0) } ?? source
                let input = PhotoTimeRepair.Input(photo:output,originalFilename:originalName(origin))
                let planned = PhotoTimeRepair.planSecondStep(inputs:[input],now:frozenFallback[origin.standardizedFileURL.path] ?? base,zone:zone)
                if let rejected = planned.rejected.first { throw Failure(rejected.reason) }
                if !planned.preserved.isEmpty {
                    records.append(["file":name,"status":"preserved","source":"existing-metadata"])
                    if let date = captureDate(output,zone:zone) { dates[name] = date }
                    continue
                }
                guard let plan = planned.plans.first else { throw Failure("无法生成照片时间计划") }
                if frozenSourceMissing[origin.standardizedFileURL.path] == false {
                    throw Failure("原片已有非空时间，但当前输出未完整传递；未使用文件名或合成时间覆盖，请检查此处理组合")
                }
                if plan.source == .filenameOrder && !synthetic {
                    records.append(["file":name,"status":"unrecognized","detail":"没有可靠时间；未启用合成排序时间，未写元数据"])
                    continue
                }
                guard role != "motionphoto" else { throw Failure("当前目标动态容器的时间重封装尚未支持；未拆成静态照片或发布不完整结果") }
                let candidate = stage.appendingPathComponent(".time-" + UUID().uuidString + "." + output.pathExtension)
                defer { try? FileManager.default.removeItem(at:candidate) }
                let originalAttributes = try FileManager.default.attributesOfItem(atPath:output.path)
                try PhotoTimeRepair.write(plan,to:candidate,verifyFeatures:verifyFeatures)
                var retainedTimes: [FileAttributeKey:Any] = [:]
                for key in [FileAttributeKey.creationDate,.modificationDate] { retainedTimes[key] = originalAttributes[key] }
                try FileManager.default.setAttributes(retainedTimes,ofItemAtPath:candidate.path)
                guard !token.isCancelled else { throw Failure("已停止，时间补全未发布") }
                // Only this private staging member is replaced; originals and published results stay read-only.
                _ = try FileManager.default.replaceItemAt(output,withItemAt:candidate)
                dates[name] = captureDate(output,zone:zone) ?? plan.date
                records.append(["file":name,"status":"written","source":plan.source.rawValue,"original_filename":originalName(origin),
                                "date":ISO8601DateFormatter().string(from:plan.date),"zone":plan.zone.identifier,
                                "synthetic":plan.source == .filenameOrder])
            }
            var fileTimeRecords: [[String:Any]] = []
            if options.exifFileTimes {
                let sharedDate = (manifest["burst"] == nil) ? names["still"].flatMap { dates[$0] } ?? (dates.count == 1 ? dates.values.first : nil) : nil
                for name in names.values {
                    let file = stage.appendingPathComponent(name)
                    let date = dates[name] ?? sharedDate
                    if let date = date {
                        try FileManager.default.setAttributes([.creationDate:date,.modificationDate:date],ofItemAtPath:file.path)
                        let actual = try FileManager.default.attributesOfItem(atPath:file.path)
                        guard let modified = actual[.modificationDate] as? Date, let created = actual[.creationDate] as? Date,
                              abs(modified.timeIntervalSince(date)) < 1, abs(created.timeIntervalSince(date)) < 1 else {
                            throw Failure("输出文件创建或修改时间未正确写入，未发布")
                        }
                        fileTimeRecords.append(["file":name,"status":"written","date":ISO8601DateFormatter().string(from:date)])
                    } else {
                        fileTimeRecords.append(["file":name,"status":"source-file-time-fallback","detail":"没有可用 EXIF 时间，沿用来源文件时间"])
                    }
                }
            }
            return ["settings":settings,"records":records,"file_times":options.exifFileTimes,"file_time_records":fileTimeRecords]
        }
    }
    private static func captureDate(_ url: URL, zone: TimeZone) -> Date? {
        guard let source = CGImageSourceCreateWithURL(url as CFURL,nil),
              let props = CGImageSourceCopyPropertiesAtIndex(source,0,nil) as? [String:Any] else { return nil }
        let exif = props[kCGImagePropertyExifDictionary as String] as? [String:Any] ?? [:]
        guard let text = exif[kCGImagePropertyExifDateTimeOriginal as String] as? String else { return nil }
        var fixed = zone
        if let offset = exif[kCGImagePropertyExifOffsetTimeOriginal as String] as? String {
            let clean = offset.replacingOccurrences(of:":",with:"")
            if clean.count == 5, let hours = Int(clean.dropFirst().prefix(2)), let minutes = Int(clean.suffix(2)),
               hours <= 14, minutes < 60, let z = TimeZone(secondsFromGMT:(clean.first == "-" ? -1 : 1) * (hours*3600+minutes*60)) { fixed = z }
        }
        return PhotoTimeRepair.formatter(zone:fixed).date(from:text)
    }
    private static func verifyFeatures(_ before: URL, _ after: URL) throws {
        let original = Array(try Data(contentsOf:before)), candidate = Array(try Data(contentsOf:after))
        guard try RustCore.codingHash(original) == RustCore.codingHash(candidate),
              try RustCore.auxiliaryHashes(original) == RustCore.auxiliaryHashes(candidate),
              try RustCore.hdrFeatureFacts(original) == RustCore.hdrFeatureFacts(candidate),
              let a = CGImageSourceCreateWithURL(before as CFURL,nil),
              let b = CGImageSourceCreateWithURL(after as CFURL,nil),
              var pa = CGImageSourceCopyPropertiesAtIndex(a,0,nil) as? [String:Any],
              var pb = CGImageSourceCopyPropertiesAtIndex(b,0,nil) as? [String:Any] else {
            throw Failure("时间补全改变了图像编码或 HDR 资源，未发布")
        }
        let beforeGain = CGImageSourceCopyAuxiliaryDataInfoAtIndex(a,0,kCGImageAuxiliaryDataTypeHDRGainMap) as? [String:Any]
        let afterGain = CGImageSourceCopyAuxiliaryDataInfoAtIndex(b,0,kCGImageAuxiliaryDataTypeHDRGainMap) as? [String:Any]
        if let beforeGain = beforeGain, let afterGain = afterGain {
            guard beforeGain[kCGImageAuxiliaryDataInfoData as String] as? Data == afterGain[kCGImageAuxiliaryDataInfoData as String] as? Data,
                  NSDictionary(dictionary:beforeGain[kCGImageAuxiliaryDataInfoDataDescription as String] as? [String:Any] ?? [:]).isEqual(to:afterGain[kCGImageAuxiliaryDataInfoDataDescription as String] as? [String:Any] ?? [:]) else {
                throw Failure("时间补全改变了 HDR 增益图，未发布")
            }
            let beforeMetadata = beforeGain[kCGImageAuxiliaryDataInfoMetadata as String]
            let afterMetadata = afterGain[kCGImageAuxiliaryDataInfoMetadata as String]
            if let beforeMetadata = beforeMetadata, let afterMetadata = afterMetadata {
                guard CFGetTypeID(beforeMetadata as CFTypeRef) == CGImageMetadataGetTypeID(),
                      CFGetTypeID(afterMetadata as CFTypeRef) == CGImageMetadataGetTypeID() else { throw Failure("HDR 元数据类型无法验证") }
                let x = CGImageMetadataCreateXMPData(beforeMetadata as! CGImageMetadata,nil) as Data?
                let y = CGImageMetadataCreateXMPData(afterMetadata as! CGImageMetadata,nil) as Data?
                guard x == y else { throw Failure("时间补全改变了 HDR 增益图元数据，未发布") }
            } else if (beforeMetadata != nil) != (afterMetadata != nil) { throw Failure("HDR 增益图元数据未完整保留") }
        } else {
            guard (beforeGain != nil) == (afterGain != nil) else { throw Failure("HDR 辅助资源未完整保留") }
            if !(try RustCore.auxiliaryHashes(original)).isEmpty || (pa[kCGImagePropertyDepth as String] as? Int ?? 8) > 8 {
                throw Failure("系统不能验证此 HDR/辅助资源的时间写入保留，未发布；请关闭补缺或更改处理范围")
            }
        }
        // Compare every property except the explicitly requested date fields and file size.
        for key in [kCGImagePropertyExifDictionary as String,kCGImagePropertyTIFFDictionary as String,kCGImagePropertyIPTCDictionary as String] {
            if var value = pa[key] as? [String:Any] {
                for dateKey in ["DateTimeOriginal","DateTimeDigitized","DateTime","OffsetTime","OffsetTimeOriginal","OffsetTimeDigitized","DateCreated","DigitalCreationDate","DigitalCreationTime","TimeCreated"] { value[dateKey] = nil }
                pa[key] = value
            }
            if var value = pb[key] as? [String:Any] {
                for dateKey in ["DateTimeOriginal","DateTimeDigitized","DateTime","OffsetTime","OffsetTimeOriginal","OffsetTimeDigitized","DateCreated","DigitalCreationDate","DigitalCreationTime","TimeCreated"] { value[dateKey] = nil }
                pb[key] = value
            }
        }
        pa[kCGImagePropertyFileSize as String] = nil; pb[kCGImagePropertyFileSize as String] = nil
        if let value = pa[kCGImagePropertyExifDictionary as String] as? [String:Any], value.isEmpty { pa[kCGImagePropertyExifDictionary as String] = nil }
        if let value = pb[kCGImagePropertyExifDictionary as String] as? [String:Any], value.isEmpty { pb[kCGImagePropertyExifDictionary as String] = nil }
        if let value = pa[kCGImagePropertyTIFFDictionary as String] as? [String:Any], value.isEmpty { pa[kCGImagePropertyTIFFDictionary as String] = nil }
        if let value = pb[kCGImagePropertyTIFFDictionary as String] as? [String:Any], value.isEmpty { pb[kCGImagePropertyTIFFDictionary as String] = nil }
        if let value = pa[kCGImagePropertyIPTCDictionary as String] as? [String:Any], value.isEmpty { pa[kCGImagePropertyIPTCDictionary as String] = nil }
        if let value = pb[kCGImagePropertyIPTCDictionary as String] as? [String:Any], value.isEmpty { pb[kCGImagePropertyIPTCDictionary as String] = nil }
        guard NSDictionary(dictionary:pa).isEqual(to:pb) else { throw Failure("时间补全未能完整保留其他元数据或照片特性，未发布") }
    }
}
