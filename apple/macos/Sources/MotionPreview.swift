import Foundation
import AVFoundation
import CoreGraphics

struct PreviewCoverCandidate: Identifiable {
    let index: Int
    let timestampUs: Int
    let time: Double?
    let issue: String?
    var id: Int { index }
    var title: String { String(format:"高清候选 %d · %.3f s",index + 1,Double(timestampUs) / 1e6) }
}

// A session owns only its extracted preview cache. Originals and paired MOVs are read-only.
final class MotionPreviewSession {
    let duration: Double
    let coverTime: Double?
    let coverIssue: String?
    let frameTimes: [Double]
    let candidates: [PreviewCoverCandidate]
    let candidateIssue: String?
    private let candidateSession: CoverCandidateSession?
    private let generator: AVAssetImageGenerator
    private let temporaryFolder: URL?

    private init(composition: AVComposition, duration: Double, cover: Double?, issue: String?,
                 frames: [Double], temporaryFolder: URL?, candidates: [PreviewCoverCandidate],
                 candidateSession: CoverCandidateSession?, candidateIssue: String?) {
        self.duration = duration; coverTime = cover; coverIssue = issue
        frameTimes = frames; self.temporaryFolder = temporaryFolder
        self.candidates = candidates; self.candidateSession = candidateSession; self.candidateIssue = candidateIssue
        generator = AVAssetImageGenerator(asset: composition)
        generator.appliesPreferredTrackTransform = true
        generator.maximumSize = CGSize(width: 640, height: 640)
        generator.requestedTimeToleranceBefore = .zero
        generator.requestedTimeToleranceAfter = .zero
    }

    deinit {
        if let folder = temporaryFolder { try? FileManager.default.removeItem(at: folder) }
    }

