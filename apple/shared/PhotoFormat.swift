import Foundation

struct NativeMotionPhoto {
    let still: [UInt8]
    let video: [UInt8]
    let timestamp: Int?
    let timestampTag: String?
    let ultra: Bool
    var coverFrame: Int? = nil
    var format: String = "google"
}

enum PhotoFormat {
    static let notMotion = "不是受支持的动态照片"
    static func parse(_ data: [UInt8]) throws -> NativeMotionPhoto { try RustCore.parse(data) }
    static func codingHash(_ data: [UInt8]) throws -> String { try RustCore.codingHash(data) }
    static func auxiliaryHashes(_ data: [UInt8]) throws -> [String] { try RustCore.auxiliaryHashes(data) }
}
