import Foundation

#if !MOTIONLIVE_SWIFT_REFERENCE
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

    /// One JSON-string C ABI call (coding hash, auxiliary hashes).
    /// Throws the embedded error string, or a generic failure when the
    /// call itself fails (NULL pointers, panic).
    private static func callJSON(
        _ input: [UInt8],
        _ fn: (UnsafePointer<UInt8>?, UInt, UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?) -> Int32
    ) throws -> Reply {
        var jsonPtr: UnsafeMutablePointer<CChar>?
        let rc = input.withUnsafeBufferPointer { buf in
            fn(buf.baseAddress, UInt(buf.count), &jsonPtr)
        }
        guard rc == 0, let json = jsonPtr else { throw Failure("Rust bridge: call failed") }
        defer { momentstays_free_string(json) }
        let reply = try JSONDecoder().decode(Reply.self, from: Data(String(cString: json).utf8))
        if let error = reply.error { throw Failure(error) }
        return reply
    }

    static func parse(_ input: [UInt8]) throws -> NativeMotionPhoto {
        var jsonPtr: UnsafeMutablePointer<CChar>?
        var stillPtr: UnsafePointer<UInt8>?
        var stillLen: UInt = 0
        let rc = input.withUnsafeBufferPointer { buf in
            momentstays_parse(buf.baseAddress, UInt(buf.count), &jsonPtr, &stillPtr, &stillLen)
        }
        guard rc == 0, let json = jsonPtr else { throw Failure("Rust bridge: parse failed") }
        defer { momentstays_free_string(json) }
        let reply = try JSONDecoder().decode(Reply.self, from: Data(String(cString: json).utf8))
        if let error = reply.error { throw Failure(error) }
        let still: [UInt8] = {
            if let p = stillPtr, stillLen > 0 {
                return Array(UnsafeBufferPointer(start: p, count: Int(stillLen)))
            }
            return []
        }()
        momentstays_free_bytes(stillPtr.map { UnsafeMutablePointer(mutating: $0) }, stillLen)
        guard let start = reply.video_start, let end = reply.video_end,
              start >= 0, end >= start, end <= input.count,
              let ultra = reply.ultra, let format = reply.format else {
            throw Failure("Rust bridge: invalid parse response")
        }
        return NativeMotionPhoto(still: still, video: Array(input[start..<end]),
            timestamp: reply.timestamp, timestampTag: reply.timestamp_tag, ultra: ultra,
            coverFrame: reply.cover_frame, format: format)
    }

    static func codingHash(_ input: [UInt8]) throws -> String {
        let reply = try callJSON(input, momentstays_coding_hash)
        guard let hash = reply.hash else { throw Failure("Rust bridge: missing coding hash") }
        return hash
    }

    static func auxiliaryHashes(_ input: [UInt8]) throws -> [String] {
        let reply = try callJSON(input, momentstays_auxiliary_hashes)
        guard let hashes = reply.hashes else { throw Failure("Rust bridge: missing auxiliary hashes") }
        return hashes
    }

    // MARK: - Conversion entries

    /// Thrown when a conversion entry returns "needs_pixel_work":
    /// the caller performs the named pixel step, then retries the
    /// same entry with prepared bytes.
    struct NeedsPixelWork: Error {
        var reason: String
        var stops: Double?
        var detail: String?
    }

    struct CoverMapping: Decodable {
        var frame: Int?
        var frame_time_us: Int?
        var frame_duration_us: Int?
        var mapped_us: Int?
        var domain: String?
        var policy: String?
        var error_us: Int?
    }

    struct HdrInfo: Decodable {
        var input: String?
        var status: String?
        var reason: String?
        var output: String?
    }

    struct ToLiveResult {
        var still: [UInt8]
        var mov: [UInt8]
        var cover: CoverMapping
        var hdr: HdrInfo
        var videoTracks: Int
    }

    struct ToMotionResult {
        var bytes: [UInt8]
        var hdr: HdrInfo
        var coverTimeUs: Int?
        var coverFrame: Int?
    }

    struct ScanRowResult: Decodable {
        var is_motion: Bool?
        var hdr: String?
        var cover: String?
        var detail: String?
        var manufacturer: String?
        var manufacturer_detail: String?
        var confirmed_static: Bool?
    }

    struct CandidateTime: Decodable {
        var index: Int?
        var timestamp_us: Int?
        var time_source: String?
        var sample_time_us: Int?
    }

    struct LivePairResult: Decodable {
        var still_identifier: String?
        var movie_identifier: String?
        var cover_time_us: Int?
        var cover_frame: Int?
        var domain: String?
        var policy: String?
        var maker33: Bool?
    }

    struct AuxiliaryExportResult {
        var mov: [UInt8]
        var repaired: Bool
        var repairMethod: String?
        var repairedDurationSeconds: Double?
        var originalSampleSha256: String?
    }

    /// Decode a JSON string from a C ABI call without freeing it
    /// twice: the caller owns the pointer and frees it via defer.
    private static func jsonString(_ jsonPtr: UnsafeMutablePointer<CChar>?) throws -> String {
        guard let json = jsonPtr else { throw Failure("Rust bridge: call failed") }
        defer { momentstays_free_string(json) }
        return String(cString: json)
    }

    /// Dispatch on the `status` field of a conversion JSON reply:
    /// "ok" passes, "needs_pixel_work" throws NeedsPixelWork,
    /// anything else throws the embedded error.
    private static func requireOK(_ string: String) throws {
        struct Envelope: Decodable {
            var status: String?
            var error: String?
            var reason: String?
            var stops: Double?
            var detail: String?
        }
        let env = try JSONDecoder().decode(Envelope.self, from: Data(string.utf8))
        switch env.status {
        case "ok": return
        case "needs_pixel_work":
            throw NeedsPixelWork(reason: env.reason ?? "unknown", stops: env.stops, detail: env.detail)
        default: throw Failure(env.error ?? "Rust bridge: conversion failed")
        }
    }

    private static func copyBytes(_ ptr: UnsafePointer<UInt8>?, _ len: UInt) -> [UInt8] {
        guard let p = ptr, len > 0 else { return [] }
        return Array(UnsafeBufferPointer(start: p, count: Int(len)))
    }

    /// Serialize a conversion options dictionary to a JSON string for
    /// the C ABI opts_json parameters.
    static func optsJSON(_ value: [String: Any]) throws -> String {
        let data = try JSONSerialization.data(withJSONObject: value, options: [])
        guard let string = String(data: data, encoding: .utf8) else {
            throw Failure("Rust bridge: options are not valid UTF-8")
        }
        return string
    }

    /// Convert a conversion HDR report to the manifest dictionary
    /// shape (omits a missing output).
    static func hdrDictionary(_ hdr: HdrInfo) -> [String: Any] {
        var result: [String: Any] = [:]
        if let input = hdr.input { result["input"] = input }
        if let status = hdr.status { result["status"] = status }
        if let reason = hdr.reason { result["reason"] = reason }
        if let output = hdr.output { result["output"] = output }
        return result
    }

    private static func freeBytes(_ ptr: UnsafePointer<UInt8>?, _ len: UInt) {
        momentstays_free_bytes(ptr.map { UnsafeMutablePointer(mutating: $0) }, len)
    }

    /// Android motion photo -> Apple Live Photo. `optsJSON` carries
    /// uuid (required), timestamp_us, cover_frame_index,
    /// timestamp_source, time_domain, video_track, missing_time,
    /// still_override_kind. When `stillOverride` is non-nil it
    /// replaces the motion still (pixel-preconverted still retry).
    static func toLive(motion: [UInt8], optsJSON: String, stillOverride: [UInt8]? = nil) throws -> ToLiveResult {
        var jsonPtr: UnsafeMutablePointer<CChar>?
        var stillPtr: UnsafePointer<UInt8>?
        var stillLen: UInt = 0
        var movPtr: UnsafePointer<UInt8>?
        var movLen: UInt = 0
        let rc = motion.withUnsafeBufferPointer { mbuf in
            optsJSON.withCString { opts in
                if let ov = stillOverride {
                    return ov.withUnsafeBufferPointer { obuf in
                        momentstays_to_live(mbuf.baseAddress, UInt(mbuf.count), opts,
                            obuf.baseAddress, UInt(obuf.count), 1,
                            &jsonPtr, &stillPtr, &stillLen, &movPtr, &movLen)
                    }
                }
                return momentstays_to_live(mbuf.baseAddress, UInt(mbuf.count), opts,
                    nil, 0, 0, &jsonPtr, &stillPtr, &stillLen, &movPtr, &movLen)
            }
        }
        guard rc == 0 else { throw Failure("Rust bridge: to_live failed") }
        let string = try jsonString(jsonPtr)
        try requireOK(string)
        let still = copyBytes(stillPtr, stillLen)
        let mov = copyBytes(movPtr, movLen)
        freeBytes(stillPtr, stillLen)
        freeBytes(movPtr, movLen)
        struct Reply: Decodable {
            var cover: CoverMapping?
            var hdr: HdrInfo?
            var video_tracks: Int?
        }
        let reply = try JSONDecoder().decode(Reply.self, from: Data(string.utf8))
        guard let cover = reply.cover, let hdr = reply.hdr, let tracks = reply.video_tracks else {
            throw Failure("Rust bridge: invalid to_live response")
        }
        return ToLiveResult(still: still, mov: mov, cover: cover, hdr: hdr, videoTracks: tracks)
    }

    /// Apple Live Photo -> Android motion photo. `optsJSON` carries
    /// vendor (required), container, cover_time_us, cover_frame,
    /// preserve_jpeg.
    static func toMotion(still: [UInt8], movie: [UInt8], optsJSON: String) throws -> ToMotionResult {
        var jsonPtr: UnsafeMutablePointer<CChar>?
        var outPtr: UnsafePointer<UInt8>?
        var outLen: UInt = 0
        let rc = still.withUnsafeBufferPointer { sbuf in
            movie.withUnsafeBufferPointer { mbuf in
                optsJSON.withCString { opts in
                    momentstays_to_motion(sbuf.baseAddress, UInt(sbuf.count),
                        mbuf.baseAddress, UInt(mbuf.count), opts, &jsonPtr, &outPtr, &outLen)
                }
            }
        }
        guard rc == 0 else { throw Failure("Rust bridge: to_motion failed") }
        let string = try jsonString(jsonPtr)
        try requireOK(string)
        let bytes = copyBytes(outPtr, outLen)
        freeBytes(outPtr, outLen)
        struct Reply: Decodable {
            var hdr: HdrInfo?
            var cover_time_us: Int?
            var cover_frame: Int?
        }
        let reply = try JSONDecoder().decode(Reply.self, from: Data(string.utf8))
        guard let hdr = reply.hdr else { throw Failure("Rust bridge: invalid to_motion response") }
        return ToMotionResult(bytes: bytes, hdr: hdr, coverTimeUs: reply.cover_time_us, coverFrame: reply.cover_frame)
    }

    /// Attach a caller-converted gain map to a base still (Ultra
    /// HDR XMP + MPF index). `srgbICC` inserts the APP2 ICC
    /// profile first when non-nil (the legacy color-profile step).
    static func attachGain(base: [UInt8], gain: [UInt8], stops: Double, srgbICC: [UInt8]? = nil) throws -> [UInt8] {
        var jsonPtr: UnsafeMutablePointer<CChar>?
        var outPtr: UnsafePointer<UInt8>?
        var outLen: UInt = 0
        let rc = base.withUnsafeBufferPointer { bbuf in
            gain.withUnsafeBufferPointer { gbuf in
                if let icc = srgbICC {
                    return icc.withUnsafeBufferPointer { ibuf in
                        momentstays_attach_gain(bbuf.baseAddress, UInt(bbuf.count),
                            gbuf.baseAddress, UInt(gbuf.count), stops,
                            ibuf.baseAddress, UInt(ibuf.count), 1, &jsonPtr, &outPtr, &outLen)
                    }
                }
                return momentstays_attach_gain(bbuf.baseAddress, UInt(bbuf.count),
                    gbuf.baseAddress, UInt(gbuf.count), stops, nil, 0, 0, &jsonPtr, &outPtr, &outLen)
            }
        }
        guard rc == 0 else { throw Failure("Rust bridge: attach_gain failed") }
        let string = try jsonString(jsonPtr)
        try requireOK(string)
        let bytes = copyBytes(outPtr, outLen)
        freeBytes(outPtr, outLen)
        return bytes
    }

    /// Clean a still for Live Photo use. When `updateIdentifier`
    /// is true, `uuid` is written as the EXIF MakerNote content
    /// identifier; otherwise the existing identifier is untouched.
    static func liveStill(_ still: [UInt8], uuid: String, updateIdentifier: Bool) throws -> [UInt8] {
        var jsonPtr: UnsafeMutablePointer<CChar>?
        var outPtr: UnsafePointer<UInt8>?
        var outLen: UInt = 0
        let rc = still.withUnsafeBufferPointer { sbuf in
            uuid.withCString { id in
                momentstays_live_still(sbuf.baseAddress, UInt(sbuf.count), id,
                    updateIdentifier ? 1 : 0, &jsonPtr, &outPtr, &outLen)
            }
        }
        guard rc == 0 else { throw Failure("Rust bridge: live_still failed") }
        let string = try jsonString(jsonPtr)
        try requireOK(string)
        let bytes = copyBytes(outPtr, outLen)
        freeBytes(outPtr, outLen)
        return bytes
    }

    /// One scan row for a file (error rows are inside the JSON;
    /// relative/source paths are filled in by the caller).
    static func scanRow(_ data: [UInt8]) throws -> ScanRowResult {
        var jsonPtr: UnsafeMutablePointer<CChar>?
        let rc = data.withUnsafeBufferPointer { buf in
            momentstays_scan_row(buf.baseAddress, UInt(buf.count), &jsonPtr)
        }
        guard rc == 0 else { throw Failure("Rust bridge: scan_row failed") }
        let string = try jsonString(jsonPtr)
        try requireOK(string)
        return try JSONDecoder().decode(ScanRowResult.self, from: Data(string.utf8))
    }

    /// Edit-list-aware cover mapping. `optsJSON` carries
    /// cover_frame_index, timestamp_us, timestamp_source,
    /// time_domain, video_track, missing_time.
    static func coverMap(video: [UInt8], optsJSON: String) throws -> CoverMapping {
        var jsonPtr: UnsafeMutablePointer<CChar>?
        let rc = video.withUnsafeBufferPointer { vbuf in
            optsJSON.withCString { opts in
                momentstays_cover_map(vbuf.baseAddress, UInt(vbuf.count), opts, &jsonPtr)
            }
        }
        guard rc == 0 else { throw Failure("Rust bridge: cover_map failed") }
        let string = try jsonString(jsonPtr)
        try requireOK(string)
        struct Reply: Decodable { var cover: CoverMapping? }
        let reply = try JSONDecoder().decode(Reply.self, from: Data(string.utf8))
        guard let cover = reply.cover else { throw Failure("Rust bridge: invalid cover_map response") }
        return cover
    }

    /// Auxiliary (second) video track -> silent MOV, with the
    /// zero-duration repair applied inside when the layout allows.
    static func exportAuxiliary(motion: [UInt8]) throws -> AuxiliaryExportResult {
        var jsonPtr: UnsafeMutablePointer<CChar>?
        var outPtr: UnsafePointer<UInt8>?
        var outLen: UInt = 0
        let rc = motion.withUnsafeBufferPointer { mbuf in
            momentstays_export_auxiliary(mbuf.baseAddress, UInt(mbuf.count), &jsonPtr, &outPtr, &outLen)
        }
        guard rc == 0 else { throw Failure("Rust bridge: export_auxiliary failed") }
        let string = try jsonString(jsonPtr)
        try requireOK(string)
        let mov = copyBytes(outPtr, outLen)
        freeBytes(outPtr, outLen)
        struct Reply: Decodable {
            var repaired: Bool?
            var repair_method: String?
            var repaired_duration_seconds: Double?
            var original_sample_sha256: String?
        }
        let reply = try JSONDecoder().decode(Reply.self, from: Data(string.utf8))
        guard let repaired = reply.repaired else { throw Failure("Rust bridge: invalid export_auxiliary response") }
        return AuxiliaryExportResult(mov: mov, repaired: repaired, repairMethod: reply.repair_method,
            repairedDurationSeconds: reply.repaired_duration_seconds, originalSampleSha256: reply.original_sample_sha256)
    }

    /// Strict Google motion-photo conversion (Google -> Honor
    /// layout). The caller keeps the CGImageSource full-decode
    /// check and the XMP metadata tree comparison (validate).
    static func vendorConvert(motion: [UInt8], target: String) throws -> (bytes: [UInt8], coverTimeUs: Int?) {
        var jsonPtr: UnsafeMutablePointer<CChar>?
        var outPtr: UnsafePointer<UInt8>?
        var outLen: UInt = 0
        let rc = motion.withUnsafeBufferPointer { mbuf in
            target.withCString { tgt in
                momentstays_vendor_convert(mbuf.baseAddress, UInt(mbuf.count), tgt, &jsonPtr, &outPtr, &outLen)
            }
        }
        guard rc == 0 else { throw Failure("Rust bridge: vendor_convert failed") }
        let string = try jsonString(jsonPtr)
        try requireOK(string)
        let bytes = copyBytes(outPtr, outLen)
        freeBytes(outPtr, outLen)
        struct Reply: Decodable { var cover_time_us: Int? }
        let reply = try JSONDecoder().decode(Reply.self, from: Data(string.utf8))
        return (bytes, reply.cover_time_us)
    }

    /// Candidate cover timestamps for a two-track video. The
    /// caller still decodes candidate JPEGs with
    /// AVAssetImageGenerator.
    static func candidates(video: [UInt8]) throws -> (repaired: Bool, list: [CandidateTime]) {
        var jsonPtr: UnsafeMutablePointer<CChar>?
        let rc = video.withUnsafeBufferPointer { vbuf in
            momentstays_candidates(vbuf.baseAddress, UInt(vbuf.count), &jsonPtr)
        }
        guard rc == 0 else { throw Failure("Rust bridge: candidates failed") }
        let string = try jsonString(jsonPtr)
        try requireOK(string)
        struct Reply: Decodable {
            var repaired: Bool?
            var candidates: [CandidateTime]?
        }
        let reply = try JSONDecoder().decode(Reply.self, from: Data(string.utf8))
        guard let repaired = reply.repaired, let list = reply.candidates else {
            throw Failure("Rust bridge: invalid candidates response")
        }
        return (repaired, list)
    }

    /// Pair a Live Photo still with its movie (cover + identifiers
    /// + MakerNote 33 presence). `domain` is the cover-time
    /// interpretation domain ("auto" | "media" | "presentation").
    static func livePair(still: [UInt8], movie: [UInt8], domain: String = "auto") throws -> LivePairResult {
        var jsonPtr: UnsafeMutablePointer<CChar>?
        let rc = still.withUnsafeBufferPointer { sbuf in
            movie.withUnsafeBufferPointer { mbuf in
                domain.withCString { dom in
                    momentstays_live_pair(sbuf.baseAddress, UInt(sbuf.count),
                        mbuf.baseAddress, UInt(mbuf.count), dom, &jsonPtr)
                }
            }
        }
        guard rc == 0 else { throw Failure("Rust bridge: live_pair failed") }
        let string = try jsonString(jsonPtr)
        try requireOK(string)
        return try JSONDecoder().decode(LivePairResult.self, from: Data(string.utf8))
    }
}
#endif
