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
        // Operation 3 (Apple HDR read).
        var container: String?
        var status: String?
        var reason: String?
        var stops: Double?
        var maker: MakerFacts?
        var heic: HeicFacts?
    }
    private struct MakerFacts: Decodable {
        var present: Bool?
        var has_33: Bool?
        var has_48: Bool?
        var stops: Double?
    }
    private struct HeicFacts: Decodable {
        var primary_item: Int?
        var gain_map_item: Int?
        var iso_tmap: Bool?
        var marker: Bool?
        var hdr_signal: Bool?
    }

    /// What Rust read of a still's Apple HDR data (JPEG or HEIC), from the
    /// file bytes alone; the HEVC gain-map pixels of a HEIC are still decoded
    /// by ImageIO. `status`: "none", "legacy" (JPEG gain map in `gain`),
    /// "gainmap" (HEIC with an Apple gain-map item) or "refuse" (HEIC that may
    /// hold HDR that cannot be interpreted: keep the original).
    struct AppleHDRReport {
        var container: String
        var status: String
        var reason: String
        var stops: Double?
        var gain: [UInt8]?
        var hasTag33: Bool
        var isoTonemap: Bool
        var hdrSignal: Bool
        var gainMapItem: Int?
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

    static func appleHDR(_ input: [UInt8]) throws -> AppleHDRReport {
        try call(3, input) { reply, response in
            guard let container = reply.container, let status = reply.status,
                  let reason = reply.reason else {
                throw Failure("Rust bridge: invalid Apple HDR response")
            }
            var gain: [UInt8]? = nil
            if status == "legacy" { gain = try bytes(response, 1) }
            return AppleHDRReport(container: container, status: status, reason: reason, stops: reply.stops,
                gain: gain, hasTag33: reply.maker?.has_33 ?? false, isoTonemap: reply.heic?.iso_tmap ?? false,
                hdrSignal: reply.heic?.hdr_signal ?? false, gainMapItem: reply.heic?.gain_map_item)
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
#endif
