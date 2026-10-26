import Foundation
import AVFoundation
import ImageIO
import CoreGraphics

struct CoverCandidate {
    let index: Int
    let timestampUs: Int
    let timeSource: String
    let sampleTime: CMTime
    var json: [String:Any] { ["index":index,"timestamp_us":timestampUs,"time_source":timeSource] }
}

// Owns only a temporary decode copy. Google's image-substitution metadata can
// carry the real frame time even when every sample in the sparse track has PTS 0.
final class CoverCandidateSession {
    let candidates: [CoverCandidate]
    let durationRepair: [String:Any]?
    private let track: AVAssetTrack
    private let generator: AVAssetImageGenerator

    static func prepare(video: [UInt8], folder: URL) throws -> CoverCandidateSession? {
        let url = folder.appendingPathComponent("candidate-source.mp4")
        // Zero-length sparse samples need a positive length for system decoders.
        // This changes the private decode copy only, never the exported video.
        let repaired = try PhotoFormat.repairAuxiliaryDuration(video)
        try Data(repaired?.0 ?? video).write(to:url,options:.atomic)
        let asset = AVURLAsset(url:url), videos = asset.tracks(withMediaType:.video)
        guard videos.count <= 2 else { throw Failure("高清候选需要唯一的辅助视频轨") }
        guard videos.count == 2 else { return nil }
        return try CoverCandidateSession(asset:asset,track:videos[1],metadata:MotionVideoMetadata.imageTimes(video),repair:repaired?.1)
    }

    private init(asset: AVAsset, track: AVAssetTrack, metadata: MotionVideoMetadata.ImageTimes?, repair: [String:Any]?) throws {
        self.track = track; durationRepair = repair
        let info = try trackInfo(asset,track)
        let points = info.pts.sorted { CMTimeCompare($0,$1) < 0 }
        guard !points.isEmpty, points.allSatisfy({ $0.isNumeric }),
              zip(points,points.dropFirst()).allSatisfy({ CMTimeCompare($0,$1) < 0 }) else { throw Failure("辅助轨没有可唯一定位的高清候选帧") }
        let recorded = metadata?.highResolution ?? []
        guard recorded.isEmpty || recorded.count == points.count else { throw Failure("高清候选时间描述符与辅助轨帧数不一致") }
        var choices: [CoverCandidate] = []
        for (index,point) in points.enumerated() {
            let presented = try Self.presented(point,track:track)
            let declared = recorded.isEmpty ? nil : recorded[index]
            let ticks = (seconds(presented) * 1e6).rounded()
            guard ticks.isFinite, ticks >= 0, ticks < Double(Int.max) else { throw Failure("辅助轨时间超出支持范围") }
            let us = declared ?? Int(ticks)
            guard us >= 0 else { throw Failure("高清候选时间无效") }
            choices.append(CoverCandidate(index:index,timestampUs:us,
                timeSource:declared == nil ? "auxiliary-track-presentation" : "google-high-res-metadata",sampleTime:point))
        }
        candidates = choices
        let composition = AVMutableComposition()
        guard let output = composition.addMutableTrack(withMediaType:.video,preferredTrackID:kCMPersistentTrackID_Invalid) else { throw Failure("无法读取高清候选画面") }
        try output.insertTimeRange(track.timeRange,of:track,at:.zero)
        output.preferredTransform = track.preferredTransform
        generator = AVAssetImageGenerator(asset:composition)
        generator.appliesPreferredTrackTransform = true
        generator.requestedTimeToleranceBefore = .zero
        generator.requestedTimeToleranceAfter = .zero
    }

    private static func presented(_ media: CMTime, track: AVAssetTrack) throws -> CMTime {
        if track.segments.isEmpty { return media }
        let segments = track.segments.filter { !$0.isEmpty && CMTimeRangeContainsTime($0.timeMapping.source,time:media) }
        guard segments.count == 1 else { throw Failure("辅助轨编辑时间线无法唯一定位候选画面") }
        let mapping = segments[0].timeMapping
        return CMTimeMapTimeFromRangeToRange(media,fromRange:mapping.source,toRange:mapping.target)
    }

    func candidate(_ index: Int) throws -> CoverCandidate {
        guard candidates.indices.contains(index) else { throw Failure("所选高清候选不存在；请重新选择封面") }
        return candidates[index]
    }

    func image(_ index: Int, maximumSize: CGFloat = 0) throws -> CGImage {
        let choice = try candidate(index)
        let time = CMTimeSubtract(try Self.presented(choice.sampleTime,track:track),track.timeRange.start)
        generator.maximumSize = maximumSize > 0 ? CGSize(width:maximumSize,height:maximumSize) : .zero
        return try generator.copyCGImage(at:time,actualTime:nil)
    }

    func writeJPEG(_ index: Int, to url: URL) throws -> [String:Any] {
        let choice = try candidate(index), image = try self.image(index)
        guard let destination = CGImageDestinationCreateWithURL(url as CFURL,"public.jpeg" as CFString,1,nil) else { throw Failure("无法创建候选照片") }
        // The pixels already include the video track transform. Do not copy an
        // unrelated still's orientation, gain map, or Apple pairing identifier.
        CGImageDestinationAddImage(destination,image,[kCGImageDestinationLossyCompressionQuality:1.0] as CFDictionary)
        guard CGImageDestinationFinalize(destination) else { throw Failure("候选照片编码失败") }
        let encoded = Array(try Data(contentsOf:url))
        let xmp = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"\(MLXMP.rdf)\"><rdf:Description xmlns:MomentStays=\"urn:momentstays:cover:1.0\" MomentStays:CandidatePresentationTimestampUs=\"\(choice.timestampUs)\" MomentStays:CandidateIndex=\"\(choice.index)\"/></rdf:RDF></x:xmpmeta>"
        let tagged = Array(encoded.prefix(2)) + (try MLJPEG.app(225,MLXMP.header + Binary.string(xmp))) + Array(encoded.dropFirst(2))
        try Data(tagged).write(to:url,options:.atomic)
        var result = choice.json
        result["width"] = image.width; result["height"] = image.height
        result["encoding"] = "decoded-video-to-jpeg"
        if let repair = durationRepair { result["decode_duration_repair"] = repair }
        return result
    }
}
