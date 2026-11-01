import Foundation
import AVFoundation
import ImageIO
import CoreGraphics

struct AndroidOptions {
    var vendor = "google"
    var container = "jpeg"
    var hdr = "ultra"
    var imageQuality = 0.95
    var gainQuality = 0.95
    var preserveJPEG = true
    var json: [String:Any] { ["vendor":vendor,"container":container,"hdr":hdr,"image_quality":imageQuality,"gain_quality":gainQuality,"preserve_jpeg":preserveJPEG] }
}

enum AndroidExport {
    struct Pair { let still: URL; let movie: URL; let identifier: String; let cover: CMTime; let frame: Int }
    static func properties(_ url: URL) throws -> [String:Any] {
        guard let source = CGImageSourceCreateWithURL(url as CFURL,nil), let info = CGImageSourceCopyPropertiesAtIndex(source,0,nil) as? [String:Any] else { throw Failure("无法读取实况照片静态图") }
        return info
    }
    static func imageIdentifier(_ url: URL) throws -> String {
        guard let maker = try properties(url)[kCGImagePropertyMakerAppleDictionary as String] as? [String:Any], let id = maker["17"] as? String, !id.isEmpty else { throw Failure("照片缺少 Apple 实况内容标识；请从「照片」导出完整原片与配对视频") }
        return id
    }
    static func videoIdentifier(_ url: URL) throws -> String {
        let asset = AVURLAsset(url:url)
        let values = asset.metadata.filter { ($0.key as? String) == "com.apple.quicktime.content.identifier" }.compactMap { $0.stringValue }
        guard values.count == 1 else { throw Failure("视频缺少唯一实况内容标识") }
        return values[0]
    }
    static func pair(_ still: URL, domain: String = "auto") throws -> Pair {
        let id = try imageIdentifier(still)
        let files = try FileManager.default.contentsOfDirectory(at:still.deletingLastPathComponent(),includingPropertiesForKeys:nil)
            .filter { ["mov","mp4"].contains($0.pathExtension.lowercased()) }
        let matching = files.filter { (try? videoIdentifier($0)) == id }
        guard matching.count == 1 else { throw Failure(matching.isEmpty ? "未找到内容标识匹配的视频；仅分享静态图不能恢复实况" : "多个视频使用同一内容标识，请将正确配对单独放入文件夹") }
        let asset = AVURLAsset(url:matching[0]), times = try timedSamples(asset)
        guard times.count == 1, times[0].0.isNumeric else { throw Failure("实况视频缺少唯一有效 still-image-time 封面标记") }
        guard let track = asset.tracks(withMediaType:.video).first else { throw Failure("实况缺少视频轨") }
        let info = try trackInfo(asset,track), sorted = info.pts.sorted { CMTimeCompare($0,$1) < 0 }
        guard !sorted.isEmpty else { throw Failure("视频没有有效画面") }
        var candidates: [CMTime] = domain == "presentation" ? [] : [times[0].0]
        if domain != "media" {
            for segment in track.segments where !segment.isEmpty && CMTimeRangeContainsTime(segment.timeMapping.target,time:times[0].0) {
                candidates.append(CMTimeMapTimeFromRangeToRange(times[0].0,fromRange:segment.timeMapping.target,toRange:segment.timeMapping.source))
            }
        }
        var matches: [Int] = []
        for candidate in candidates {
            let index = sorted.indices.min { abs(seconds(CMTimeSubtract(sorted[$0],candidate))) < abs(seconds(CMTimeSubtract(sorted[$1],candidate))) }!
            let duration = index + 1 < sorted.count ? seconds(CMTimeSubtract(sorted[index + 1],sorted[index])) : seconds(info.durations.last ?? .invalid)
            if duration > 0 && abs(seconds(CMTimeSubtract(sorted[index],candidate))) <= duration / 2 + 0.000002 && !matches.contains(index) { matches.append(index) }
        }
        guard matches.count == 1, let index = matches.first, seconds(sorted[index]) >= 0 else { throw Failure("封面时间越界或编辑时间线存在歧义；未使用中点猜测，请检查高级时间域选项") }
        return Pair(still:still,movie:matching[0],identifier:id,cover:sorted[index],frame:index)
    }
    static func selection(_ sources: [URL]) throws -> (base:URL,entries:[URL]) {
        var images: [URL] = []
        for source in sources {
            if ["mov","mp4"].contains(source.pathExtension.lowercased()) {
                let id = try videoIdentifier(source)
                let siblings = try FileManager.default.contentsOfDirectory(at:source.deletingLastPathComponent(),includingPropertiesForKeys:nil)
                let matching = siblings.filter { ["jpg","jpeg","heic","heif"].contains($0.pathExtension.lowercased()) && (try? imageIdentifier($0)) == id }
                guard matching.count == 1 else { throw Failure("所选视频缺少唯一匹配的静态照片") }; images.append(matching[0])
            } else { images += try NativeBatch.files(source) }
        }
        guard !images.isEmpty else { throw Failure("未找到完整实况照片") }
        return try NativeBatch.selection(images)
    }
    static func scan(_ sources: [URL], token:StopToken, concurrency:Int? = nil,
                     rowsReady: (([ScanRow])->Void)? = nil, progress:@escaping (Int,Int)->Void) throws -> [ScanRow] {
        let (base,entries) = try selection(sources)
        let relatives = try entries.map { try NativeBatch.relative($0,root:base) }
        var rows: [Int:ScanRow] = [:], completed = 0
        var pending: [ScanRow] = [], lastUpdate = Date.distantPast
        BatchScheduling.perform(files:entries,limit:concurrency ?? BatchScheduling.limit(scanning:true),token:token,work:{ i in
            let url = entries[i], rel = relatives[i]
            return scanFile(url,relative:rel)
        },receive:{ i,row in
            rows[i] = row; completed += 1; pending.append(row)
            if completed == entries.count || Date().timeIntervalSince(lastUpdate) >= 0.1 {
                rowsReady?(pending); pending.removeAll(keepingCapacity:true); lastUpdate = Date()
            }
            progress(completed,entries.count)
        })
        if !pending.isEmpty { rowsReady?(pending) }
        return rows.keys.sorted().compactMap { rows[$0] }
    }
    static func scanFile(_ url:URL, relative:String) -> ScanRow {
            do {
                let value = try pair(url), props = try properties(url)
                let hasMap = (props[kCGImagePropertyMakerAppleDictionary as String] as? [String:Any])?["33"] != nil
                return ScanRow(source:url,relative:relative,isMotion:true,hdr:hasMap ? "Apple HDR" : "原始图像",cover:String(format:"%.6f 秒",seconds(value.cover)),detail:"实况内容标识匹配；封面来自视频 timed metadata",manufacturer:"苹果",manufacturerDetail:"配对依据：Apple ContentIdentifier")
            } catch { return ScanRow(source:url,relative:relative,isMotion:false,hdr:"—",cover:"—",detail:error.localizedDescription,manufacturer:"苹果 / 待检查") }
    }
    static func remux(_ source:URL, to destination:URL, token:StopToken) throws -> [String:Any] {
        let asset = AVURLAsset(url:source), videos = asset.tracks(withMediaType:.video), audios = asset.tracks(withMediaType:.audio)
        guard videos.count == 1, audios.count <= 1 else { throw Failure("实况视频轨道布局不受支持") }
        let tracks = videos + audios, before = try tracks.map { try trackInfo(asset,$0) }
        guard before.allSatisfy({ $0.pts.allSatisfy { $0.isNumeric && seconds($0) >= 0 } }) else { throw Failure("视频含负时间样本，暂不无损重封装") }
        let writer = try AVAssetWriter(outputURL:destination,fileType:.mp4)
        writer.movieTimeScale = 1_000_000
        var work: [(AVAssetReader,AVAssetReaderTrackOutput,AVAssetWriterInput)] = []
        for track in tracks {
            let reader = try AVAssetReader(asset:asset), output = AVAssetReaderTrackOutput(track:track,outputSettings:nil)
            reader.add(output)
            let hint = track.formatDescriptions.first as! CMFormatDescription?
            let input = AVAssetWriterInput(mediaType:track.mediaType,outputSettings:nil,sourceFormatHint:hint)
            input.expectsMediaDataInRealTime = false
            if track.mediaType == .video { input.transform = track.preferredTransform; input.mediaTimeScale = track.naturalTimeScale }
            guard writer.canAdd(input) else { throw Failure("此视频/音频编码不能无损写入 MP4") }
            writer.add(input); work.append((reader,output,input))
        }
        guard writer.startWriting() else { throw Failure(writer.error?.localizedDescription ?? "无法启动 MP4 写入") }
        writer.startSession(atSourceTime:.zero)
        let group = DispatchGroup(), lock = NSLock(); var errors: [String] = []
        for (reader,output,input) in work {
            group.enter(); DispatchQueue.global().async {
                defer { group.leave() }
                do {
                    guard reader.startReading() else { throw Failure("无法读取实况视频") }
                    while let sample = output.copyNextSampleBuffer() {
                        if token.isCancelled { throw Failure("已停止") }
                        guard isMediaSample(sample) else { continue }
                        try waitReady(input,writer)
                        guard input.append(sample) else { throw Failure(writer.error?.localizedDescription ?? "MP4 样本写入失败") }
                    }
                    guard reader.status == .completed else { throw Failure(reader.error?.localizedDescription ?? "视频读取失败") }
                    input.markAsFinished()
                } catch { lock.lock(); errors.append(error.localizedDescription); lock.unlock(); writer.cancelWriting() }
            }
        }
        group.wait(); if let error = errors.first { throw Failure(error) }
        let done = DispatchSemaphore(value:0); writer.finishWriting { done.signal() }; done.wait()
        guard writer.status == .completed else { throw Failure(writer.error?.localizedDescription ?? "MP4 写入未完成") }
        let result = AVURLAsset(url:destination), afterTracks = result.tracks(withMediaType:.video) + result.tracks(withMediaType:.audio)
        guard afterTracks.count == before.count else { throw Failure("重封装丢失了音视频轨") }
        var report: [[String:Any]] = []
        for (i,track) in afterTracks.enumerated() {
            let after = try trackInfo(result,track), original = before[i]
            guard after.hash == original.hash, after.pts.count == original.pts.count, after.sizes == original.sizes else { throw Failure("音视频压缩数据校验失败") }
            let shift = zip(after.pts,original.pts).map { abs(seconds(CMTimeSubtract($0,$1))) }.max() ?? 0
            guard shift < 0.000002 else { throw Failure("重封装改变了样本时间戳") }
            report.append(["type":track.mediaType.rawValue,"sha256":after.hash,"samples":after.pts.count,"max_pts_shift_seconds":shift])
        }
        return ["tracks":report,"video_reencoded":false,"audio_reencoded":false]
    }
    static func xmp(vendor:String, length:Int, time:Int, heic:Bool, gainLength:Int = 0) -> String {
        let extra: String
        if vendor == "oppo" { extra = "xmlns:OpCamera=\"\(VendorMotion.oppo)\" OpCamera:MotionPhotoPrimaryPresentationTimestampUs=\"\(time)\" OpCamera:VideoLength=\"\(length)\"" }
        else if vendor == "vivo" { extra = "xmlns:Vivo=\"\(VendorMotion.vivo)\" Vivo:VMotionPhotoVersion=\"2\" Vivo:VMotionPhotoSource=\"1\"" }
        else { extra = "" }
        let hdr = gainLength > 0 ? "xmlns:hdrgm=\"\(MLXMP.hdr)\" hdrgm:Version=\"1.0\"" : ""
        let gain = gainLength > 0 ? "<rdf:li rdf:parseType=\"Resource\"><Container:Item Item:Semantic=\"GainMap\" Item:Mime=\"image/jpeg\" Item:Length=\"\(gainLength)\"/></rdf:li>" : ""
        return "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"\(MLXMP.rdf)\"><rdf:Description xmlns:GCamera=\"\(MLXMP.camera)\" xmlns:Container=\"\(MLXMP.container)\" xmlns:Item=\"\(MLXMP.item)\" \(extra) \(hdr) GCamera:MotionPhoto=\"1\" GCamera:MotionPhotoVersion=\"1\" GCamera:MotionPhotoPresentationTimestampUs=\"\(time)\"><Container:Directory><rdf:Seq><rdf:li rdf:parseType=\"Resource\"><Container:Item Item:Semantic=\"Primary\" Item:Mime=\"image/\(heic ? "heic" : "jpeg")\" Item:Length=\"0\" Item:Padding=\"\(heic ? 8 : 0)\"/></rdf:li>\(gain)<rdf:li rdf:parseType=\"Resource\"><Container:Item Item:Semantic=\"MotionPhoto\" Item:Mime=\"video/mp4\" Item:Length=\"\(length)\"/></rdf:li></rdf:Seq></Container:Directory></rdf:Description></rdf:RDF></x:xmpmeta>"
    }
    // JPEG readers commonly use a single standard XMP packet. HDR and motion
    // must share one directory: Primary, optional GainMap, MotionPhoto.
    static func unifiedJPEG(_ still:[UInt8], motion:String) throws -> [UInt8] {
        let packets = try MLXMP.packets(still)
        let output = try XMLDocument(xmlString:motion,options:[.nodeLoadExternalEntitiesNever])
        guard let rdf = MLXMP.all(output).first(where: { MLXMP.uri($0) == MLXMP.rdf && $0.localName == "RDF" }) else { throw Failure("动态照片 XMP 缺少 RDF") }
        var changed: Set<Int> = []
        let standard = packets.indices.filter { packets[$0].guid == nil }
        for (i,packet) in packets.enumerated() {
            for element in MLXMP.all(packet.document).reversed() {
                for attr in element.attributes ?? [] where MLXMP.uri(attr) == MLXMP.hdr && attr.localName == "Version" { attr.detach(); changed.insert(i) }
                if (MLXMP.uri(element) == MLXMP.container && element.localName == "Directory") || (MLXMP.uri(element) == MLXMP.hdr && element.localName == "Version") { element.detach(); changed.insert(i) }
            }
            if packet.guid == nil {
                for description in MLXMP.all(packet.document).filter({ MLXMP.uri($0) == MLXMP.rdf && $0.localName == "Description" && MLXMP.uri($0.parent ?? $0) == MLXMP.rdf && $0.parent?.localName == "RDF" }) {
                    let copy = description.copy() as! XMLElement
                    var scope: [String:XMLNode] = [:], ancestors: [XMLElement] = []
                    var ancestor: XMLNode? = description
                    while let value = ancestor { if let element = value as? XMLElement { ancestors.append(element) }; ancestor = value.parent }
                    for element in ancestors.reversed() { for namespace in element.namespaces ?? [] { scope[namespace.name ?? ""] = namespace } }
                    for namespace in scope.values where copy.namespaces?.contains(where: { $0.name == namespace.name }) != true { copy.addNamespace(namespace.copy() as! XMLNode) }
                    rdf.addChild(copy)
                }
            }
        }
        let result: [UInt8]
        if let first = standard.first {
            packets[first].document.setRootElement(output.rootElement()!.copy() as! XMLElement)
            changed.insert(first)
            var replacements = try MLXMP.rewrites(packets,changed:changed)
            for i in standard.dropFirst() { replacements[packets[i].segment.start] = [] }
            result = try MLJPEG.edit(still,replacements:replacements)
        } else { result = try MLJPEG.edit(still,replacements:[:],insert:MLXMP.segment(output)) }
        let after = try MLXMP.packets(result)
        guard after.filter({ $0.guid == nil }).count == 1,
              after.flatMap({ MLXMP.all($0.document) }).filter({ MLXMP.uri($0) == MLXMP.container && $0.localName == "Directory" }).count == 1,
              try MLJPEG.auxiliary(still).dropFirst().map({ sha(Data($0)) }) == MLJPEG.auxiliary(result).dropFirst().map({ sha(Data($0)) }) else { throw Failure("HDR/动态照片合并元数据校验失败") }
        return result
    }
    static func samsungSEF(videoStart:Int, length:Int) throws -> [UInt8] {
        guard videoStart <= Int(UInt32.max), length <= Int(UInt32.max) else { throw Failure("三星 SEF 4 GiB 范围超限") }
        let tag: [UInt8] = [0,0,0x30,0x0a]
        let record = tag + Binary.bytes(16,4,little:true) + Binary.string("MotionPhoto_Data") + Binary.string("mpv2") + Binary.bytes(videoStart,4) + Binary.bytes(length,4)
        let header = Binary.string("SEFH") + Binary.bytes(107,4,little:true) + Binary.bytes(1,4,little:true)
        let directory = header + tag + Binary.bytes(record.count,4,little:true) + Binary.bytes(record.count,4,little:true)
        return record + directory + Binary.bytes(directory.count,4,little:true) + Binary.string("SEFT")
    }
    static func assemble(still:[UInt8], video:[UInt8], options:AndroidOptions, time:Int, frame:Int) throws -> [UInt8] {
        let heic = PhotoHEIF.matches(still), xml = xmp(vendor:options.vendor,length:video.count,time:time,heic:heic)
        let image: [UInt8]
        if heic { image = try HEIFMetadata.write(still,xmp:Binary.string(xml)) }
        else {
            let cleaned = try PhotoFormat.liveStill(still,uuid:"",updateIdentifier:false)
            let maps = try MLJPEG.auxiliary(cleaned)
            guard maps.count <= 2 else { throw Failure("动态照片含多个辅助 JPEG，无法安全建立目录") }
            let document = xmp(vendor:options.vendor,length:video.count,time:time,heic:false,gainLength:maps.count == 2 ? maps[1].count : 0)
            image = try unifiedJPEG(cleaned,motion:document)
        }
        let start = image.count + (heic ? 8 : 0)
        let tail = options.vendor == "samsung" ? try samsungSEF(videoStart:start,length:video.count) : []
        if heic { return image + Binary.bytes(8 + video.count + tail.count,4) + Binary.string("mpvd") + video + tail }
        // Honor Gallery plays the LIVE_ layout differently from the standard
        // XMP layout: a rotated Apple still/video pair can turn during playback.
        // Use its working Google-compatible path without altering either image
        // orientation or the video transform / compressed samples.
        if options.vendor == "huawei" {
            func field(_ text:String) throws -> [UInt8] { guard text.utf8.count <= 20 else { throw Failure("厂商尾标数值超限") }; return Binary.string(text) + [UInt8](repeating:32,count:20 - text.utf8.count) }
            return image + video + (try field("v2_f\(frame)") + field("0:0") + field("LIVE_\(video.count + 20)"))
        }
        return image + video + tail
    }
}

