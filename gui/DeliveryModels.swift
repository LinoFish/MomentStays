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

struct DeliveredFile: Codable, Equatable {
    var source: String
    var folder: String
    var files: [String]
    var key: String
    var origin: LibraryOrigin?
    var hashes: [String: String] = [:]
    func verify() throws {
        for file in files {
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
}

struct LibraryImportRecord: Codable {
    var key: String
    var assetID: String?
    var albumID: String?
    var state: String
    var message: String = ""
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
        var values = settings.options.json
        values["delivery_revision"] = "1.3.0"
        if let origin = origin {
            values["asset"] = origin.assetID; values["version"] = origin.version; values["edited"] = origin.edited
        } else { values["source_sha256"] = try NativeBatch.hashFile(source) }
        return sha(try JSONSerialization.data(withJSONObject: values, options: .sortedKeys))
    }
    static func collect(_ outcome: BatchOutcome, settings: ConversionSettings, origins: [String: LibraryOrigin]) throws -> [DeliveredFile] {
        let entries = outcome.reportData["files"] as? [[String: Any]] ?? []
        let bySource = Dictionary(origins.map { (NativeBatch.normalized(URL(fileURLWithPath: $0.key)).path, $0.value) }, uniquingKeysWith: { first, _ in first })
        return try entries.compactMap { entry in
            guard ["converted", "resumed"].contains(entry["status"] as? String ?? ""),
                  let source = entry["source"] as? String, let path = entry["output"] as? String else { return nil }
            let input = URL(fileURLWithPath: source), folder = URL(fileURLWithPath: path)
            let files: [URL]
            if settings.direction == .toAndroid {
                let ext = settings.container == .heic ? "heic" : "jpg"
                files = [folder.appendingPathComponent(input.deletingPathExtension().lastPathComponent + ".MP." + ext)]
            } else {
                guard settings.exportMode != .auxiliaryOnly else { return nil }
                files = [folder.appendingPathComponent(input.lastPathComponent), folder.appendingPathComponent(input.deletingPathExtension().lastPathComponent + ".mov")]
            }
            guard files.allSatisfy({ FileManager.default.fileExists(atPath: $0.path) }) else { throw Failure("转换输出不完整，未交付：\(input.lastPathComponent)") }
            return DeliveredFile(source: source, folder: path, files: files.map(\.path),
                key: try key(source: input, origin: bySource[NativeBatch.normalized(input).path], settings: settings), origin: bySource[NativeBatch.normalized(input).path], hashes: try Dictionary(uniqueKeysWithValues: files.map { ($0.path, try NativeBatch.hashFile($0)) }))
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
        for file in task.results.flatMap(\.files) {
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
