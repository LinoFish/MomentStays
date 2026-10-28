@testable import MomentStaysBackend
import Foundation

// Separate from NativeDriver so exact byte/error parity can be checked without
// changing the app engine's normal headless entry point.
@main struct RustBridgeDriver {
    static func main() throws {
        for path in CommandLine.arguments.dropFirst() {
            do {
                let p = try PhotoFormat.parse(Array(Data(contentsOf: URL(fileURLWithPath: path))))
                try emit(["still": Data(p.still).base64EncodedString(),
                    "video": Data(p.video).base64EncodedString(), "format": p.format,
                    "timestamp": p.timestamp.map { $0 as Any } ?? NSNull(),
                    "tag": p.timestampTag.map { $0 as Any } ?? NSNull(),
                    "frame": p.coverFrame.map { $0 as Any } ?? NSNull(), "ultra": p.ultra,
                    "hash": try PhotoFormat.codingHash(p.still),
                    "auxiliary": try PhotoFormat.auxiliaryHashes(p.still)])
            } catch { try emit(["error": error.localizedDescription]) }
        }
    }
}