extension AndroidExport {
    static func convert(source:URL,target:URL,options:ConversionOptions,token:StopToken,overwrite:Bool,timings:BatchTimings = BatchTimings()) throws -> [String:Any] {
        let config = options.android, fm = FileManager.default
        guard ["google","xiaomi","samsung","huawei","honor","oppo","vivo"].contains(config.vendor), ["jpeg","heic"].contains(config.container) else { throw Failure("安卓目标厂商或容器无效") }
        if config.container == "heic" && !["google","xiaomi","samsung"].contains(config.vendor) { throw Failure("此厂商的 HEIC 动态封装尚无可验证方案；请选择 JPEG") }
        let sourceHash = try timings.measure("source_hash") { try NativeBatch.hashFile(source) }
        if !overwrite && fm.fileExists(atPath:target.path) {
            let previous = try NativeBatch.verify(target)
            guard previous["implementation"] as? String == "reverse-native-1.2.1" else { throw Failure("已有输出使用旧版 HDR 封装，请选择覆盖以修复") }
            if config.vendor == "honor" {
                guard previous["motion_layout"] as? String == "google-compatible" else { throw Failure("已有荣耀输出使用旧版私有尾标，可能导致视频方向错误；请选择覆盖以修复") }
            }
            guard previous["source_sha256"] as? String == sourceHash, let previousOptions = previous["options"] as? [String:Any], (previousOptions as NSDictionary).isEqual(to:options.json),
                  let oldMovie = previous["paired_video_source"] as? String, let oldHash = previous["paired_video_sha256"] as? String, try NativeBatch.hashFile(URL(fileURLWithPath:oldMovie)) == oldHash else { throw Failure("已有输出的输入或设置不同，请选择覆盖或跳过") }
            return ["status":"resumed","source":source.path,"output":target.path,"detail":"已验证安卓动态照片，跳过重复转换"]
        }
        let paired = try pair(source,domain:options.timeDomain), videoHash = try NativeBatch.hashFile(paired.movie)
        let work = fm.temporaryDirectory.appendingPathComponent("motionlive-reverse-" + UUID().uuidString)
        let stage = target.deletingLastPathComponent().appendingPathComponent(".motionlive-" + UUID().uuidString)
        defer { try? fm.removeItem(at:work); try? fm.removeItem(at:stage) }
        try fm.createDirectory(at:work,withIntermediateDirectories:true)
        try fm.createDirectory(at:stage,withIntermediateDirectories:true)
        if token.isCancelled { throw Failure("已停止") }
        let clip = work.appendingPathComponent("motion.mp4")
        var timing = try remux(paired.movie,to:clip,token:token)
        let video = Array(try Data(contentsOf:clip)), time = Int((seconds(paired.cover) * 1_000_000).rounded())
        let (still,hdr) = try timings.measure("image_and_hdr") { try AndroidHDR.prepare(source,options:config) }
        let result = try assemble(still:still,video:video,options:config,time:time,frame:paired.frame)
        let decoded = try PhotoFormat.parse(result)
        guard decoded.video == video, try PhotoFormat.codingHash(decoded.still) == PhotoFormat.codingHash(still) else { throw Failure("安卓封装读回校验失败：图像或视频数据不同") }
        if config.vendor == "huawei" { guard decoded.coverFrame == paired.frame else { throw Failure("厂商封面帧号校验失败") } }
        else { guard decoded.timestamp == time else { throw Failure("安卓封面时间校验失败") } }
        let name = source.deletingPathExtension().lastPathComponent + ".MP." + (config.container == "heic" ? "heic" : "jpg")
        let file = stage.appendingPathComponent(name); try Data(result).write(to:file)
        if let date = try source.resourceValues(forKeys:[.contentModificationDateKey]).contentModificationDate { try fm.setAttributes([.modificationDate:date],ofItemAtPath:file.path) }
        timing["source_still_image_time_seconds"] = seconds(paired.cover); timing["cover_timestamp_us"] = time; timing["cover_frame_index"] = paired.frame
        var warnings = config.container == "heic" ? ["HEIC 原始 HDR 编码已保留；目标安卓厂商对 Apple 私有 HDR 的显示兼容性需真机验证"] : ["厂商封装已通过本地解析校验；不同手机固件识别与 HDR 显示仍需真机验证"]
        if config.vendor == "honor" { warnings.append("荣耀格式仅作备用，新款荣耀推荐选择 Google / 通用。此兼容性针对留刻导出的文件，不代表能识别 Google 相机原片") }
        let manifest: [String:Any] = ["version":"1.2.1","implementation":"reverse-native-1.2.1","source":source.path,"source_sha256":sourceHash,
            "paired_video_source":paired.movie.path,"paired_video_sha256":videoHash,"options":options.json,"identifier":paired.identifier,
            "motion_layout":config.vendor == "honor" ? "google-compatible" : config.vendor,
            "output_files":["motionphoto":name],"output_sha256":[name:sha(Data(result))],"timing":timing,"hdr":hdr,"warnings":warnings]
        try NativeBatch.json(manifest,to:stage.appendingPathComponent("manifest.json"))
        guard try NativeBatch.hashFile(paired.movie) == videoHash else { throw Failure("配对视频在转换期间发生变化，未发布") }
        try timings.measure("validation_and_publish") { try NativeBatch.publish(stage,target:target,source:source,sourceHash:sourceHash,token:token,overwrite:overwrite,exportDataFiles:options.exportDataFiles) }
        return ["status":"converted","source":source.path,"output":target.path,"detail":"已导出 \(config.vendor) \(config.container.uppercased()) 动态照片；封面 \(Double(time) / 1_000_000) 秒","hdr":hdr,"timing":timing,"warnings":warnings]
    }
}
