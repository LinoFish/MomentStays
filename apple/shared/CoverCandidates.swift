import Foundation
import ImageIO
import CoreGraphics

struct CoverCandidate {
    let index: Int
    let timestampUs: Int
    let timeSource: String
    var json: [String:Any] { ["index":index,"timestamp_us":timestampUs,"time_source":timeSource] }
}

// Owns only a temporary decode copy. Google's image-substitution metadata can
// carry the real frame time even when every sample in the sparse track has PTS 0.
final class CoverCandidateSession {
    let candidates: [CoverCandidate]
    private let source: URL
    private let folder: URL
    private init(source: URL, folder: URL, value: [String:Any]) {
        self.source = source; self.folder = folder
        candidates = (value["candidates"] as? [[String:Any]] ?? []).compactMap { v in
            guard let i = v["index"] as? Int, let us = v["timestamp_us"] as? Int, let ts = v["time_source"] as? String else { return nil }
            return CoverCandidate(index:i,timestampUs:us,timeSource:ts)
        }
    }
    static func prepare(video: [UInt8], folder: URL) throws -> CoverCandidateSession? {
        let source = folder.appendingPathComponent("candidate-original.mp4")
        try Data(video).write(to:source,options:.atomic)
        let value = try RustCore.engine(["mode":"candidates-video","input":source.path,"output":folder.path])
        guard value["present"] as? Bool == true else { return nil }
        return CoverCandidateSession(source:source,folder:folder,value:value)
    }
    func candidate(_ index: Int) throws -> CoverCandidate {
        guard let value = candidates.first(where: { $0.index == index }) else { throw Failure("所选高清候选不存在；请重新选择封面") }
        return value
    }
    // The preview may request a candidate while another thumbnail is loading.
    private let lock = NSLock()
    func writeJPEG(_ index: Int, to url: URL) throws -> [String:Any] {
        lock.lock(); defer { lock.unlock() }
        _ = try candidate(index)
        let value = try RustCore.engine(["mode":"candidates-video","input":source.path,"output":folder.path,"index":index])
        let encoded = folder.appendingPathComponent("candidate.jpg")
        if encoded.standardizedFileURL != url.standardizedFileURL { try Data(contentsOf:encoded).write(to:url,options:.atomic) }
        guard let result = value["image"] as? [String:Any] else { throw Failure("Rust engine: candidate image missing") }
        return result
    }
    func image(_ index: Int, maximumSize: CGFloat = 0) throws -> CGImage {
        let output = folder.appendingPathComponent("candidate-preview-" + UUID().uuidString + ".jpg")
        defer { try? FileManager.default.removeItem(at:output) }
        _ = try writeJPEG(index,to:output)
        guard let source = CGImageSourceCreateWithURL(output as CFURL,nil) else { throw Failure("无法读取候选画面") }
        if maximumSize > 0, let image = CGImageSourceCreateThumbnailAtIndex(source,0,
            [kCGImageSourceCreateThumbnailFromImageAlways:true,kCGImageSourceThumbnailMaxPixelSize:maximumSize] as CFDictionary) { return image }
        guard let image = CGImageSourceCreateImageAtIndex(source,0,nil) else { throw Failure("无法读取候选画面") }
        return image
    }
}
