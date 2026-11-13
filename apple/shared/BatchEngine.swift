import Foundation
import CryptoKit
import Darwin

struct ManualCoverChoice: Codable, Equatable {
    let source_sha256: String
    let source_version: String
    let track_index: Int
    let frame_index: Int
    var json: [String:Any] { ["source_sha256":source_sha256,"source_version":source_version,"track_index":track_index,"frame_index":frame_index] }
}

struct ConversionOptions {
    var direction = "to-apple"
    var android = AndroidOptions()
    var legacyHDR = true
    var missingTime = "error"
    var timeDomain = "auto"
    var videoTrack = "primary"
    var outputMode = "livephoto"
    var repairAuxiliaryDuration = false
    var exportDataFiles = true
    var coverIndex: Int?
    var coverSelections: [String:Int] = [:]
    var candidateDelivery = "static"
    var burstPlans: [BurstGroup] = []
    var burstChoices: [String:BurstChoice] = [:]
    var manualCover: ManualCoverChoice?
    var manualCovers: [String:ManualCoverChoice] = [:]
    var json: [String:Any] {
        if direction == "honor-to-google" { return ["direction":direction,"legacy_hdr":legacyHDR,"hdr_policy":"supported-private-hdr-2","export_data_files":exportDataFiles] }
        if direction == "android-to-android" { return ["direction":direction,"target":"honor","layout":AndroidTransfer.revision,"vendor":"google","container":"jpeg","export_data_files":exportDataFiles] }
        if direction == "to-android" { return ["direction":direction,"android":android.json,"legacy_hdr":legacyHDR,"hdr_policy":"supported-private-hdr-2","time_domain":timeDomain,"export_data_files":exportDataFiles] }
        var result: [String:Any] = ["legacy_hdr": legacyHDR ? "auto" : "preserve", "missing_time": missingTime, "time_domain": timeDomain,
            "video_track":videoTrack,"output_mode":outputMode,"repair_auxiliary_duration":outputMode != "livephoto" && repairAuxiliaryDuration,"export_data_files":exportDataFiles]
        if let index = coverIndex { result["cover_candidate_index"] = index }
        if outputMode == "livephoto+candidates" && candidateDelivery != "static" { result["candidate_delivery"] = candidateDelivery }
        if let choice = manualCover { result["manual_cover"] = choice.json }
        return result
    }
}

enum ExistingOutputDecision { case resume, skip, overwrite, cancel }

final class StopToken {
    let rustToken = ms_stop_new()!
    deinit { ms_stop_free(rustToken) }
    private let lock = NSLock()
    private var stopped = false
    func cancel() { lock.lock(); stopped = true;
        ms_stop_cancel(rustToken)
 lock.unlock() }
    var isCancelled: Bool { lock.lock(); defer { lock.unlock() }; return stopped }
}

// Rust owns grouping, evidence, ordering and plans; these are transport/UI values.
struct BurstFrame: Codable, Equatable {
    let group_id: String
    let source_group: String
    let index: Int
    let source_cover: Bool
    let source_sha256: String?
    let supported: Bool
    let reason: String
}
struct BurstMember: Codable, Equatable, Identifiable {
    let source: String
    let index: Int
    let source_cover: Bool
    let version: String?
    let source_sha256: String?
    var id: String { source }
}
struct BurstGroup: Codable, Equatable, Identifiable {
    let id: String
    let label: String
    let members: [BurstMember]
    let signature: String
    let representative: String?
    let reason: String
    let ordered: Bool
    let has_gaps: Bool?
    let supported: Bool
    let source_cover_count: Int
}
struct BurstChoice: Codable, Equatable {
    var signature: String
    var representative: String?
    var independent = false
}
func transportJSON<T: Encodable>(_ value: T) -> Any? {
    (try? JSONEncoder().encode(value)).flatMap { try? JSONSerialization.jsonObject(with:$0) }
}

