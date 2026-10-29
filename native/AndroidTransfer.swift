import Foundation
import AVFoundation
import ImageIO

// Google-standard JPEG input only; this never calls the Apple pair converter.
enum AndroidTransfer {
    static let revision = "google-to-honor-a-modern-1"
    static func inspect(_ data: [UInt8], parsed: NativeMotionPhoto? = nil) throws -> NativeMotionPhoto {
        do { return try inspectStructure(data, parsed:parsed) }
        catch { throw rejection(error) }
    }
    static func rejection(_ error: Error) -> Failure {
        let reason = error.localizedDescription
        let classified = ["来源不支持","文件损坏","封面时间不明确","结构无法无损保留"].contains { reason.hasPrefix($0) }
        return Failure(classified ? reason : "文件损坏：" + reason)
    }
    private static func inspectStructure(_ data: [UInt8], parsed: NativeMotionPhoto?) throws -> NativeMotionPhoto {
        #if MOTIONLIVE_SWIFT_REFERENCE
        guard data.starts(with:[255,216]) else { throw Failure("来源不支持：暂仅支持 Google → 荣耀的 Motion Photo JPEG，不支持 HEIC") }
        guard let image = CGImageSourceCreateWithData(Data(data) as CFData,nil), CGImageSourceGetStatus(image) == .statusComplete, CGImageSourceCreateImageAtIndex(image,0,[kCGImageSourceShouldCacheImmediately:true] as CFDictionary) != nil else { throw Failure("文件损坏：JPEG 图像无法完整读取") }
        let packets = try MLXMP.packets(data)
        let modern = try MLXMP.integer(packets,"MotionPhoto")
        let legacy = try MLXMP.integer(packets,"MicroVideo")
        guard modern == 1 || legacy == 1 else { throw Failure("来源不支持：缺少 Google Motion Photo 声明") }
        guard modern == nil || modern == 1, legacy == nil || legacy == 1 else { throw Failure("文件损坏：Google 动态声明冲突") }
        for key in ["MotionPhotoVersion","MicroVideoVersion"] {
            if let version = try MLXMP.integer(packets,key), version != 1 { throw Failure("来源不支持：未知 Google 动态版本") }
        }
        for packet in packets {
            for node in MLXMP.all(packet.document) {
                let nodes: [XMLNode] = [node] + (node.attributes ?? [])
                for value in nodes {
                    let uri = MLXMP.uri(value) ?? ""
                    guard ![VendorMotion.oppo,VendorMotion.vivo].contains(uri), !uri.lowercased().contains("samsung"), !uri.lowercased().contains("xiaomi"), !uri.lowercased().contains("huawei"), !uri.lowercased().contains("honor") else { throw Failure("来源不支持：检测到其他厂商动态结构") }
                    if uri == MLXMP.hdr && value.localName == "Version", value.stringValue != "1.0" { throw Failure("结构无法无损保留：未知增益图版本") }
                }
            }
        }
        let photo = try parsed ?? PhotoFormat.parse(data)
        guard photo.format == "google-compatible", photo.coverFrame == nil else { throw Failure("来源不支持：不是 Google 标准动态封装") }
        let ranges = try MLJPEG.imageRanges(data).sorted { $0.lowerBound < $1.lowerBound }
        guard ranges.count == 1 || ranges.count == 2 && photo.ultra else { throw Failure("结构无法无损保留：附加图片无法确认是增益图") }
        var end = 0
        for range in ranges {
            guard range.lowerBound == end else { throw Failure("结构无法无损保留：图片之间存在未解释载荷") }
            end = range.upperBound
        }
        guard end + photo.video.count == data.count, Array(data[end...]) == photo.video else { throw Failure("结构无法无损保留：存在额外尾标、间隙或未解释载荷") }
        let offset = try MLXMP.integer(packets,"MicroVideoOffset")
        guard offset == nil || offset == photo.video.count else { throw Failure("文件损坏：旧版偏移与视频长度冲突") }
        let directories = packets.flatMap { MLXMP.all($0.document) }.filter { MLXMP.uri($0) == MLXMP.container && $0.localName == "Directory" }
        guard directories.count <= 1, modern != 1 || directories.count == 1 else { throw Failure("结构无法无损保留：Google 目录缺失或重复") }
        if let directory = directories.first {
            for node in MLXMP.all(directory) {
                let uri = MLXMP.uri(node), name = node.localName ?? ""
                let structural = uri == MLXMP.container && ["Directory","Item"].contains(name) || uri == MLXMP.rdf && ["Seq","li"].contains(name)
                let property = uri == MLXMP.item && ["Semantic","Mime","Length","Padding"].contains(name)
                guard structural || property else { throw Failure("结构无法无损保留：目录包含未知字段") }
                for attribute in node.attributes ?? [] {
                    let known = MLXMP.uri(attribute) == MLXMP.item && ["Semantic","Mime","Length","Padding"].contains(attribute.localName ?? "")
                    let resource = MLXMP.uri(attribute) == MLXMP.rdf && attribute.localName == "parseType" && attribute.stringValue == "Resource"
                    guard known || resource else { throw Failure("结构无法无损保留：目录包含未知属性") }
                }
            }
            let items = MLXMP.all(directory).filter { MLXMP.property($0,MLXMP.item,"Semantic") != nil }
            let semantics = items.compactMap { MLXMP.property($0,MLXMP.item,"Semantic") }
            guard semantics == (ranges.count == 2 ? ["Primary","GainMap","MotionPhoto"] : ["Primary","MotionPhoto"]), ranges.count <= 2 else { throw Failure("结构无法无损保留：目录含未知或重复载荷") }
            for (i,item) in items.enumerated() {
                for name in ["Semantic","Mime","Length","Padding"] {
                    guard MLXMP.values(item,MLXMP.item,name).count <= 1 else { throw Failure("文件损坏：目录属性重复或冲突") }
                }
                let semantic = semantics[i]
                let expected = semantic == "Primary" ? 0 : semantic == "GainMap" ? ranges[1].count : photo.video.count
                let length = MLXMP.property(item,MLXMP.item,"Length") ?? "0"
                let padding = MLXMP.property(item,MLXMP.item,"Padding") ?? "0"
                let mime = MLXMP.property(item,MLXMP.item,"Mime")
                guard Int(length) == expected, Int(padding) == 0, mime == (semantic == "MotionPhoto" ? "video/mp4" : "image/jpeg") else { throw Failure("文件损坏：目录长度、类型或填充与实际载荷不符") }
            }
        }
        let times = try [MLXMP.integer(packets,"MotionPhotoPresentationTimestampUs"),MLXMP.integer(packets,"MicroVideoPresentationTimestampUs")].compactMap { $0 }
        guard !times.isEmpty, times.allSatisfy({ $0 >= 0 }), Set(times).count == 1, photo.timestamp == times.first else { throw Failure("封面时间不明确：时间缺失、无效或声明冲突") }
        // Validate the exact transformation before accepting an input, including gain maps.
        let result = try AndroidExport.assemble(still:photo.still,video:photo.video,options:AndroidOptions(vendor:"google"),time:times[0],frame:0)
        try validate(photo, result:result)
        return photo
        #else
        // Strict structure check + write via Rust; the two checks that
        // cannot move stay in Swift: the CGImageSource full-decode
        // check, and the XMP metadata tree comparison in validate().
        // Rust strict errors reuse the Swift category prefixes, so
        // rejection() classifies them unchanged.
        guard data.starts(with:[255,216]) else { throw Failure("来源不支持：暂仅支持 Google → 荣耀的 Motion Photo JPEG，不支持 HEIC") }
        guard let image = CGImageSourceCreateWithData(Data(data) as CFData,nil), CGImageSourceGetStatus(image) == .statusComplete, CGImageSourceCreateImageAtIndex(image,0,[kCGImageSourceShouldCacheImmediately:true] as CFDictionary) != nil else { throw Failure("文件损坏：JPEG 图像无法完整读取") }
        let photo = try parsed ?? PhotoFormat.parse(data)
        let (bytes, _) = try RustCore.vendorConvert(motion:data,target:"google")
        try validate(photo, result:bytes)
        return photo
        #endif
    }

