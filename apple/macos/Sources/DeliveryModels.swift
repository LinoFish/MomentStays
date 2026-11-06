import Foundation

struct LibraryOrigin: Codable, Equatable {
    var assetID: String
    var version: String
    var edited: Bool
    var filename: String
    var created: Date?
    var latitude: Double?
    var longitude: Double?
}

struct DeliveredBurstMember: Codable, Equatable {
    var source: String
    var file: String
    var index: Int
    var source_cover: Bool
    var representative: Bool
    var photo_identifier: String?
}
struct DeliveredBurst: Codable, Equatable {
    var group_id: String
    var uuid: String?
    var independent: Bool
    var members: [DeliveredBurstMember]
}
struct BurstLibraryFacts {
    var expectedIDs: [String]
    var actualIDs: [String]
    var identifiers: [String?]
    var representativeCount: Int
    var groupMembers: Set<String>
    var recognized: Bool {
        let expected = Set(expectedIDs)
        let nonempty = identifiers.compactMap { $0 }.filter { !$0.isEmpty }
        return expectedIDs.count > 1 && expected.count == expectedIDs.count && Set(actualIDs) == expected && actualIDs.count == expectedIDs.count &&
            identifiers.count == expectedIDs.count && nonempty.count == expectedIDs.count && Set(nonempty).count == 1 && representativeCount == 1 && expected.isSubset(of:groupMembers)
    }
}

struct DeliveredFile: Codable, Equatable {
    var source: String
    var folder: String
    var files: [String]
    var key: String
    var origin: LibraryOrigin?
    var hashes: [String: String] = [:]
    var standaloneHDR: Bool? = nil
    var burst: DeliveredBurst? = nil
    // Opaque engine verification, retained even when JSON output attachments are disabled.
    var verification: Data? = nil
    func verify() throws {
        for file in Set(files).union(hashes.keys) {
            guard let expected = hashes[file], try NativeBatch.hashFile(URL(fileURLWithPath: file)) == expected else {
                throw Failure("输出文件已改变或缺少校验记录，请重新转换：" + URL(fileURLWithPath: file).lastPathComponent)
            }
        }
    }
}

struct DeliveryTask: Codable, Identifiable {
    var id = UUID().uuidString
    var created = Date()
    var settings: ConversionSettings
    var output: String
    var results: [DeliveredFile] = []
    var shareFiles: [String] = []
    var shareState = "待共享"
    var importSummary = ""
    var cancelled = false
    var processingCounts: [String:Int]? = nil
    var originalExport: FileExportRecord? = nil
    var resultExport: FileExportRecord? = nil
    var resultSummary: String {
        let photos = results.reduce(0) { $0 + ($1.burst?.members.count ?? 1) }
        let groups = results.filter { $0.burst?.uuid != nil }.count
        return "\(photos) 张" + (groups > 0 ? " · \(groups) 组连拍" : "")
    }
    var processingSummary: String {
        guard let counts = processingCounts else { return "" }
        return "转换 \(counts["converted",default:0]) · 独立照片原样输出 \(counts["copied",default:0]) · 已有 \(counts["resumed",default:0]) · 跳过 \(counts["skipped",default:0]) · 失败 \(counts["failed",default:0])"
    }
    var canShare: Bool { !results.isEmpty && (settings.direction.exportsAndroid || results.contains { $0.burst != nil }) }
    var hasRecord: Bool { !results.isEmpty || originalExport != nil }
}

// Plans and verification remain opaque. Swift stores them, Rust owns resource
// discovery, source validation, stable names and atomic publication.
struct FileExportRecord: Codable {
    var kind: String
    var output: String
    var request: Data
    var plans: Data?
    var report: Data?
    var counts: [String:Int] = [:]
    var state = "待处理"
    var detail: String?
    var summary: String {
        (kind == "originals" ? "原样导出" : "结果导出") + "：新增 \(counts["copied",default:0]) · 复用 \(counts["resumed",default:0]) · 跳过 \(counts["skipped",default:0]) · 失败 \(counts["failed",default:0]) · " + state
    }
    var outputs: [String] {
        guard let data = report, let report = try? JSONSerialization.jsonObject(with:data) as? [String:Any],
              let files = report["files"] as? [[String:Any]] else { return [] }
        return files.filter { ["copied","resumed"].contains($0["status"] as? String ?? "") }.compactMap { $0["output"] as? String }
    }
}