struct ScanRow {
    let source: URL
    let relative: String
    let isMotion: Bool
    let hdr: String
    let cover: String
    let detail: String
    var manufacturer: String = "未知"
    var manufacturerDetail: String = ""
    var confirmedStatic = false
    var version: SourceVersion?
    var burst: BurstFrame? = nil
    var isStaticHDR: Bool { confirmedStatic && ["Ultra HDR","ISO HDR","旧 Apple HDR","荣耀私有 HDR","HEIC HDR"].contains(hdr) }
    var isConvertible: Bool { burst.map { $0.supported } ?? (isMotion || isStaticHDR) }
    var needsInspection: Bool { burst.map { !$0.supported } ?? (!isMotion && !confirmedStatic) }
}

// A batch-local reuse check, including inode and change time. Never persisted as
// proof of content identity; conversion still hashes its input and output.
struct SourceVersion: Equatable {
    let device: Int32
    let inode: UInt64
    let size: Int64
    let modified: timespec
    let changed: timespec
    static func read(_ url: URL) -> SourceVersion? {
        var value = stat()
        guard lstat(url.path, &value) == 0, (value.st_mode & S_IFMT) == S_IFREG else { return nil }
        return SourceVersion(device:value.st_dev,inode:value.st_ino,size:value.st_size,modified:value.st_mtimespec,changed:value.st_ctimespec)
    }
    static func readDirectory(_ url:URL) -> SourceVersion? {
        var value = stat()
        guard lstat(url.path,&value) == 0, (value.st_mode & S_IFMT) == S_IFDIR else { return nil }
        return SourceVersion(device:value.st_dev,inode:value.st_ino,size:value.st_size,modified:value.st_mtimespec,changed:value.st_ctimespec)
    }
    static func == (a: Self, b: Self) -> Bool {
        a.device == b.device && a.inode == b.inode && a.size == b.size &&
        a.modified.tv_sec == b.modified.tv_sec && a.modified.tv_nsec == b.modified.tv_nsec &&
        a.changed.tv_sec == b.changed.tv_sec && a.changed.tv_nsec == b.changed.tv_nsec
    }
}

// Fixed-size workers, with weighted admission so large inputs run alone when
// necessary. Results/progress are serialized; no task is created per library item.
enum BatchScheduling {
    static func limit(scanning: Bool) -> Int {
        let info = ProcessInfo.processInfo
        if info.thermalState == .serious || info.thermalState == .critical { return 1 }
        return max(1,min(scanning ? 4 : 2,info.activeProcessorCount / 2,Int(info.physicalMemory / (2 * 1024 * 1024 * 1024))))
    }
    @discardableResult static func perform<T>(files: [URL], limit: Int, heavy: Bool = false, token: StopToken,
                           work: @escaping (Int) -> T, receive: @escaping (Int,T) -> Void) -> (peak: Int, reservedBytes: Int64) {
        guard !files.isEmpty else { return (0,0) }
        let condition = NSCondition(), group = DispatchGroup()
        let budget = max(Int64(256 * 1024 * 1024),Int64(ProcessInfo.processInfo.physicalMemory / 8))
        let costs = files.map { min(budget,max(Int64(64 * 1024 * 1024),min(budget,SourceVersion.read($0)?.size ?? budget) * (heavy ? 24 : 12))) }
        var next = 0, used: Int64 = 0
        var running = 0, peak = 0, peakBytes: Int64 = 0
        for _ in 0..<max(1,min(limit,files.count)) {
            group.enter()
            DispatchQueue.global(qos:.userInitiated).async {
                defer { group.leave() }
                while true {
                    condition.lock()
                    while next < files.count && !token.isCancelled && used + costs[next] > budget {
                        _ = condition.wait(until:Date().addingTimeInterval(0.1))
                    }
                    guard next < files.count, !token.isCancelled else { condition.unlock(); return }
                    let index = next, cost = costs[index]; next += 1; used += cost
                    running += 1; peak = max(peak,running); peakBytes = max(peakBytes,used)
                    condition.unlock()
                    let value = autoreleasepool { work(index) }
                    condition.lock()
                    receive(index,value)
                    used -= cost; running -= 1; condition.broadcast(); condition.unlock()
                }
            }
        }
        group.wait()
        return (peak,peakBytes)
    }
}

