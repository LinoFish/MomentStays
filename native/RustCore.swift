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
}
#endif
