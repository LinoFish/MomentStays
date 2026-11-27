import Foundation
import AVFoundation
import CryptoKit

// Shared error, hashing and native preview sample inspection. Conversion lives in Rust.
struct Failure: LocalizedError {
    let message: String
    init(_ message: String) { self.message = message }
    var errorDescription: String? { message }
}

func emit(_ value: [String: Any]) throws {
    let data = try JSONSerialization.data(withJSONObject: value, options: [.sortedKeys])
    print(String(decoding: data, as: UTF8.self))
}

func hex(_ digest: SHA256.Digest) -> String {
    let digits = Array("0123456789abcdef".utf8)
    var chars = [UInt8](repeating: 0, count: SHA256.Digest.byteCount * 2)
    for (i, byte) in digest.enumerated() {
        chars[i * 2] = digits[Int(byte >> 4)]
        chars[i * 2 + 1] = digits[Int(byte & 0x0f)]
    }
    return String(decoding: chars, as: UTF8.self)
}

func sha(_ data: Data) -> String { hex(SHA256.hash(data: data)) }

func seconds(_ t: CMTime) -> Double { CMTimeGetSeconds(t) }

struct TrackInfo {
    let track: AVAssetTrack
    let pts: [CMTime]
    let durations: [CMTime]
    let sizes: [Int]
    let hash: String
}

func isMediaSample(_ sample: CMSampleBuffer) -> Bool {
    // AVAssetReader also emits zero-sample edit/discontinuity markers with invalid PTS.
    guard CMSampleBufferGetNumSamples(sample) > 0,
          let block = CMSampleBufferGetDataBuffer(sample) else { return false }
    return CMBlockBufferGetDataLength(block) > 0
}

func trackInfo(_ asset: AVAsset, _ track: AVAssetTrack) throws -> TrackInfo {
    let reader = try AVAssetReader(asset: asset)
    let output = AVAssetReaderTrackOutput(track: track, outputSettings: nil)
    reader.add(output)
    guard reader.startReading() else { throw Failure("Cannot read source samples") }
    var pts: [CMTime] = [], durations: [CMTime] = [], sizes: [Int] = []
    var hash = SHA256()
    while let sample = output.copyNextSampleBuffer() {
        guard isMediaSample(sample) else { continue }
        // Readers may regroup AAC packets when MP4 is remuxed into MOV. Buffer
        // boundaries are not media packet boundaries: compare each sample's
        // own timestamp, duration and size, independent of reader batching.
        for index in 0..<CMSampleBufferGetNumSamples(sample) {
            var timing = CMSampleTimingInfo()
            guard CMSampleBufferGetSampleTimingInfo(sample,at:index,timingInfoOut:&timing) == noErr,
                  timing.presentationTimeStamp.isNumeric else {
                throw Failure("Compressed packet has no numeric presentation timestamp")
            }
            pts.append(timing.presentationTimeStamp)
            durations.append(timing.duration)
            sizes.append(CMSampleBufferGetSampleSize(sample,at:index))
        }
        if let block = CMSampleBufferGetDataBuffer(sample) {
            let count = CMBlockBufferGetDataLength(block)
            var bytes = [UInt8](repeating: 0, count: count)
            guard CMBlockBufferCopyDataBytes(block, atOffset: 0, dataLength: count, destination: &bytes) == noErr else {
                throw Failure("Cannot read compressed packet")
            }
            // Hash the copied bytes directly; wrapping them in Data would copy every sample again.
            bytes.withUnsafeBytes { hash.update(bufferPointer: $0) }
        }
    }
    guard reader.status == .completed else { throw Failure(reader.error?.localizedDescription ?? "Sample reading failed") }
    return TrackInfo(track: track, pts: pts, durations: durations, sizes: sizes,
                     hash: hex(hash.finalize()))
}

/// Per-track sample facts read from the MP4/MOV sample tables by the shared
/// Rust core (`track-info`, core/src/engine/decisions/track_info.rs).
struct CoreTrackInfo {
    let handler: String
    let timescale: Int32
    let mediaPTS: [Int64]
    let presentedPTS: [Int64?]
    let durations: [Int64]
    let sizes: [Int]
    let hash: String
}

func coreTrackInfo(_ url: URL) throws -> [CoreTrackInfo] {
    let v = try RustCore.engine(["mode":"track-info","input":url.path])
    guard let tracks = v["tracks"] as? [[String:Any]] else { throw Failure("Rust engine: invalid track info") }
    return try tracks.map { t in
        guard let handler = t["handler"] as? String, let timescale = (t["timescale"] as? NSNumber)?.int32Value,
              let media = t["media_pts"] as? [NSNumber], let presented = t["presented_pts"] as? [Any],
              let durations = t["durations"] as? [NSNumber], let sizes = t["sizes"] as? [NSNumber],
              let hash = t["sha256"] as? String else { throw Failure("Rust engine: invalid track info") }
        return CoreTrackInfo(handler:handler,timescale:timescale,mediaPTS:media.map(\.int64Value),
            presentedPTS:presented.map { ($0 as? NSNumber)?.int64Value },durations:durations.map(\.int64Value),
            sizes:sizes.map(\.intValue),hash:hash)
    }
}