    static func prepare(source: URL, direction: String, timeDomain: String = "auto",
                        token: StopToken) throws -> MotionPreviewSession {
        try check(token)
        var folder: URL?
        var ownsFolder = true
        defer { if ownsFolder, let folder = folder { try? FileManager.default.removeItem(at: folder) } }
        let movie: URL
        var request: [String: Any] = ["time_domain": timeDomain, "missing_time": "error"]
        if direction == "to-android" {
            let pair = try AndroidExport.pair(source, domain: timeDomain)
            movie = pair.movie
            // Pair validation already resolves the source metadata to an actual media frame.
            request["cover_frame_index"] = pair.frame
        } else {
            let photo = try PhotoFormat.parse(Array(Data(contentsOf: source)))
            try check(token)
            let cache = FileManager.default.temporaryDirectory.appendingPathComponent("MomentStays-preview-" + UUID().uuidString)
            try FileManager.default.createDirectory(at: cache, withIntermediateDirectories: false)
            folder = cache; movie = cache.appendingPathComponent("source.mp4")
            try Data(photo.video).write(to: movie, options: .atomic)
            if let timestamp = photo.timestamp { request["timestamp_us"] = timestamp }
            if let frame = photo.coverFrame { request["cover_frame_index"] = frame }
        }
        try check(token)
        let asset = AVURLAsset(url: movie)
        guard let track = asset.tracks(withMediaType: .video).first else { throw Failure("没有可预览的视频轨") }
        let info = try trackInfo(asset, track)
        try check(token)
        // Readers may expose a leading empty edit (for example a clip whose first PTS
        // is 2 seconds). Trim to actual visible samples so the preview begins at its first frame.
        func presented(_ media: CMTime) throws -> CMTime {
            let mappings = track.segments.filter { !$0.isEmpty && CMTimeRangeContainsTime($0.timeMapping.source, time: media) }
            if track.segments.isEmpty { return media }
            guard mappings.count == 1 else { throw Failure("视频编辑时间线无法唯一定位预览画面") }
            let mapping = mappings[0].timeMapping
            return CMTimeMapTimeFromRangeToRange(media, fromRange: mapping.source, toRange: mapping.target)
        }
        let visible = zip(info.pts, info.durations).compactMap { point, length -> (CMTime, CMTime, CMTime)? in
            guard let time = try? presented(point) else { return nil }
            return (point, time, length)
        }.sorted { CMTimeCompare($0.1, $1.1) < 0 }
        guard let first = visible.first, let last = visible.last else { throw Failure("视频没有可预览的画面") }
        var lastDuration = last.2
        if !lastDuration.isNumeric || seconds(lastDuration) <= 0 {
            guard visible.count > 1 else { throw Failure("无法确定预览画面时长") }
            lastDuration = CMTimeSubtract(last.1, visible[visible.count - 2].1)
        } else if let segment = track.segments.first(where: { !$0.isEmpty && CMTimeRangeContainsTime($0.timeMapping.source, time: last.0) }) {
            let mapping = segment.timeMapping
            lastDuration = CMTimeSubtract(CMTimeMapTimeFromRangeToRange(CMTimeAdd(last.0, lastDuration), fromRange: mapping.source, toRange: mapping.target), last.1)
        }
        let range = CMTimeRange(start: first.1, end: CMTimeMinimum(CMTimeAdd(last.1, lastDuration), CMTimeRangeGetEnd(track.timeRange)))
        let duration = seconds(range.duration)
        guard duration.isFinite, duration > 0 else { throw Failure("视频时长无效，无法预览") }
        let composition = AVMutableComposition()
        guard let previewTrack = composition.addMutableTrack(withMediaType: .video, preferredTrackID: kCMPersistentTrackID_Invalid) else {
            throw Failure("无法创建预览轨道")
        }
        try previewTrack.insertTimeRange(range, of: track, at: .zero)
        previewTrack.preferredTransform = track.preferredTransform
        func offset(_ media: CMTime) throws -> Double {
            let value = seconds(CMTimeSubtract(try presented(media), range.start))
            guard value.isFinite, value >= -0.000002, value < duration else { throw Failure("画面超出可预览时间范围") }
            return max(0, value)
        }
        let frames = info.pts.compactMap { try? offset($0) }.sorted()
        guard !frames.isEmpty else { throw Failure("视频没有可预览的画面") }
        var cover: Double?, issue: String?
        do {
            let resolved = try sourceCoverTiming(request, video: info, selection: "primary", primaryInfo: { info })
            cover = try offset(resolved.frameTime)
        } catch { issue = error.localizedDescription }
        var candidateSession: CoverCandidateSession?, choices: [PreviewCoverCandidate] = [], candidateIssue: String?
        if direction == "to-apple", let folder = folder, asset.tracks(withMediaType:.video).count > 1 {
            do {
                candidateSession = try CoverCandidateSession.prepare(video:Array(Data(contentsOf:movie)),folder:folder)
                for candidate in candidateSession?.candidates ?? [] {
                    do {
                        let resolved = try sourceCoverTiming(["timestamp_us":candidate.timestampUs,"time_domain":"presentation"],
                            video:info,selection:"primary",primaryInfo:{ info })
                        choices.append(PreviewCoverCandidate(index:candidate.index,timestampUs:candidate.timestampUs,
                            time:try offset(resolved.frameTime),issue:nil))
                    } catch {
                        choices.append(PreviewCoverCandidate(index:candidate.index,timestampUs:candidate.timestampUs,time:nil,issue:error.localizedDescription))
                    }
                }
            } catch { candidateIssue = error.localizedDescription }
        }
        try check(token)
        let session = MotionPreviewSession(composition: composition, duration: duration, cover: cover,
            issue: issue, frames: frames, temporaryFolder: folder,candidates:choices,
            candidateSession:candidateSession,candidateIssue:candidateIssue)
        ownsFolder = false
        return session
    }

    func candidateFrame(_ index: Int) throws -> CGImage {
        guard let session = candidateSession else { throw Failure("原片没有辅助高清候选") }
        return try session.image(index,maximumSize:640)
    }

    // Snap to a real source sample, including the last frame when the pointer reaches 100%.
    func frame(at fraction: Double) throws -> (image: CGImage, time: Double) {
        let time = frameTimes[Self.frameIndex(fraction: fraction, duration: duration, times: frameTimes)]
        var actual = CMTime.invalid
        let image = try generator.copyCGImage(at: CMTime(seconds: time, preferredTimescale: 1_000_000), actualTime: &actual)
        return (image, actual.isNumeric ? seconds(actual) : time)
    }

    static func frameIndex(fraction: Double, duration: Double, times: [Double]) -> Int {
        guard !times.isEmpty else { return 0 }
        let target = (fraction.isFinite ? min(1, max(0, fraction)) : 0) * duration
        var lower = 0, upper = times.count
        while lower < upper {
            let middle = (lower + upper) / 2
            if times[middle] <= target { lower = middle + 1 } else { upper = middle }
        }
        return max(0, lower - 1)
    }

    private static func check(_ token: StopToken) throws {
        if token.isCancelled { throw Failure("预览已取消") }
    }
}

#if UI_DIAGNOSTICS
// Set only by the native screenshot harness, using a generated test photo.
enum MotionPreviewDiagnostics {
    static var source: URL?
    static var duration: Double?
    static var cover: Double?
    static var frame: Double?
    static var hovering = false
    static var candidateCount = 0
    static var candidateImageLoaded = false
}
#endif