enum FileExportRequest {
    static func remainingOriginals(_ record: FileExportRecord, outcome: BatchOutcome) throws -> FileExportRecord {
        var record = record
        var request = try JSONSerialization.jsonObject(with:record.request) as? [String:Any] ?? [:]
        let entries = outcome.reportData["files"] as? [[String:Any]] ?? []
        var completed: Set<String> = []
        for entry in entries where ["converted","resumed"].contains(entry["status"] as? String ?? "") {
            if let source = entry["source"] as? String { completed.insert(source) }
            completed.formUnion(entry["sources"] as? [String] ?? [])
            if let burst = entry["burst"] as? [String:Any], let members = burst["members"] as? [[String:Any]] {
                completed.formUnion(members.compactMap { $0["source"] as? String })
            }
        }
        request["items"] = (request["items"] as? [[String:Any]] ?? []).filter { !completed.contains($0["source"] as? String ?? "") }
        record.request = try JSONSerialization.data(withJSONObject:request,options:.sortedKeys)
        return record
    }
    static func originals(_ rows: [ScanRow], direction: ConversionDirection, photos: Set<String>) -> [String:Any] {
        ["mode":"export-plan","kind":"originals","direction":direction.rawValue,"items":rows.map { row in
            ["source":row.source.path,"source_kind":photos.contains(row.source.path) ? "photos" : "local","scan_row":RustCore.rowJSON(row)] as [String:Any]
        }]
    }
    static func results(_ results: [DeliveredFile]) throws -> [String:Any] {
        var seen: Set<String> = []
        let items = try results.filter { seen.insert($0.folder).inserted }.map { result -> [String:Any] in
            var item: [String:Any] = ["source":result.folder]
            if let data = result.verification { item["verification"] = try JSONSerialization.jsonObject(with:data) }
            return item
        }
        return ["mode":"export-plan","kind":"results","items":items]
    }
    static func run(_ record: inout FileExportRecord, token: StopToken,
                    progress: @escaping (BatchProgress) -> Void,
                    existing: @escaping (URL,URL) -> ExistingOutputDecision,
                    checkpoint: (FileExportRecord) throws -> Void) throws {
        let request = try JSONSerialization.jsonObject(with:record.request) as? [String:Any] ?? [:]
        let planned = try RustCore.engine(request,token:token)
        let plans = planned["plans"] as? [Any] ?? []
        record.plans = try JSONSerialization.data(withJSONObject:plans,options:.sortedKeys)
        let rejected = planned["files"] as? [[String:Any]] ?? []
        record.counts = ["failed":rejected.filter { ($0["status"] as? String) == "failed" }.count]
        record.state = "正在导出"
        record.report = try JSONSerialization.data(withJSONObject:["export_kind":record.kind,"files":rejected,"counts":record.counts],options:.sortedKeys)
        try checkpoint(record)
        guard !token.isCancelled, planned["cancelled"] as? Bool != true else { record.state = "已停止"; return }
        guard !plans.isEmpty else { record.state = rejected.isEmpty ? "本轮无待导出项" : "没有可导出项"; return }
        let result = try RustCore.engine(["mode":"export","kind":record.kind,"output":record.output,"plans":plans,
            "existing_output":"resume","ask_existing_output":true],token:token,progress:progress,existing:existing)
        let rejectedCount = record.counts["failed",default:0]
        record.counts = result["counts"] as? [String:Int] ?? [:]
        record.counts["failed",default:0] += rejectedCount
        var report = result["report_data"] as? [String:Any] ?? [:]
        report["files"] = rejected + (report["files"] as? [[String:Any]] ?? [])
        report["counts"] = record.counts
        record.report = try JSONSerialization.data(withJSONObject:report,options:.sortedKeys)
        record.state = result["cancelled"] as? Bool == true || token.isCancelled ? "已停止" : record.counts["failed",default:0] > 0 ? "部分失败" : "已结束"
    }
}

struct LibraryImportRecord: Codable {
    var key: String
    var assetID: String?
    var albumID: String?
    var state: String
    var message: String = ""
    var assetIDs: [String]? = nil
    var burstIdentifier: String? = nil
    var representativeAssetID: String? = nil
}

// Shared by the PhotoKit adapter and isolated tests. Never recreate a missing
// committed/uncertain asset; only a definitely unsubmitted record may create.
enum LibraryImportPolicy {
    static func shouldCreate(_ record: LibraryImportRecord?, assetExists: Bool) throws -> Bool {
        guard let record = record else { return true }
        if assetExists { return false }
        if record.assetID != nil { throw Failure("上次导入的照片已删除或不可读，未自动补回") }
        if record.state == "pending" { return true }
        throw Failure("上次入库状态待核对，未自动重复创建照片")
    }
}

