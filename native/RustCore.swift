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
#endif
