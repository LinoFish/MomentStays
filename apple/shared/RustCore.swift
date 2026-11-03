import Foundation

enum RustCore {
    private struct Reply: Decodable {
        var error: String?
        var video_start: Int?
        var video_end: Int?
        var timestamp: Int?
        var timestamp_tag: String?
        var ultra: Bool?
        var cover_frame: Int?
        var format: String?
        var hash: String?
        var hashes: [String]?
    }

    private static func call<T>(_ operation: UInt32, _ input: [UInt8],
                                _ consume: (Reply, UnsafeMutableRawPointer) throws -> T) throws -> T {
        guard let response = input.withUnsafeBufferPointer({ ms_core_call(operation, $0.baseAddress, $0.count) }) else {
            throw Failure("Rust bridge: missing response")
        }
        defer { ms_core_free(response) }
        let reply = try JSONDecoder().decode(Reply.self, from: Data(bytes(response, 0)))
        if let error = reply.error { throw Failure(error) }
        return try consume(reply, response)
    }

    private static func bytes(_ response: UnsafeMutableRawPointer, _ slot: UInt32) throws -> [UInt8] {
        var length = 0
        guard let pointer = ms_core_bytes(response, slot, &length), length >= 0 else {
            throw Failure("Rust bridge: invalid response buffer")
        }
        return Array(UnsafeBufferPointer(start: pointer, count: length))
    }

    static func parse(_ input: [UInt8]) throws -> NativeMotionPhoto {
        try call(0, input) { reply, response in
            guard let start = reply.video_start, let end = reply.video_end,
                  start >= 0, end >= start, end <= input.count,
                  let ultra = reply.ultra, let format = reply.format else {
                throw Failure("Rust bridge: invalid parse response")
            }
            return NativeMotionPhoto(still: try bytes(response, 1), video: Array(input[start..<end]),
                timestamp: reply.timestamp, timestampTag: reply.timestamp_tag, ultra: ultra,
                coverFrame: reply.cover_frame, format: format)
        }
    }

    static func codingHash(_ input: [UInt8]) throws -> String {
        try call(1, input) { reply, _ in
            guard let hash = reply.hash else { throw Failure("Rust bridge: missing coding hash") }
            return hash
        }
    }

    static func auxiliaryHashes(_ input: [UInt8]) throws -> [String] {
        try call(2, input) { reply, _ in
            guard let hashes = reply.hashes else { throw Failure("Rust bridge: missing auxiliary hashes") }
            return hashes
        }
    }
}

import AVFoundation
import ImageIO

// Owns callback closures only for the synchronous Rust call. Rust serializes
// event callbacks; codecs have no shared mutable state.
private final class RustEngineContext {
    var progress: ((BatchProgress)->Void)?
    var existing: ((URL,URL)->ExistingOutputDecision)?
    var rows: (([ScanRow])->Void)?
}
private func rustContext(_ pointer: UnsafeMutableRawPointer?) -> RustEngineContext {
    Unmanaged<RustEngineContext>.fromOpaque(pointer!).takeUnretainedValue()
}
private func rustJSON(_ pointer: UnsafePointer<UInt8>?, _ count: Int) -> Any? {
    guard let pointer = pointer else { return nil }
    return try? JSONSerialization.jsonObject(with:Data(bytes:pointer,count:count))
}

