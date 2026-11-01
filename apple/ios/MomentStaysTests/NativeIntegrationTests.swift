import Foundation
import Testing
@testable import MomentStays

struct NativeIntegrationTests {
    @Test func syntheticRoundTrip() async throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("NativeIntegration-\(UUID().uuidString)", isDirectory: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let media = root.appendingPathComponent("media", isDirectory: true)
        try FileManager.default.createDirectory(at: media, withIntermediateDirectories: true)
        _ = try fixture(["folder": media.path])
        let still = Array(try Data(contentsOf: media.appendingPathComponent("still.jpg")))
        let video = Array(try Data(contentsOf: media.appendingPathComponent("clip.mp4")))
        let bytes = try AndroidExport.assemble(still: still, video: video, options: AndroidOptions(vendor: "google"), time: 400000, frame: 12)
        let source = root.appendingPathComponent("motion.jpg")
        try Data(bytes).write(to: source)
        let engine = NativeConversionAdapter()
        let inspection: InspectionResult = try await withCheckedThrowingContinuation { c in
            engine.inspect(input: source, cancellation: InspectionCancellation()) { c.resume(with: $0) }
        }
        #expect(inspection.classification == .motion)
        let client = ConversionClient(engine: engine)
        let input = InputPhoto(direction: .androidToLive, file: source, assetID: nil, name: "motion.jpg", image: nil)
        let apple = try await client.convert(input, "Google")
        defer { PhotoSession.remove(containing: apple.files.first) }
        #expect(apple.files.count == 2)
        let photo = try #require(apple.files.first { ["jpg", "jpeg", "heic", "heif"].contains($0.pathExtension.lowercased()) })
        let movie = try #require(apple.files.first { ["mov", "mp4"].contains($0.pathExtension.lowercased()) })
        let pair = InputPhoto(direction: .liveToAndroid, file: photo, assetID: nil, pairedVideo: movie, name: "LivePhoto", image: nil)
        let android = try await client.convert(pair, "Google")
        defer { PhotoSession.remove(containing: android.files.first) }
        let result = try #require(android.files.first)
        let reinspection: InspectionResult = try await withCheckedThrowingContinuation { c in
            engine.inspect(input: result, cancellation: InspectionCancellation()) { c.resume(with: $0) }
        }
        #expect(reinspection.classification == .motion)
    }
}