// Own storage only. Reads at launch; directories are created only for a user task.
// Injectable root keeps all automated tests outside the real library/user history.
final class DeliveryStore {
    let root: URL
    private let lock = NSRecursiveLock()
    init(root: URL = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
        .appendingPathComponent("Motion-Live", isDirectory: true)) { self.root = root }
    var tasksURL: URL { root.appendingPathComponent("Tasks", isDirectory: true) }
    func taskFolder(_ id: String) -> URL { tasksURL.appendingPathComponent(id, isDirectory: true) }
    private func read<T: Decodable>(_ type: T.Type, _ url: URL, fallback: T) -> T {
        guard let data = try? Data(contentsOf: url), let value = try? JSONDecoder().decode(type, from: data) else { return fallback }
        return value
    }
    private func write<T: Encodable>(_ value: T, _ url: URL) throws {
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        let encoder = JSONEncoder(); encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        try encoder.encode(value).write(to: url, options: .atomic)
    }
    func save(_ task: DeliveryTask) throws {
        lock.lock(); defer { lock.unlock() }
        guard UUID(uuidString: task.id) != nil else { throw Failure("任务标识无效") }
        try write(task, taskFolder(task.id).appendingPathComponent("task.json"))
    }
    func tasks() -> [DeliveryTask] {
        lock.lock(); defer { lock.unlock() }
        let dirs = (try? FileManager.default.contentsOfDirectory(at: tasksURL, includingPropertiesForKeys: nil)) ?? []
        return dirs.compactMap { url -> DeliveryTask? in
            guard let data = try? Data(contentsOf: url.appendingPathComponent("task.json")) else { return nil }
            guard let task = try? JSONDecoder().decode(DeliveryTask.self, from: data), UUID(uuidString: task.id) != nil, task.id == url.lastPathComponent else { return nil }
            return task
        }.sorted { $0.created > $1.created }
    }
    func validate() throws {
        let url = root.appendingPathComponent("imports.json")
        if FileManager.default.fileExists(atPath: url.path) {
            do { _ = try JSONDecoder().decode([String: LibraryImportRecord].self, from: Data(contentsOf: url)) }
            catch { throw Failure("图库去重记录不可读，已停止写入以避免重复导入；请保留记录并检查。") }
        }
    }
    func records() -> [String: LibraryImportRecord] {
        lock.lock(); defer { lock.unlock() }
        return read([String: LibraryImportRecord].self, root.appendingPathComponent("imports.json"), fallback: [:])
    }
    func save(_ record: LibraryImportRecord) throws {
        lock.lock(); defer { lock.unlock() }
        try validate()
        var values = records(); values[record.key] = record
        try write(values, root.appendingPathComponent("imports.json"))
    }
    var generatedAssetIDs: Set<String> { Set(records().values.compactMap(\.assetID)) }
    func inspectionVersion(_ id: String, scope: String) -> String? {
        lock.lock(); defer { lock.unlock() }
        return read([String: String].self, root.appendingPathComponent("inspected.json"), fallback: [:])[scope + "|" + id]
    }
    func inspected(_ id: String, version: String, scope: String) throws {
        lock.lock(); defer { lock.unlock() }
        let url = root.appendingPathComponent("inspected.json")
        var values = read([String: String].self, url, fallback: [:]); values[scope + "|" + id] = version
        try write(values, url)
    }
    func removeTask(_ task: DeliveryTask, active: Set<String>) throws {
        guard UUID(uuidString: task.id) != nil else { throw Failure("任务标识无效") }
        guard !active.contains(task.id) else { throw Failure("任务仍在读取或共享，暂不能清理") }
        let folder = taskFolder(task.id)
        guard folder.deletingLastPathComponent().standardizedFileURL == tasksURL.standardizedFileURL else { throw Failure("任务路径无效") }
        // Only app-owned cache: never the user-selected output path.
        if FileManager.default.fileExists(atPath: folder.path) { try FileManager.default.removeItem(at: folder) }
    }
    static func key(source: URL, origin: LibraryOrigin?, settings: ConversionSettings) throws -> String {
        var options = settings.options; options.coverIndex = settings.coverIndex(for:source)
        options.manualCover = settings.manualCovers?[NativeBatch.normalized(source).path]
        var values = options.json
        values["delivery_revision"] = "1.3.0"
        if let origin = origin {
            values["asset"] = origin.assetID; values["version"] = origin.version; values["edited"] = origin.edited
        } else { values["source_sha256"] = try NativeBatch.hashFile(source) }
        return sha(try JSONSerialization.data(withJSONObject: values, options: .sortedKeys))
    }
    static func collect(_ outcome: BatchOutcome, settings: ConversionSettings, origins: [String: LibraryOrigin]) throws -> [DeliveredFile] {
        let entries = outcome.reportData["files"] as? [[String: Any]] ?? []
        let bySource = Dictionary(origins.map { (NativeBatch.normalized(URL(fileURLWithPath: $0.key)).path, $0.value) }, uniquingKeysWith: { first, _ in first })
        var burstOutputs: Set<String> = []
        return try entries.flatMap { entry -> [DeliveredFile] in
            guard ["converted", "resumed"].contains(entry["status"] as? String ?? ""),
                  let source = entry["source"] as? String, let path = entry["output"] as? String else { return [] }
            let input = URL(fileURLWithPath: source), folder = URL(fileURLWithPath: path)
            let verification = try entry["verification"].map { try JSONSerialization.data(withJSONObject:$0,options:.sortedKeys) }
            var verifyRequest: [String:Any] = ["mode":"verify","input":path]
            if let value = entry["verification"] { verifyRequest["verification"] = value }
            if let burstJSON = entry["burst"] as? [String:Any] {
                guard burstOutputs.insert(path).inserted else { return [] }
                let verifiedManifest = try RustCore.engine(verifyRequest)
                let burst = try JSONDecoder().decode(DeliveredBurst.self,from:JSONSerialization.data(withJSONObject:burstJSON))
                let resources = burst.members.map { folder.appendingPathComponent($0.file) }
                let names = entry["output_files"] as? [String:String] ?? [:]
                var verified = names.values.map { folder.appendingPathComponent($0) }
                let manifest = folder.appendingPathComponent("manifest.json")
                if FileManager.default.fileExists(atPath:manifest.path) { verified.append(manifest) }
                let identityValue: [String:Any] = ["identity":burstJSON["identity"] ?? burstJSON,"implementation":verifiedManifest["implementation"] ?? "","outputs":verifiedManifest["output_sha256"] ?? [:]]
                let identity = try JSONSerialization.data(withJSONObject:identityValue,options:[.sortedKeys])
                return [DeliveredFile(source:source,folder:path,files:resources.map(\.path),key:sha(identity),origin:nil,
                    hashes:try Dictionary(uniqueKeysWithValues:verified.map { ($0.path,try NativeBatch.hashFile($0)) }),burst:burst,verification:verification)]
            }
            let files: [URL]
            if let names = entry["output_files"] as? [String:String], let photo = names["static"] {
                _ = try RustCore.engine(verifyRequest)
                files = [folder.appendingPathComponent(photo)]
            } else if settings.direction.exportsAndroid {
                let ext = settings.direction == .toAndroid && settings.container == .heic ? "heic" : "jpg"
                files = [folder.appendingPathComponent(input.deletingPathExtension().lastPathComponent + ".MP." + ext)]
            } else {
                guard settings.exportMode != .auxiliaryOnly else { return [] }
                var options = settings.options; options.coverIndex = settings.coverIndex(for:input)
                let names = entry["output_files"] as? [String:String] ?? NativeBatch.expectedNames(input,options:options)
                guard let still = names["still"], let movie = names["video"] else { throw Failure("缺少实况照片输出信息") }
                files = [folder.appendingPathComponent(still),folder.appendingPathComponent(movie)]
            }
            guard files.allSatisfy({ FileManager.default.fileExists(atPath: $0.path) }) else { throw Failure("转换输出不完整，未交付：\(input.lastPathComponent)") }
            let origin = bySource[NativeBatch.normalized(input).path]
            let baseKey = try key(source:input,origin:origin,settings:settings)
            var verifiedFiles = files
            if settings.manualCovers?[NativeBatch.normalized(input).path] != nil {
                _ = try RustCore.engine(verifyRequest)
                let names = entry["output_files"] as? [String:String] ?? [:]
                verifiedFiles += names.filter { $0.key.hasPrefix("legacy_") }.values.map { folder.appendingPathComponent($0) }
                verifiedFiles.append(folder.appendingPathComponent("manifest.json"))
            }
            var result = [DeliveredFile(source:source,folder:path,files:files.map(\.path),key:baseKey,origin:origin,
                hashes:try Dictionary(uniqueKeysWithValues:verifiedFiles.map { ($0.path,try NativeBatch.hashFile($0)) }),
                standaloneHDR:(entry["output_files"] as? [String:String])?["static"] != nil,verification:verification)]
            if settings.direction == .toApple && settings.exportMode == .withCandidates {
                let names = entry["output_files"] as? [String:String] ?? [:]
                for role in names.keys.sorted() where NativeBatch.isCandidatePhotoRole(role) {
                    let photo = folder.appendingPathComponent(names[role]!)
                    var resources = [photo]
                    if let video = names[NativeBatch.candidateVideoRole(role)] { resources.append(folder.appendingPathComponent(video)) }
                    result.append(DeliveredFile(source:source,folder:path,files:resources.map(\.path),key:sha(Data((baseKey + "|" + role).utf8)),
                        origin:origin,hashes:try Dictionary(uniqueKeysWithValues:resources.map { ($0.path,try NativeBatch.hashFile($0)) }),verification:verification))
                }
            }
            return result
        }
    }
    func stageShare(_ task: inout DeliveryTask, token: StopToken? = nil) throws -> [URL] {
        guard UUID(uuidString: task.id) != nil else { throw Failure("任务标识无效") }
        let folder = taskFolder(task.id).appendingPathComponent("Shared Files", isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        var names: Set<String> = [], result: [URL] = []
        for result in task.results {
            guard token?.isCancelled != true else { throw Failure("共享准备已停止；文件保留") }
            try result.verify()
        }
        // A legacy result is shared as one complete archive, so its source
        // backups and manifest cannot be separated by filename collision handling.
        let archived = task.results.filter { $0.burst != nil || $0.hashes.keys.contains(where:{ URL(fileURLWithPath:$0).lastPathComponent == "source-original.bin" }) }
        for item in archived {
            let work = taskFolder(task.id).appendingPathComponent("Share-" + UUID().uuidString,isDirectory:true)
            let bundleName = URL(fileURLWithPath:item.source).deletingPathExtension().lastPathComponent + (item.burst == nil ? ".livephoto" : ".burst")
            let bundle = work.appendingPathComponent(bundleName,isDirectory:true)
            try FileManager.default.createDirectory(at:bundle,withIntermediateDirectories:true)
            defer { try? FileManager.default.removeItem(at:work) }
            for (path,hash) in item.hashes {
                let source = URL(fileURLWithPath:path)
                guard NativeBatch.normalized(source.deletingLastPathComponent()) == NativeBatch.normalized(URL(fileURLWithPath:item.folder)) else { throw Failure("附属数据路径无效") }
                let target = bundle.appendingPathComponent(source.lastPathComponent)
                try FileManager.default.copyItem(at:source,to:target)
                guard try NativeBatch.hashFile(target) == hash else { throw Failure("附属数据共享副本校验失败") }
            }
            var name = bundleName + ".zip", index = 1
            while names.contains(name.lowercased()) { index += 1; name = bundleName + " (\(index)).zip" }
            names.insert(name.lowercased())
            let temporary = work.appendingPathComponent("result.zip")
            let process = Process(); process.executableURL = URL(fileURLWithPath:"/usr/bin/ditto")
            process.arguments = ["-c","-k","--keepParent",bundle.path,temporary.path]
            try process.run(); process.waitUntilExit()
            guard process.terminationStatus == 0, token?.isCancelled != true else { throw Failure("完整结果共享准备未完成；原输出保留") }
            let target = folder.appendingPathComponent(name)
            if FileManager.default.fileExists(atPath:target.path) { try FileManager.default.removeItem(at:target) }
            try FileManager.default.moveItem(at:temporary,to:target); result.append(target)
        }
        let normal = task.results.filter { item in !archived.contains(where:{ $0.key == item.key }) }
        for file in normal.flatMap(\.files) {
            guard token?.isCancelled != true else { throw Failure("共享准备已停止；文件保留") }
            let source = URL(fileURLWithPath: file), ext = source.pathExtension
            var stem = source.deletingPathExtension().lastPathComponent
            if stem.hasSuffix(".MP") { stem.removeLast(3) }
            var index = 1, name = source.lastPathComponent
            while names.contains(name.lowercased()) { index += 1; name = stem + " (\(index)).MP." + ext }
            names.insert(name.lowercased())
            let target = folder.appendingPathComponent(name)
            let expected = try NativeBatch.hashFile(source)
            if FileManager.default.fileExists(atPath: target.path) {
                guard try NativeBatch.hashFile(target) == expected else { throw Failure("共享缓存与输出不一致，请创建新任务") }
            } else { try FileManager.default.copyItem(at: source, to: target) }
            guard try NativeBatch.hashFile(target) == expected else { throw Failure("共享副本校验失败") }
            result.append(target)
        }
        task.shareFiles = result.map(\.path); try save(task)
        return result
    }
}