extension RustCore {
    static func engine(_ request: [String:Any], token: StopToken? = nil,
                       progress: ((BatchProgress)->Void)? = nil,
                       existing: ((URL,URL)->ExistingOutputDecision)? = nil,
                       rows: (([ScanRow])->Void)? = nil) throws -> [String:Any] {
        let context = RustEngineContext()
        context.progress = progress; context.existing = existing; context.rows = rows
        var host = MsNative()
        host.context = Unmanaged.passUnretained(context).toOpaque()
        host.progress = { ctx,p,n in
            guard let v = rustJSON(p,n) as? [String:Any] else { return }
            rustContext(ctx).progress?(BatchProgress(index:v["index"] as? Int ?? 0,total:v["total"] as? Int ?? 0,
                relative:v["relative"] as? String ?? "",status:v["status"] as? String ?? "",detail:v["detail"] as? String ?? ""))
        }
        host.existing_output = { ctx,s,t in
            guard let s = s, let t = t, let decide = rustContext(ctx).existing else { return -1 }
            switch decide(URL(fileURLWithPath:String(cString:s)),URL(fileURLWithPath:String(cString:t))) {
            case .skip: return 0
            case .overwrite: return 1
            case .cancel: return 2
            }
        }
        host.rows_ready = { ctx,p,n in
            guard let a = rustJSON(p,n) as? [[String:Any]] else { return }
            rustContext(ctx).rows?(a.compactMap(RustCore.scanRow))
        }
        let input = try JSONSerialization.data(withJSONObject:request,options:[.sortedKeys])
        return try withExtendedLifetime((context,token)) {
            guard let reply = input.withUnsafeBytes({ raw in
                ms_core_engine(&host,raw.bindMemory(to:UInt8.self).baseAddress,raw.count,token?.rustToken)
            }) else { throw Failure("Rust engine: missing response") }
            defer { ms_core_free(reply) }
            let payload = try bytes(reply,0)
            guard let value = try JSONSerialization.jsonObject(with:Data(payload)) as? [String:Any] else { throw Failure("Rust engine: invalid response") }
            if let error = value["error"] as? String { throw Failure(error) }
            return value
        }
    }
    static func scanRow(_ v: [String:Any]) -> ScanRow? {
        guard let source = v["source"] as? String else { return nil }
        var row = ScanRow(source:URL(fileURLWithPath:source),relative:v["relative"] as? String ?? "",isMotion:v["motion"] as? Bool ?? false,
            hdr:v["hdr"] as? String ?? "—",cover:v["cover"] as? String ?? "—",detail:v["detail"] as? String ?? "",
            manufacturer:v["manufacturer"] as? String ?? "未知",manufacturerDetail:v["manufacturer_detail"] as? String ?? "",confirmedStatic:v["static"] as? Bool ?? false)
        if let ver = v["version"] as? [String:Any], let modified = (ver["modified_ns"] as? String).flatMap(Int64.init),
           let changed = (ver["changed_ns"] as? String).flatMap(Int64.init), let dev = ver["device"] as? NSNumber,
           let ino = ver["inode"] as? NSNumber, let size = ver["size"] as? NSNumber {
            row.version = SourceVersion(device:dev.int32Value,inode:ino.uint64Value,size:size.int64Value,
                modified:timespec(tv_sec:Int(modified / 1_000_000_000),tv_nsec:Int(modified % 1_000_000_000)),
                changed:timespec(tv_sec:Int(changed / 1_000_000_000),tv_nsec:Int(changed % 1_000_000_000)))
        }
        return row
    }
    static func rowJSON(_ r: ScanRow) -> [String:Any] {
        var v: [String:Any] = ["source":r.source.path,"relative":r.relative,"motion":r.isMotion,"hdr":r.hdr,"cover":r.cover,"detail":r.detail,
            "manufacturer":r.manufacturer,"manufacturer_detail":r.manufacturerDetail,"static":r.confirmedStatic]
        if let ver = r.version {
            v["version"] = ["device":UInt32(bitPattern:ver.device),"inode":ver.inode,"size":ver.size,
                "modified_ns":String(Int64(ver.modified.tv_sec) * 1_000_000_000 + Int64(ver.modified.tv_nsec)),
                "changed_ns":String(Int64(ver.changed.tv_sec) * 1_000_000_000 + Int64(ver.changed.tv_nsec))] as [String:Any]
        }
        return v
    }
    static func scanFile(_ file: URL, relative: String, direction: String) -> ScanRow {
        do {
            let v = try engine(["mode":"scan-file","input":file.path,"relative":relative,"direction":direction])
            guard let row = scanRow(v) else { throw Failure("Rust engine: invalid scan row") }
            return row
        } catch { return ScanRow(source:file,relative:relative,isMotion:false,hdr:"—",cover:"—",detail:error.localizedDescription) }
    }
    static func batchRequest(_ options: ConversionOptions) -> [String:Any] {
        var req: [String:Any] = [:]
        req["mode"] = "batch"
        req["direction"] = options.direction
        req["legacy_hdr"] = options.legacyHDR
        req["missing_time"] = options.missingTime
        req["time_domain"] = options.timeDomain
        req["video_track"] = options.videoTrack
        req["output_mode"] = options.outputMode
        req["repair_auxiliary_duration"] = options.repairAuxiliaryDuration
        req["export_data_files"] = options.exportDataFiles
        if let index = options.coverIndex { req["cover_candidate_index"] = index }
        req["cover_selections"] = options.coverSelections
        req["manual_covers"] = options.manualCovers.mapValues { $0.json }
        if let choice = options.manualCover { req["manual_cover"] = choice.json }
        req["candidate_delivery"] = options.candidateDelivery
        req["vendor"] = options.android.vendor
        req["container"] = options.android.container
        req["image_quality"] = options.android.imageQuality
        req["gain_quality"] = options.android.gainQuality
        req["preserve_jpeg"] = options.android.preserveJPEG
        return req
    }
    static func run(sources: [URL], output: URL, options: ConversionOptions, token: StopToken, scanRows: [ScanRow]?,
                    concurrency: Int?, existing: ((URL,URL)->ExistingOutputDecision)?, progress: @escaping (BatchProgress)->Void) throws -> BatchOutcome {
        var req = batchRequest(options)
        req["inputs"] = sources.map { $0.path }; req["output"] = output.path; req["include_report_data"] = true
        req["concurrency"] = concurrency ?? BatchScheduling.limit(scanning:false)
        req["ask_existing_output"] = existing != nil
        if let rows = scanRows { req["scan_rows"] = rows.map(rowJSON) }
        let v = try engine(req,token:token,progress:progress,existing:existing)
        return BatchOutcome(counts:v["counts"] as? [String:Int] ?? [:],report:(v["report"] as? String).map { URL(fileURLWithPath:$0) },
            reportData:v["report_data"] as? [String:Any] ?? [:],cancelled:v["cancelled"] as? Bool ?? false)
    }
}
