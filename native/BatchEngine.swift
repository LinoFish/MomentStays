import Foundation
import CryptoKit
import Darwin

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
    var json: [String:Any] { if direction == "to-android" { return ["direction":direction,"android":android.json,"time_domain":timeDomain,"export_data_files":exportDataFiles] }; return ["legacy_hdr": legacyHDR ? "auto" : "preserve", "missing_time": missingTime, "time_domain": timeDomain,
        "video_track":videoTrack,"output_mode":outputMode,"repair_auxiliary_duration":outputMode != "livephoto" && repairAuxiliaryDuration,"export_data_files":exportDataFiles] }
}

enum ExistingOutputDecision { case skip, overwrite, cancel }

final class StopToken {
    private let lock = NSLock()
    private var stopped = false
    func cancel() { lock.lock(); stopped = true; lock.unlock() }
    var isCancelled: Bool { lock.lock(); defer { lock.unlock() }; return stopped }
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
    var needsInspection: Bool { !isMotion && detail != PhotoFormat.notMotion }
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
    static let revision = "gui-native-1.1.4"
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
    // Expand only the selected inputs, deduplicating overlapping folders/files.
    // Their common parent keeps identical basenames in separate subdirectories.
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
    static func scan(source: URL, token: StopToken, progress: (Int,Int)->Void) throws -> [ScanRow] {
        try scan(sources: [source], token: token, progress: progress)
    }
    static func scan(sources: [URL], direction:String = "to-apple", token: StopToken, progress: (Int,Int)->Void) throws -> [ScanRow] {
        if direction == "to-android" { return try AndroidExport.scan(sources,token:token,progress:progress) }
        let (base, entries) = try selection(sources)
        var rows: [ScanRow] = []
        for (index,file) in entries.enumerated() {
            if token.isCancelled { break }
            let rel = try relative(file, root: base)
            let row: ScanRow = autoreleasepool {
                do {
                    let photo = try PhotoFormat.parse(Array(Data(contentsOf: file)))
                    let maker = PhotoVendor.identify(photo.still,format:photo.format)
                    let hdr: String
                    if PhotoHEIF.matches(photo.still) { hdr = "HEIC（原样保留）" }
                    else if photo.ultra { hdr = "Ultra HDR" }
                    else if try PhotoHDR.honorPrivate(photo.still) != nil { hdr = "荣耀私有 HDR" }
                    else if try PhotoHDR.hasISO(photo.still) { hdr = "ISO HDR" }
                    else { hdr = ((try? PhotoHDR.legacy(photo.still).0) ?? nil) != nil ? "旧 Apple HDR" : "SDR / 其他" }
                    let time = photo.coverFrame.map { "原始第 \($0) 帧（从0计）" } ?? photo.timestamp.map { String(format: "%.6f 秒", Double($0) / 1e6) } ?? "未记录"
                    return ScanRow(source: file, relative: rel, isMotion: true, hdr: hdr, cover: time,
                        detail: hdr == "荣耀私有 HDR" ? "荣耀私有 HDR：保留静态扩展，暂不能转为 Ultra HDR；Apple 设备显示 SDR" : photo.timestamp == nil && photo.coverFrame == nil ? "缺少封面时间；默认不转换" : "\(photo.format)：等待转换",manufacturer:maker.name,manufacturerDetail:maker.detail)
                } catch {
                    let maker = PhotoVendor.identify((try? Array(Data(contentsOf:file))) ?? [],format:"")
                    return ScanRow(source: file, relative: rel, isMotion: false, hdr: "—", cover: "—", detail: error.localizedDescription,manufacturer:maker.name,manufacturerDetail:maker.detail)
                }
            }
            rows.append(row); progress(index + 1, entries.count)
        }
        return rows
    }
    static func hashFile(_ url: URL) throws -> String {
        // Streaming avoids retaining an additional copy of each original photo.
        let handle = try FileHandle(forReadingFrom: url)
        defer { try? handle.close() }
        var hash = SHA256()
        while let data = try handle.read(upToCount: 1024 * 1024), !data.isEmpty { hash.update(data: data) }
        return hash.finalize().map { String(format: "%02x", $0) }.joined()
    }
    static func json(_ value: Any, to url: URL) throws {
        try JSONSerialization.data(withJSONObject: value, options: [.prettyPrinted,.sortedKeys,.withoutEscapingSlashes]).write(to: url, options: .atomic)
    }
    static func verify(_ folder: URL) throws -> [String:Any] {
        let data = try Data(contentsOf:folder.appendingPathComponent("manifest.json"))
        guard let manifest = try JSONSerialization.jsonObject(with:data) as? [String:Any],
              let hashes = manifest["output_sha256"] as? [String:String],
              manifest["timing"] is [String:Any], manifest["hdr"] is [String:Any] else { throw Failure("已有输出的校验清单无效") }
        let names = manifest["output_files"] as? [String:String] ?? ["still":"photo.jpg","video":"photo.mov"]
        let keys = Set(names.keys)
        guard [Set(["still","video"]),Set(["still","video","auxiliary"]),Set(["auxiliary"]),Set(["motionphoto"])].contains(keys),
              Set(hashes.keys) == Set(names.values), hashes.count == names.count,
              names.values.allSatisfy({ !$0.isEmpty && !$0.contains("/") && !$0.contains("\0") && $0 != "." && $0 != ".." }) else {
            throw Failure("已有输出的配对文件名无效")
        }
        for (role,name) in names {
            let ext = URL(fileURLWithPath:name).pathExtension.lowercased()
            guard ["still","motionphoto"].contains(role) ? ["jpg","jpeg","heic","heif"].contains(ext) : ext == "mov" else { throw Failure("输出文件扩展名无效") }
            guard try hashFile(folder.appendingPathComponent(name)) == hashes[name] else { throw Failure("已有输出校验失败：\(name)") }
        }
        return manifest
    }
    static func outputNames(_ source: URL) -> [String:String] {
        ["still":source.lastPathComponent,"video":source.deletingPathExtension().lastPathComponent + ".mov"]
    }
    static func expectedNames(_ source: URL, options: ConversionOptions) -> [String:String] {
        var names = options.outputMode == "auxiliary-only" ? [:] : outputNames(source)
        if options.outputMode != "livephoto" { names["auxiliary"] = source.deletingPathExtension().lastPathComponent + ".auxiliary.mov" }
        return names
    }
    static func publish(_ stage: URL, target: URL, source: URL, sourceHash: String, token: StopToken, overwrite: Bool = false, exportDataFiles: Bool = true) throws {
        _ = try verify(stage)
        guard try hashFile(source) == sourceHash else { throw Failure("转换期间原片发生变化，未发布结果") }
        if token.isCancelled { throw Failure("已停止；当前暂存结果未发布") }
        if !exportDataFiles { try FileManager.default.removeItem(at:stage.appendingPathComponent("manifest.json")) }
        let replacing = overwrite && FileManager.default.fileExists(atPath:target.path)
        let status = stage.withUnsafeFileSystemRepresentation { a in
            target.withUnsafeFileSystemRepresentation { b in renamex_np(a!,b!,UInt32(replacing ? RENAME_SWAP : RENAME_EXCL)) }
        }
        guard status == 0 else { throw Failure("无法发布结果（可能有同名文件或权限不足）：\(String(cString:strerror(errno)))") }
    }
    // Name-only upgrade from 1.0.3: stage a complete copy, verify it, then swap
    // directories atomically. A collision or cancellation leaves the old pair intact.
    static func restoreNames(_ target: URL, source: URL, manifest original: [String:Any], token: StopToken) throws {
        let fm = FileManager.default, names = outputNames(source)
        let oldNames = original["output_files"] as? [String:String] ?? ["still":"photo.jpg","video":"photo.mov"]
        let oldHashes = original["output_sha256"] as! [String:String]
        let stage = target.deletingLastPathComponent().appendingPathComponent(".motionlive-name-" + UUID().uuidString)
        defer { try? fm.removeItem(at:stage) }
        try fm.copyItem(at:target,to:stage)
        var hashes: [String:String] = [:]
        for key in ["still","video"] {
            let before = oldNames[key]!, after = names[key]!
            if before != after { try fm.moveItem(at:stage.appendingPathComponent(before),to:stage.appendingPathComponent(after)) }
            hashes[after] = oldHashes[before]!
        }
        var manifest = original
        manifest["implementation"] = revision; manifest["version"] = "1.1.4"
        manifest["output_files"] = names; manifest["original_filename"] = source.lastPathComponent
        manifest["output_sha256"] = hashes
        try json(manifest,to:stage.appendingPathComponent("manifest.json"))
        _ = try verify(stage)
        if token.isCancelled { throw Failure("已停止；原有文件名未改动") }
        let status = stage.withUnsafeFileSystemRepresentation { a in
            target.withUnsafeFileSystemRepresentation { b in renamex_np(a!,b!,UInt32(RENAME_SWAP)) }
        }
        guard status == 0 else { throw Failure("无法恢复输出文件名；原结果已保留：\(String(cString:strerror(errno)))") }
    }
    static func convert(source: URL, target: URL, options: ConversionOptions, token: StopToken, overwrite: Bool = false) throws -> [String:Any] {
        guard ["primary","secondary"].contains(options.videoTrack), ["livephoto","livephoto+auxiliary","auxiliary-only"].contains(options.outputMode) else { throw Failure("轨道或导出模式无效") }
        let fm = FileManager.default
        let sourceHash = try hashFile(source)
        if !overwrite && fm.fileExists(atPath: target.path) {
            let manifest = try verify(target)
            var previous = manifest["options"] as? [String:Any] ?? [:]
            if previous["video_track"] == nil { previous["video_track"] = "primary" }
            if previous["output_mode"] == nil { previous["output_mode"] = "livephoto" }
            if previous["repair_auxiliary_duration"] == nil { previous["repair_auxiliary_duration"] = false }
            if previous["export_data_files"] == nil { previous["export_data_files"] = true }
            if manifest["source_sha256"] as? String == sourceHash,
               [revision,"gui-native-1.1.3","gui-native-1.1.2","gui-native-1.1.1","gui-native-1.1.0","gui-native-1.0.6","gui-native-1.0.5","gui-native-1.0.4","gui-native-1.0.3"].contains(manifest["implementation"] as? String ?? ""),
               (previous as NSDictionary).isEqual(to:options.json) {
                if manifest["implementation"] as? String != revision && options.outputMode != "auxiliary-only" {
                    let original = try PhotoFormat.parse(Array(Data(contentsOf:source)))
                    if let extensionData = try PhotoHDR.honorPrivate(original.still),
                       (manifest["hdr"] as? [String:Any])?["private_extension_sha256"] as? String != sha(Data(extensionData)) {
                        throw Failure("旧版本输出未保留荣耀私有 HDR 静态扩展，请在界面选择覆盖后重新转换")
                    }
                }
                if manifest["output_files"] as? [String:String] != expectedNames(source,options:options) {
                    guard options.outputMode == "livephoto" else { throw Failure("已有输出文件名与所选模式不同，请使用新的输出位置") }
                    try restoreNames(target,source:source,manifest:manifest,token:token)
                    return ["status":"resumed", "source":source.path, "output":target.path, "detail":"已恢复原始文件名，图片和视频未重新编码"]
                }
                return ["status":"resumed", "source":source.path, "output":target.path, "detail":"已验证，跳过重复转换"]
            }
            throw Failure("同名输出已存在且内容或设置不同；未覆盖，请选择新的输出目录")
        }
        let photo = try PhotoFormat.parse(Array(Data(contentsOf: source)))
        if options.outputMode != "auxiliary-only" && photo.timestamp == nil && photo.coverFrame == nil && options.missingTime == "error" { throw Failure("原片没有有效封面时间戳或封面帧号，未使用推测位置") }
        if token.isCancelled { throw Failure("已停止") }
        try fm.createDirectory(at: target.deletingLastPathComponent(), withIntermediateDirectories: true)
        let stage = target.deletingLastPathComponent().appendingPathComponent(".motionlive-" + UUID().uuidString)
        let work = fm.temporaryDirectory.appendingPathComponent("motionlive-" + UUID().uuidString)
        try fm.createDirectory(at: stage, withIntermediateDirectories: false)
        defer { try? fm.removeItem(at: stage); try? fm.removeItem(at: work) }
        try fm.createDirectory(at: work, withIntermediateDirectories: false)
        let sourceVideo = work.appendingPathComponent("source.mp4")
        try Data(photo.video).write(to:sourceVideo)
        let names = expectedNames(source,options:options)
        var auxiliary: [String:Any]? = nil
        if let auxiliaryName = names["auxiliary"] {
            let auxiliaryURL = stage.appendingPathComponent(auxiliaryName)
            var repair: [String:Any]? = nil
            let exportSource = work.appendingPathComponent("auxiliary-input.mp4")
            var exportPath = sourceVideo.path
            if options.repairAuxiliaryDuration, let fixed = try PhotoFormat.repairAuxiliaryDuration(photo.video) {
                try Data(fixed.0).write(to:exportSource); exportPath = exportSource.path; repair = fixed.1
            }
            auxiliary = try exportAuxiliary(["input":exportPath,"output":auxiliaryURL.path])
            if let repair = repair {
                guard auxiliary?["compressed_sha256"] as? String == repair["original_sample_sha256"] as? String else { throw Failure("修复后的辅助轨压缩数据与原片不一致") }
                auxiliary?["duration_repair"] = repair
            }
            if let date = try source.resourceValues(forKeys:[.contentModificationDateKey]).contentModificationDate {
                try fm.setAttributes([.modificationDate:date],ofItemAtPath:auxiliaryURL.path)
            }
        }
        if options.outputMode == "auxiliary-only" {
            let name = names["auxiliary"]!
            let manifest: [String:Any] = ["version":"1.1.4","implementation":revision,"source":source.path,"source_sha256":sourceHash,
                "original_filename":source.lastPathComponent,"options":options.json,"output_files":names,
                "output_sha256":[name:try hashFile(stage.appendingPathComponent(name))],"timing":auxiliary!,
                "auxiliary_video":auxiliary!,"hdr":["status":"not-applicable"],"warnings":[]]
            try json(manifest,to:stage.appendingPathComponent("manifest.json"))
            try publish(stage,target:target,source:source,sourceHash:sourceHash,token:token,overwrite:overwrite,exportDataFiles:options.exportDataFiles)
            return ["status":"converted","source":source.path,"output":target.path,"detail":"辅助轨已单独导出：\(auxiliary!["width"]!) × \(auxiliary!["height"]!)，\(auxiliary!["frames"]!) 帧，\(auxiliary!["duration_seconds"]!) 秒；无音频，未重新编码" + (auxiliary?["duration_repair"] == nil ? "" : "；已按设置修复零时长，画面数据未改变"),"auxiliary_video":auxiliary!]
        }
        var image = photo.still
        var hdr: [String:Any] = ["input":photo.ultra ? "ultra-hdr" : "legacy-or-sdr", "status":"preserved", "reason":"Ultra HDR 主图及增益图原样保留"]
        let isHEIC = PhotoHEIF.matches(image)
        if !isHEIC && !photo.ultra, try PhotoHDR.hasISO(image) { hdr["input"] = "iso-hdr" }
        var warnings: [String] = []
        let honorHDR = try PhotoHDR.honorPrivate(image)
        if honorHDR != nil {
            hdr["input"] = "honor-private-ehdr"
            hdr["status"] = "unsupported-preserved"
            hdr["reason"] = "已保留荣耀私有 HDR 静态扩展；尚不能可靠转换为 Ultra HDR，Apple 设备显示 SDR。可先用荣耀相册转为通用 HDR 后再导入"
            hdr["private_extension_sha256"] = sha(Data(honorHDR!))
            warnings.append(hdr["reason"] as! String)
        }
        if auxiliary?["duration_repair"] != nil { warnings.append("已按高级设置修复独立辅助视频的零时长，压缩画面与主封面时间未改变") }
        if isHEIC {
            hdr = ["input":"heic", "status":"preserved", "reason":"HEIC 图像编码、色彩与方向属性原样保留；未新增或转换 HDR"]
        }
        if !isHEIC && !photo.ultra && honorHDR == nil {
            hdr["status"] = "unchanged"
            hdr["reason"] = "旧版 HDR 转换已关闭"
            if options.legacyHDR {
                do {
                    if try PhotoHDR.hasISO(image) {
                        if let upgraded = try PhotoHDR.upgradeISO(image) {
                            image = upgraded; hdr["status"] = "converted"; hdr["output"] = "ultra-hdr+iso"
                            hdr["reason"] = "已有 ISO 增益图已补充 Ultra HDR 元数据；主图及增益图均未重新编码"
                        } else { hdr["reason"] = "ISO HDR 数据已保留；缺少可验证的单增益图布局，未推测转换" }
                    } else {
                    let (legacy,reason) = try PhotoHDR.legacy(image)
                    hdr["reason"] = reason
                    if let legacy = legacy {
                        let oldMap = work.appendingPathComponent("old-gain.jpg"), newMap = work.appendingPathComponent("gain.jpg")
                        try Data(legacy.image).write(to: oldMap)
                        var result = try legacyGain(["input":oldMap.path,"gain_output":newMap.path,"stops":legacy.stops])
                        let icc = (result.removeValue(forKey: "srgb_icc_base64") as? String).flatMap { Data(base64Encoded:$0) }
                        if result["status"] as? String == "converted", let icc = icc {
                            let tagged = try PhotoHDR.colorProfile(image, srgb: Array(icc))
                            image = try PhotoHDR.ultra(tagged, gainJPEG: Array(Data(contentsOf:newMap)), stops: legacy.stops)
                            result["reason"] = "已有旧 Apple 增益图已转换，主图未重新编码"
                            hdr.merge(result) { _,new in new }; hdr["output"] = "ultra-hdr"
                        } else { hdr["reason"] = result["reason"] ?? "旧版增益图无法转换，已保留原始数据" }
                    }
                    }
                } catch { warnings.append(error.localizedDescription); hdr["reason"] = "旧 HDR 尝试转换未完成，原格式已保留" }
            }
        }
        let uuid = UUID().uuidString
        let still = try PhotoFormat.liveStill(image, uuid: uuid)
        guard try PhotoHDR.honorPrivate(still) == honorHDR else { throw Failure("荣耀私有 HDR 静态扩展字节校验失败") }
        guard try (isHEIC ? (PhotoHEIF.layout(still).boxes.allSatisfy { !["mpvd","moov"].contains($0.type) }) : MLJPEG.staticOnly(still) == still) else { throw Failure("导出静态图片仍包含图片范围之外的数据") }
        guard try PhotoFormat.codingHash(still) == PhotoFormat.codingHash(photo.still) else { throw Failure("静态主图压缩数据发生变化") }
        let jpg = stage.appendingPathComponent(names["still"]!), mov = stage.appendingPathComponent(names["video"]!)
        try Data(still).write(to:jpg)
        var request: [String:Any] = ["input":sourceVideo.path,"output":mov.path,"still":jpg.path,"identifier":uuid,
            "missing_time":options.missingTime,"time_domain":options.timeDomain,"video_track":options.videoTrack]
        if let timestamp = photo.timestamp { request["timestamp_us"] = timestamp }
        if let frame = photo.coverFrame { request["cover_frame_index"] = frame }
        if photo.timestampTag == "MP4:motionphoto-image-meta" {
            request["timestamp_source"] = "video-metadata"
            warnings.append("主 XMP 缺失；已从内嵌视频元数据恢复原始封面时间")
        }
        let timing = try movie(request)
        if let date = try source.resourceValues(forKeys:[.contentModificationDateKey]).contentModificationDate {
            for url in [jpg,mov] { try fm.setAttributes([.modificationDate:date], ofItemAtPath:url.path) }
        }
        var hashes: [String:String] = [:]
        for name in names.values { hashes[name] = try hashFile(stage.appendingPathComponent(name)) }
        var manifest: [String:Any] = ["version":"1.1.4","implementation":revision,"source":source.path,
            "source_sha256":sourceHash,"identifier":uuid,"options":options.json,
            "output_files":names,"original_filename":source.lastPathComponent,
            "source_timestamp_us":photo.timestamp.map { $0 as Any } ?? NSNull(),
            "source_timestamp_tag":photo.timestampTag.map { $0 as Any } ?? NSNull(),
            "source_cover_frame_index":photo.coverFrame.map { $0 as Any } ?? NSNull(),"source_format":photo.format,"still_contains_embedded_media":false,
            "timing":timing,"hdr":hdr,"warnings":warnings,"primary_coding_sha256":try PhotoFormat.codingHash(still),
            "auxiliary_sha256":try PhotoFormat.auxiliaryHashes(still),"output_sha256":hashes]
        if let auxiliary = auxiliary { manifest["auxiliary_video"] = auxiliary }
        try json(manifest,to:stage.appendingPathComponent("manifest.json"))
        try publish(stage,target:target,source:source,sourceHash:sourceHash,token:token,overwrite:overwrite,exportDataFiles:options.exportDataFiles)
        return ["status":"converted","source":source.path,"output":target.path,"timing":timing,"hdr":hdr,"warnings":warnings,
            "detail":warnings.isEmpty ? (hdr["reason"] as? String ?? "已转换并校验") : warnings.joined(separator:"；")]
    }
    static func run(source: URL, output: URL, options: ConversionOptions, token: StopToken,
                    progress: (BatchProgress)->Void) throws -> BatchOutcome {
        try run(sources: [source], output: output, options: options, token: token, progress: progress)
    }
    static func run(sources: [URL], output: URL, options: ConversionOptions, token: StopToken,
                    existingOutput: ((URL,URL)->ExistingOutputDecision)? = nil,
                    progress: (BatchProgress)->Void) throws -> BatchOutcome {
        let destination = normalized(output)
        try validateOutput(sources:sources,output:destination)
        let (base, entries) = try options.direction == "to-android" ? AndroidExport.selection(sources) : selection(sources)
        try FileManager.default.createDirectory(at:destination,withIntermediateDirectories:true)
        var results: [[String:Any]] = []
        var counts = ["converted":0,"resumed":0,"skipped":0,"failed":0]
        for (index,file) in entries.enumerated() {
            if token.isCancelled { break }
            let rel = try relative(file,root:base)
            let target = destination.appendingPathComponent(rel + (options.direction == "to-android" ? ".motionphoto" : options.outputMode == "auxiliary-only" ? ".auxiliary-export" : ".livephoto"))
            var overwrite = false
            if FileManager.default.fileExists(atPath:target.path), let decide = existingOutput {
                switch decide(file,target) {
                case .skip:
                    counts["skipped",default:0] += 1
                    let detail = "输出已存在，已按选择跳过；已有文件未改动"
                    results.append(["status":"skipped","source":file.path,"output":target.path,"detail":detail])
                    progress(BatchProgress(index:index + 1,total:entries.count,relative:rel,status:"skipped",detail:detail))
                    continue
                case .overwrite: overwrite = true
                case .cancel: token.cancel()
                }
            }
            if token.isCancelled { break }
            let result: [String:Any] = autoreleasepool {
                do {
                    if options.direction == "to-android" { return try AndroidExport.convert(source:file,target:target,options:options,token:token,overwrite:overwrite) }
                    return try convert(source:file,target:target,options:options,token:token,overwrite:overwrite) }
                catch {
                    let skipped = error.localizedDescription == PhotoFormat.notMotion
                    return ["status":skipped ? "skipped" : "failed","source":file.path,"error":error.localizedDescription,"detail":error.localizedDescription]
                }
            }
            if token.isCancelled { break }
            let status = result["status"] as? String ?? "failed"
            counts[status,default:0] += 1; results.append(result)
            progress(BatchProgress(index:index + 1,total:entries.count,relative:rel,status:status,detail:result["detail"] as? String ?? ""))
        }
        let stamp = ISO8601DateFormatter().string(from:Date()).replacingOccurrences(of:":",with:"-")
        let report = options.exportDataFiles ? destination.appendingPathComponent("report-\(stamp)-\(UUID().uuidString.prefix(6)).json") : nil
        let data: [String:Any] = ["counts":counts,"cancelled":token.isCancelled,"files":results]
        if let report = report { try json(data,to:report) }
        return BatchOutcome(counts:counts,report:report,reportData:data,cancelled:token.isCancelled)
    }
}
