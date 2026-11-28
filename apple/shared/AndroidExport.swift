import Foundation
import AVFoundation

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
    static func pair(_ still: URL, domain: String = "auto") throws -> Pair {
        let v = try RustCore.engine(["mode":"android-pair","input":still.path,"time_domain":domain])
        guard let movie = v["movie"] as? String, let id = v["identifier"] as? String,
              let cover = v["cover_seconds"] as? Double, let frame = v["frame"] as? Int else { throw Failure("Rust engine: invalid pair") }
        return Pair(still:still,movie:URL(fileURLWithPath:movie),identifier:id,cover:CMTime(seconds:cover,preferredTimescale:1_000_000_000),frame:frame)
    }

    /// The one sibling still whose Apple content identifier equals the movie's,
    /// decided by the shared Rust core (`live-selection`).
    static func liveStill(_ movie: URL) throws -> URL {
        let v = try RustCore.engine(["mode":"live-selection","input":movie.path])
        guard let still = v["still"] as? String else { throw Failure("Rust engine: invalid live selection") }
        return URL(fileURLWithPath:still)
    }

    static func scanFile(_ url:URL, relative:String) -> ScanRow {
        return RustCore.scanFile(url,relative:relative,direction:"to-android")
    }

}
