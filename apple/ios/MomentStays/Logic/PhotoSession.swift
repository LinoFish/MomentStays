import Foundation
import CoreTransferable
import UniformTypeIdentifiers

enum PhotoSession {
    static let root = FileManager.default.temporaryDirectory.appendingPathComponent("MomentStaysSessions", isDirectory: true)
    static func directory() throws -> URL {
        let url = root.appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }
    static func remove(containing url: URL?) {
        guard let url else { return }
        let relative = url.standardizedFileURL.path
        let prefix = root.standardizedFileURL.path + "/"
        guard relative.hasPrefix(prefix), let first = relative.dropFirst(prefix.count).split(separator: "/").first else { return }
        try? FileManager.default.removeItem(at: root.appendingPathComponent(String(first), isDirectory: true))
    }
    static func removeExpired() {
        let cutoff = Date().addingTimeInterval(-24 * 60 * 60)
        guard let folders = try? FileManager.default.contentsOfDirectory(at: root, includingPropertiesForKeys: [.contentModificationDateKey]) else { return }
        for folder in folders {
            if let date = try? folder.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate, date < cutoff {
                try? FileManager.default.removeItem(at: folder)
            }
        }
    }
}
