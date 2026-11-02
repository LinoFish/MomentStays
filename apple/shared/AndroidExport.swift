import Foundation
import AVFoundation
import ImageIO

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
        let v = try RustCore.engine(["mode":"android-pair","input":still.path,"time_domain":domain])
        guard let movie = v["movie"] as? String, let id = v["identifier"] as? String,
              let cover = v["cover_seconds"] as? Double, let frame = v["frame"] as? Int else { throw Failure("Rust engine: invalid pair") }
        return Pair(still:still,movie:URL(fileURLWithPath:movie),identifier:id,cover:CMTime(seconds:cover,preferredTimescale:1_000_000_000),frame:frame)
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

    static func scanFile(_ url:URL, relative:String) -> ScanRow {
        return RustCore.scanFile(url,relative:relative,direction:"to-android")
    }

}
