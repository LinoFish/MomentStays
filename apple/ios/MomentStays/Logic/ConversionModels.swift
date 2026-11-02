import Foundation
import UIKit

// Conversion-facing models shared by the app and its tests. The UI owns presentation only;
// the conversion implementation (`ConversionEngine`) is injected at the app boundary.
struct ConversionClient {
    let engine: any ConversionEngine
    func convert(_ input: InputPhoto, _ brand: String) async throws -> ConversionOutput {
        guard let file = input.file else { throw ConversionFailure.unavailable }
        if input.direction == .liveToAndroid && input.pairedVideo == nil { throw ConversionFailure.unreadable }
        let directory = try PhotoSession.directory()
        var succeeded = false
        defer { if !succeeded { PhotoSession.remove(containing: directory) } }
        let vendor = input.direction == .androidToLive ? ConversionVendor.apple : (ConversionVendor(rawValue: brand.lowercased()) ?? .google)
        let request = ConversionRequest(inputs: [file] + (input.pairedVideo.map { [$0] } ?? []), outputDirectory: directory, targetVendor: vendor)
        let result: ConversionResult = try await withCheckedThrowingContinuation { continuation in
            engine.convert(request: request, progress: { _ in }) { continuation.resume(with: $0) }
        }
        guard let item = result.items.first, item.status == .converted || item.status == .resumed, let output = item.output else {
            throw ConversionError(code: .failed, message: result.items.first?.detail ?? String(localized: "Couldn't convert this photo"))
        }
        let all = (try? FileManager.default.contentsOfDirectory(at: output, includingPropertiesForKeys: nil)) ?? [output]
        let photos = all.filter { ["jpg", "jpeg", "heic", "heif"].contains($0.pathExtension.lowercased()) }
        let videos = all.filter { ["mov", "mp4"].contains($0.pathExtension.lowercased()) }
        let files = input.direction == .androidToLive ? Array(photos.prefix(1)) + Array(videos.prefix(1)) : Array(photos.prefix(1))
        guard files.count == (input.direction == .androidToLive ? 2 : 1) else { throw ConversionError(code: .failed, message: String(localized: "Couldn't find converted files.")) }
        succeeded = true
        return ConversionOutput(direction: input.direction, files: files, brand: brand, image: photos.first.flatMap { UIImage(contentsOfFile: $0.path) })
    }
}
struct UnavailableEngine: ConversionEngine {
    func inspect(input: URL, cancellation: InspectionCancellation, completion: @escaping (Result<InspectionResult, ConversionError>) -> Void) {
        completion(.failure(ConversionError(code: .unavailable, message: String(localized: "Conversion is not available in this build."))))
    }
    func convert(request: ConversionRequest, progress: @escaping (ConversionProgress) -> Void, completion: @escaping (Result<ConversionResult, ConversionError>) -> Void) {
        completion(.failure(ConversionError(code: .unavailable, message: String(localized: "Conversion is not available in this build."))))
    }
}
enum ConversionFailure: LocalizedError {
    case unavailable, unreadable
    var errorDescription: String? {
        switch self {
        case .unavailable: return String(localized: "Conversion is not available in this build.")
        case .unreadable: return String(localized: "Couldn't open this file.")
        }
    }
}
enum Direction { case androidToLive, liveToAndroid }
struct InputPhoto: Identifiable, Hashable {
    static func == (lhs: Self, rhs: Self) -> Bool { lhs.id == rhs.id }
    func hash(into hasher: inout Hasher) { hasher.combine(id) }
    let id = UUID()
    let direction: Direction
    let file: URL?
    let assetID: String?
    var pairedVideo: URL? = nil
    let name: String
    let image: UIImage?
    var originalCertain = true
}
struct ConversionOutput: Identifiable, Hashable {
    static func == (lhs: Self, rhs: Self) -> Bool { lhs.id == rhs.id }
    func hash(into hasher: inout Hasher) { hasher.combine(id) }
    let id = UUID()
    let direction: Direction
    let files: [URL]
    let brand: String
    let image: UIImage?
}
