import Foundation
import AVFoundation

// Test-only semantic checks. trackInfo hashes compressed sample bytes, not container bytes.
private func requireCheck(_ condition: Bool, _ message: String) throws {
    guard condition else { throw Failure("semantic check: " + message) }
}
private func essence(_ url: URL) throws -> [String: String] {
    let asset = AVURLAsset(url: url)
    var hashes: [String: String] = [:]
    for (name, type) in [("video", AVMediaType.video), ("audio", AVMediaType.audio)] {
        let tracks = asset.tracks(withMediaType: type)
        try requireCheck(tracks.count == 1, "expected one \(name) track")
        hashes[name] = try trackInfo(asset, tracks[0]).hash
    }
    return hashes
}
func checkAppleOutput(original: URL, output: URL, work: URL) throws {
    let photo = try PhotoFormat.parse(Array(Data(contentsOf: original)))
    try requireCheck(!photo.ultra, "expected SDR sample")
    let photoURL = output.appendingPathComponent("motion.jpg")
    let pair = try AndroidExport.pair(photoURL)
    try requireCheck(abs(seconds(pair.cover) - 0.4) < 0.00001, "Apple cover")
    let originalVideo = work.appendingPathComponent("original-video.mp4")
    try Data(photo.video).write(to: originalVideo)
    let a = try essence(originalVideo), b = try essence(pair.movie)
    try requireCheck(a == b, "Apple compressed video/audio hashes differ")
    let sourceStill = try PhotoFormat.codingHash(photo.still)
    let resultStill = try PhotoFormat.codingHash(Array(Data(contentsOf: photoURL)))
    try requireCheck(sourceStill == resultStill, "Apple JPEG coding hash differs")
    print("Apple paired content ID=\(pair.identifier), cover=\(seconds(pair.cover)), JPEG=\(sourceStill), essence=\(a)")
}
func checkAndroidOutput(pairDirectory: URL, output: URL, work: URL) throws {
    let source = try AndroidExport.pair(pairDirectory.appendingPathComponent("motion.jpg"))
    let result = try PhotoFormat.parse(Array(Data(contentsOf: output.appendingPathComponent("motion.MP.jpg"))))
    try requireCheck(!result.ultra, "expected SDR output")
    try requireCheck(result.timestamp == 400000, "Android cover timestamp")
    let originalStill = try PhotoFormat.codingHash(Array(Data(contentsOf: source.still)))
    let resultStill = try PhotoFormat.codingHash(result.still)
    try requireCheck(originalStill == resultStill, "Android JPEG coding hash differs")
    let resultVideo = work.appendingPathComponent("result-video.mp4")
    try Data(result.video).write(to: resultVideo)
    let a = try essence(source.movie), b = try essence(resultVideo)
    try requireCheck(a == b, "Android compressed video/audio hashes differ")
    print("Android source pair ID=\(source.identifier), cover=\(result.timestamp!), JPEG=\(resultStill), essence=\(a)")
}