    static func metadata(_ still:[UInt8]) throws -> [String] {
        // MPF offsets and Google dynamic/HDR directory declarations are rewritten.
        // All other APP/COM bytes and XMP leaf properties must survive.
        var values: [String] = []
        for segment in try MLJPEG.segments(still) {
            guard (224...239).contains(segment.marker) || segment.marker == 254 else { continue }
            if segment.marker == 226 && segment.payload.starts(with:MLJPEG.mpf) { continue }
            if segment.marker == 225 && (segment.payload.starts(with:MLXMP.header) || segment.payload.starts(with:MLXMP.extended)) { continue }
            values.append("\(segment.marker):" + sha(Data(segment.payload)))
        }
        func rewritten(_ node: XMLNode) -> Bool {
            let uri = MLXMP.uri(node), name = node.localName ?? ""
            return uri == MLXMP.container && name == "Directory" || uri == MLXMP.hdr && name == "Version" ||
                uri == MLXMP.note && name == "HasExtendedXMP" || uri == MLXMP.camera &&
                ["MotionPhoto","MotionPhotoVersion","MotionPhotoPresentationTimestampUs","MicroVideo","MicroVideoVersion","MicroVideoOffset","MicroVideoPresentationTimestampUs"].contains(name)
        }
        // Compare full property trees, including RDF arrays and resource attributes;
        // prefix changes and regrouping standard/extended packets are immaterial.
        func tree(_ element: XMLElement) throws -> String? {
            if rewritten(element) { return nil }
            var attributes: [String] = []
            for attribute in element.attributes ?? [] where !rewritten(attribute) {
                attributes.append((MLXMP.uri(attribute) ?? "") + ":" + (attribute.localName ?? "") + "=" + (attribute.stringValue ?? ""))
            }
            let children = element.children?.compactMap { $0 as? XMLElement } ?? []
            let nested = try children.compactMap { try tree($0) }
            let fields: [Any] = [MLXMP.uri(element) ?? "",element.localName ?? "",attributes.sorted(),nested,children.isEmpty ? element.stringValue ?? "" : ""]
            return String(decoding:try JSONSerialization.data(withJSONObject:fields),as:UTF8.self)
        }
        for packet in try MLXMP.packets(still) {
            // The assembler copies RDF descriptions. Unknown data outside those
            // resources must be detected rather than disappearing during normalization.
            for element in MLXMP.all(packet.document) {
                let uri = MLXMP.uri(element), name = element.localName ?? ""
                let wrapper = uri == "adobe:ns:meta/" && name == "xmpmeta" || uri == MLXMP.rdf && ["RDF","Description"].contains(name)
                var parent: XMLNode? = element.parent
                var insideResource = false
                while let node = parent {
                    if MLXMP.uri(node) == MLXMP.rdf && node.localName == "Description" && node.parent?.localName == "RDF" { insideResource = true; break }
                    parent = node.parent
                }
                if !wrapper && !insideResource, let value = try tree(element) { values.append("outside-resource:" + value) }
                if wrapper && name != "Description" {
                    for attribute in element.attributes ?? [] where attribute.localName != "xmptk" {
                        values.append("wrapper:" + (MLXMP.uri(attribute) ?? "") + ":" + (attribute.localName ?? "") + "=" + (attribute.stringValue ?? ""))
                    }
                }
            }
            for description in MLXMP.all(packet.document).filter({ MLXMP.uri($0) == MLXMP.rdf && $0.localName == "Description" && $0.parent?.localName == "RDF" }) {
                for attribute in description.attributes ?? [] where !rewritten(attribute) {
                    // Empty rdf:about is merely an anonymous resource wrapper.
                    if MLXMP.uri(attribute) == MLXMP.rdf && attribute.localName == "about" && attribute.stringValue == "" { continue }
                    values.append((MLXMP.uri(attribute) ?? "") + ":" + (attribute.localName ?? "") + "=" + (attribute.stringValue ?? ""))
                }
                for child in description.children?.compactMap({ $0 as? XMLElement }) ?? [] {
                    if let value = try tree(child) { values.append(value) }
                }
            }
        }
        return values.sorted()
    }
    static func validate(_ photo:NativeMotionPhoto, result:[UInt8]) throws {
        let after = try PhotoFormat.parse(result)
        guard after.video == photo.video, after.timestamp == photo.timestamp,
              try PhotoFormat.codingHash(after.still) == PhotoFormat.codingHash(photo.still),
              try PhotoFormat.auxiliaryHashes(after.still) == PhotoFormat.auxiliaryHashes(photo.still),
              try metadata(after.still) == metadata(photo.still) else { throw Failure("结构无法无损保留：图像、EXIF、增益图或必要元数据校验失败") }
    }
    static func cover(_ photo:NativeMotionPhoto, folder:URL) throws -> [String:Any] {
        #if MOTIONLIVE_SWIFT_REFERENCE
        let file = folder.appendingPathComponent("source.mp4")
        try Data(photo.video).write(to:file)
        let asset = AVURLAsset(url:file)
        guard let track = asset.tracks(withMediaType:.video).first else { throw Failure("文件损坏：视频缺少画面轨") }
        let info: TrackInfo
        do {
            info = try trackInfo(asset,track)
            for other in asset.tracks(withMediaType:.audio) + Array(asset.tracks(withMediaType:.video).dropFirst()) { _ = try trackInfo(asset,other) }
        } catch { throw rejection(error) }
        do {
            let timing = try sourceCoverTiming(["timestamp_us":photo.timestamp!,"time_domain":"auto","missing_time":"error"],video:info,selection:"primary",primaryInfo:{info})
            return ["cover_timestamp_us":photo.timestamp!,"mapped_seconds":seconds(timing.mapped),"domain":timing.domain,"video_sha256":sha(Data(photo.video))]
        } catch { throw Failure("封面时间不明确：" + error.localizedDescription) }
        #else
        // Edit-list-aware mapping from bytes; the folder argument is
        // kept for the callers but no longer needs a source.mp4 file.
        let mapping: RustCore.CoverMapping
        do {
            mapping = try RustCore.coverMap(video:photo.video,optsJSON:try RustCore.optsJSON(
                ["timestamp_us":photo.timestamp!,"time_domain":"auto","missing_time":"error"]))
        } catch { throw Failure("封面时间不明确：" + error.localizedDescription) }
        guard let mapped = mapping.mapped_us, let domain = mapping.domain else {
            throw Failure("封面时间不明确：映射结果不完整")
        }
        return ["cover_timestamp_us":photo.timestamp!,"mapped_seconds":Double(mapped) / 1e6,"domain":domain,"video_sha256":sha(Data(photo.video))]
        #endif
    }
    static func scanFile(_ source:URL, relative:String, bytes:[UInt8]? = nil, parsed:Result<NativeMotionPhoto,Error>? = nil) -> ScanRow {
        let before = SourceVersion.read(source)
        do {
            let data = try bytes ?? Array(Data(contentsOf:source))
            let photo = try inspect(data,parsed:parsed?.get())
            let folder = FileManager.default.temporaryDirectory.appendingPathComponent("motionlive-inspect-" + UUID().uuidString)
            defer { try? FileManager.default.removeItem(at:folder) }
            try FileManager.default.createDirectory(at:folder,withIntermediateDirectories:true)
            _ = try cover(photo,folder:folder)
            var row = ScanRow(source:source,relative:relative,isMotion:true,hdr:photo.ultra ? "Ultra HDR · 保留" : "原始 JPEG",cover:String(format:"%.6f 秒",Double(photo.timestamp!) / 1e6),detail:"Google → 荣耀；原视频与源封面保持不变",manufacturer:"Google 标准",manufacturerDetail:"依据 XMP、目录及实际媒体结构")
            row.version = before == SourceVersion.read(source) ? before : nil
            return row
        } catch {
            var row = NativeBatch.scanFile(source,relative:relative,bytes:bytes,parsed:parsed)
            row = ScanRow(source:source,relative:relative,isMotion:false,hdr:row.hdr,cover:row.cover,detail:rejection(error).localizedDescription,manufacturer:row.manufacturer,manufacturerDetail:row.manufacturerDetail,confirmedStatic:row.confirmedStatic,version:row.version)
            return row
        }
    }
    static func convert(source:URL,target:URL,options:ConversionOptions,token:StopToken,overwrite:Bool,timings:BatchTimings) throws -> [String:Any] {
        let fm = FileManager.default, hash = try NativeBatch.hashFile(source)
        if !overwrite && fm.fileExists(atPath:target.path) {
            let previous = try NativeBatch.verify(target)
            guard previous["implementation"] as? String == revision, previous["source_sha256"] as? String == hash,
                  let old = previous["options"] as? [String:Any], (old as NSDictionary).isEqual(to:options.json) else { throw Failure("已有输出的来源或设置不同，请选择覆盖或跳过") }
            return ["status":"resumed","source":source.path,"output":target.path,"detail":"已校验 Google → 荣耀结果，跳过重复转换"]
        }
        if token.isCancelled { throw Failure("已停止") }
        let stage = target.deletingLastPathComponent().appendingPathComponent(".motionlive-" + UUID().uuidString)
        defer { try? fm.removeItem(at:stage) }
        try fm.createDirectory(at:stage,withIntermediateDirectories:true)
        let data = Array(Data(contentsOf:source))
        let photo = try timings.measure("input_validation") { try inspect(data) }
        let timing = try cover(photo,folder:stage)
        #if MOTIONLIVE_SWIFT_REFERENCE
        try fm.removeItem(at:stage.appendingPathComponent("source.mp4"))
        let result = try AndroidExport.assemble(still:photo.still,video:photo.video,options:AndroidOptions(vendor:"google"),time:photo.timestamp!,frame:0)
        #else
        // The new cover() writes no source.mp4, so there is nothing to
        // remove; the conversion bytes come from Rust directly.
        let (result, _) = try RustCore.vendorConvert(motion:data,target:"google")
        #endif
        let name = source.deletingPathExtension().lastPathComponent + ".MP.jpg", file = stage.appendingPathComponent(name)
        try Data(result).write(to:file)
        try validate(photo,result:Array(Data(contentsOf:file)))
        let hdr: [String:Any] = ["status":"preserved","reason":"JPEG、EXIF、原视频、源封面及增益图保持不变"]
        let manifest: [String:Any] = ["implementation":revision,"source":source.path,"source_sha256":hash,"options":options.json,"output_files":["motionphoto":name],"output_sha256":[name:sha(Data(result))],"timing":timing,"hdr":hdr]
        try NativeBatch.json(manifest,to:stage.appendingPathComponent("manifest.json"))
        try NativeBatch.publish(stage,target:target,source:source,sourceHash:hash,token:token,overwrite:overwrite,exportDataFiles:options.exportDataFiles)
        return ["status":"converted","source":source.path,"output":target.path,"output_files":["motionphoto":name],"timing":timing,"hdr":hdr,"detail":"Google → 荣耀：已无损保留原视频和源封面并校验"]
    }
}