struct BatchProgress {
    let index: Int
    let total: Int
    let relative: String
    let status: String
    let detail: String
}

struct BatchOutcome {
    let counts: [String:Int]
    let report: URL?
    let reportData: [String:Any]
    let cancelled: Bool
}

enum NativeBatch {
    static func normalized(_ url: URL) -> URL { url.standardizedFileURL.resolvingSymlinksInPath() }
    static func root(_ url: URL) -> URL {
        (try? url.resourceValues(forKeys: [.isDirectoryKey]).isDirectory) == true ? url : url.deletingLastPathComponent()
    }
    static func files(_ source: URL) throws -> [URL] {
        let props = try source.resourceValues(forKeys: [.isDirectoryKey, .isRegularFileKey])
        if props.isRegularFile == true {
            guard ["jpg","jpeg","heic","heif"].contains(source.pathExtension.lowercased()) else { throw Failure("请选择 Android 动态照片 JPEG/HEIC 或照片文件夹") }
            return [source]
        }
        guard props.isDirectory == true else { throw Failure("输入不是可读取的文件或文件夹") }
        var errors: [Error] = []
        guard let enumerator = FileManager.default.enumerator(at: source, includingPropertiesForKeys: [.isRegularFileKey,.isSymbolicLinkKey],
            options: [.skipsHiddenFiles,.skipsPackageDescendants], errorHandler: { _,error in errors.append(error); return true }) else { throw Failure("无法读取照片文件夹") }
        var found: [URL] = []
        for case let file as URL in enumerator {
            let info = try file.resourceValues(forKeys: [.isRegularFileKey,.isSymbolicLinkKey])
            if info.isRegularFile == true && info.isSymbolicLink != true && ["jpg","jpeg","heic","heif"].contains(file.pathExtension.lowercased()) { found.append(file) }
        }
        if let error = errors.first { throw Failure("部分目录不可读取：\(error.localizedDescription)") }
        return found.sorted { $0.path.localizedStandardCompare($1.path) == .orderedAscending }
    }
    static func selection(_ sources: [URL]) throws -> (base: URL, entries: [URL]) {
        guard let first = sources.first else { throw Failure("请选择照片或文件夹") }
        var base = root(normalized(first))
        var selected: Set<URL> = []
        for source in sources.map({ normalized($0) }) {
            let parent = root(source)
            while parent != base && !parent.path.hasPrefix(base.path == "/" ? "/" : base.path + "/") {
                let next = base.deletingLastPathComponent()
                guard next != base else { break }
                base = next
            }
            for file in try files(source) { selected.insert(normalized(file)) }
        }
        return (base, selected.sorted { $0.path.localizedStandardCompare($1.path) == .orderedAscending })
    }
    static func validateOutput(sources: [URL], output: URL) throws {
        for source in sources { try validateOutput(source: source, output: output) }
    }
    static func relative(_ source: URL, root: URL) throws -> String {
        let sourcePath = normalized(source).path, rootPath = normalized(root).path
        let prefix = rootPath.hasSuffix("/") ? rootPath : rootPath + "/"
        guard sourcePath.hasPrefix(prefix) else { throw Failure("输入文件位于所选目录之外：\(sourcePath) / \(rootPath)") }
        return String(sourcePath.dropFirst(prefix.count))
    }
    static func validateOutput(source: URL, output: URL) throws {
        let input = normalized(root(source)), output = normalized(output)
        let prefix = input.path.hasSuffix("/") ? input.path : input.path + "/"
        guard output != input && !output.path.hasPrefix(prefix) else { throw Failure("请选择原片文件夹之外的输出位置") }
    }
    static func scan(source: URL, token: StopToken, progress: @escaping (Int,Int)->Void) throws -> [ScanRow] {
        try scan(sources: [source], token: token, progress: progress)
    }
    static func scan(sources: [URL], direction:String = "to-apple", token: StopToken,
                     concurrency: Int? = nil, rowsReady: (([ScanRow])->Void)? = nil,
                     progress: @escaping (Int,Int)->Void) throws -> [ScanRow] {
        let req: [String:Any] = ["mode":"scan","inputs":sources.map { $0.path },"direction":direction,"full_rows":true,
            "concurrency":concurrency ?? BatchScheduling.limit(scanning:true)]
        let value = try RustCore.engine(req,token:token,progress:{ progress($0.index,$0.total) },rows:rowsReady)
        return (value["rows"] as? [[String:Any]] ?? []).compactMap(RustCore.scanRow)
    }
    static func scanFile(_ file: URL, relative: String, bytes: [UInt8]? = nil, parsed: Result<NativeMotionPhoto,Error>? = nil) -> ScanRow {
        return RustCore.scanFile(file,relative:relative,direction:"to-apple")
    }
    static func hashFile(_ url: URL) throws -> String {
        // Streaming avoids retaining an additional copy of each original photo.
        let handle = try FileHandle(forReadingFrom: url)
        defer { try? handle.close() }
        var hash = SHA256()
        while let data = try handle.read(upToCount: 1024 * 1024), !data.isEmpty { hash.update(data: data) }
        return hex(hash.finalize())
    }
    static func outputNames(_ source: URL) -> [String:String] {
        ["still":source.lastPathComponent,"video":source.deletingPathExtension().lastPathComponent + ".mov"]
    }
    static func isCandidatePhotoRole(_ role: String) -> Bool {
        role == "original" || (role.hasPrefix("candidate_") && Int(role.dropFirst(10)).map { $0 >= 0 } == true)
    }
    static func candidateVideoRole(_ photoRole: String) -> String {
        photoRole == "original" ? "original_video" : "candidate_video_" + photoRole.dropFirst(10)
    }
    static func expectedNames(_ source: URL, options: ConversionOptions) -> [String:String] {
        var names = options.outputMode == "auxiliary-only" ? [:] : outputNames(source)
        if options.coverIndex != nil { names["still"] = source.deletingPathExtension().lastPathComponent + ".jpg" }
        if ["livephoto+auxiliary","auxiliary-only"].contains(options.outputMode) { names["auxiliary"] = source.deletingPathExtension().lastPathComponent + ".auxiliary.mov" }
        return names
    }
    static func run(source: URL, output: URL, options: ConversionOptions, token: StopToken,
                    progress: @escaping (BatchProgress)->Void) throws -> BatchOutcome {
        try run(sources:[source],output:output,options:options,token:token,progress:progress)
    }
    static func run(sources: [URL], output: URL, options: ConversionOptions, token: StopToken,
                    scanRows: [ScanRow]? = nil, concurrency: Int? = nil,
                    existingOutput: ((URL,URL)->ExistingOutputDecision)? = nil,
                    progress: @escaping (BatchProgress)->Void) throws -> BatchOutcome {
        return try RustCore.run(sources:sources,output:output,options:options,token:token,scanRows:scanRows,
            concurrency:concurrency,existing:existingOutput,progress:progress)
    }
}


extension SourceVersion {
    var manualSignature: String {
        let modifiedNS = Int64(modified.tv_sec) * 1_000_000_000 + Int64(modified.tv_nsec)
        let changedNS = Int64(changed.tv_sec) * 1_000_000_000 + Int64(changed.tv_nsec)
        return "\(UInt32(bitPattern:device)):\(inode):\(size):\(modifiedNS):\(changedNS)"
    }
}
