import Foundation

// Build with the production Rust bridge and test-only system-codec fixture support.
@main struct MakeNativeFixture {
    static func main() throws {
        let root = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
        let media = root.appendingPathComponent("media", isDirectory: true)
        try FileManager.default.createDirectory(at: media, withIntermediateDirectories: true)
        _ = try fixture(["folder": media.path, "audio": true])
        let still = Array(try Data(contentsOf: media.appendingPathComponent("still.jpg")))
        let video = Array(try Data(contentsOf: media.appendingPathComponent("clip.mp4")))
        let bytes = try fixtureMotion(still: still, video: video, options: AndroidOptions(vendor: "google"), time: 400000, frame: 12)
        let input = root.appendingPathComponent("input", isDirectory: true)
        try FileManager.default.createDirectory(at: input, withIntermediateDirectories: true)
        try Data(bytes).write(to: input.appendingPathComponent("motion.jpg"))
    }
}
