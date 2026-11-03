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

// Cluster nearby choices so timeline controls remain operable at narrow widths.
// A missing timestamp is disclosed in the label, never invented as a video time.
struct CoverTimelineMarker: Identifiable {
    let index: Int?
    let time: Double?
    let issue: String?
    var id: String { index.map { "candidate-\($0)" } ?? "original" }
    var shortTitle: String { index.map { "高清\($0 + 1)" } ?? "原片" }
    var title: String {
        let name = index.map { "高清候选 \($0 + 1)" } ?? "原始照片"
        return name + (time.map { String(format:" · %.3f s",$0) } ?? " · 时间未确定")
    }
}

struct CoverTimelineGroup: Identifiable {
    static let controlWidth: CGFloat = 60
    var markers: [CoverTimelineMarker]
    var x: CGFloat
    var id: String { markers.map(\.id).joined(separator:"|") }

    static func layout(original: Double?, candidates: [PreviewCoverCandidate], duration: Double, width: CGFloat) -> [Self] {
        guard duration.isFinite, duration > 0, width.isFinite, width > 0 else { return [] }
        let inset = min(controlWidth / 2,width / 2)
        var values = [CoverTimelineMarker(index:nil,time:original,issue:nil)]
        for candidate in candidates {
            values.append(CoverTimelineMarker(index:candidate.index,time:candidate.time,issue:candidate.issue))
        }
        let usableWidth: CGFloat = max(0,width - 2 * inset)
        var positioned: [(marker:CoverTimelineMarker,x:CGFloat)] = []
        for marker in values {
            var time = marker.time ?? 0
            if !time.isFinite { time = 0 }
            let fraction: Double = min(1,max(0,time / duration))
            let x: CGFloat = inset + CGFloat(fraction) * usableWidth
            positioned.append((marker:marker,x:x))
        }
        positioned.sort { lhs,rhs in
            if lhs.x == rhs.x { return lhs.marker.id < rhs.marker.id }
            return lhs.x < rhs.x
        }
        var groups: [Self] = []
        for (marker,x) in positioned {
            if let last = groups.last, x - last.x < controlWidth + 4 {
                groups[groups.count - 1].markers.append(marker)
            } else { groups.append(Self(markers:[marker],x:x)) }
        }
        return groups
    }
}

// Source hints are suggestions bound to a main-track sample, not auxiliary covers.
struct ManualFramePoint: Decodable {
    let frame_index: Int
    let presented_us: Int
    let media_us: Int
}
struct ManualHintOrigin: Decodable {
    let raw_field: String
    let component: Int
    let raw_value: String
    let interpretation: String
    let time_domain: String
}
struct ManualSourceHint: Decodable, Identifiable {
    let frame_index: Int
    let presented_us: Int
    let verified: Bool
    let sources: [ManualHintOrigin]
    var id: Int { frame_index }
    var time: Double { Double(presented_us)/1e6 }
    var title: String { String(format:"%@ %.3f s",verified ? "源封面" : "可能",time) }
    var detail: String {
        sources.map { "尾标「\($0.raw_field)」第\($0.component)值\($0.raw_value)，按毫秒尝试\($0.time_domain == "media" ? "媒体" : "播放")时间域，单位/时间域未确定" }.joined(separator:"；")
    }
}
struct ManualHintGroup: Identifiable {
    static let controlWidth: CGFloat = 104
    let hints: [ManualSourceHint]
    let x: CGFloat
    var id: String { hints.map { String($0.id) }.joined(separator:"|") }
    static func layout(_ hints:[ManualSourceHint], end:Double, width:CGFloat) -> [Self] {
        guard end.isFinite, end > 0, width.isFinite, width > 0 else { return [] }
        let inset = min(controlWidth/2,width/2)
        let values = hints.filter { $0.time.isFinite && $0.time >= 0 && $0.time <= end }.sorted { $0.time < $1.time }
        var result = [Self]()
        for hint in values {
            let x = inset + CGFloat(hint.time/end) * max(0,width - 2*inset)
            if let last = result.last, x-last.x < controlWidth+8 {
                result[result.count-1] = Self(hints:last.hints+[hint],x:last.x)
            } else { result.append(Self(hints:[hint],x:x)) }
        }
        return result
    }
}
enum ManualFrameTimeline {
    static func nearest(_ time:Double, frames:[ManualFramePoint]) -> Int {
        guard time.isFinite else { return 0 }
        return frames.indices.min { abs(Double(frames[$0].presented_us)/1e6-time) < abs(Double(frames[$1].presented_us)/1e6-time) } ?? 0
    }
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
        if direction == "to-android" {
            let pair = try AndroidExport.pair(source, domain: timeDomain)
            movie = pair.movie
        } else {
            try check(token)
            let cache = FileManager.default.temporaryDirectory.appendingPathComponent("MomentStays-preview-" + UUID().uuidString)
            try FileManager.default.createDirectory(at: cache, withIntermediateDirectories: false)
            folder = cache; movie = cache.appendingPathComponent("source.mp4")
            if direction == "to-apple" {
                _ = try RustCore.engine(["mode":"extract-to-apple","input":source.path,"output":movie.path],token:token)
            } else {
                let photo = try PhotoFormat.parse(Array(Data(contentsOf:source)))
                try Data(photo.video).write(to:movie,options:.atomic)
            }
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
        var candidateSession: CoverCandidateSession?, choices: [PreviewCoverCandidate] = [], candidateIssue: String?
        let resolved = try RustCore.engine(["mode":"preview-cover","input":source.path,"direction":direction,"time_domain":timeDomain],token:token)
        if let value = resolved["cover"] as? [String:Any] {
            issue = value["issue"] as? String
            if let us = value["presented_us"] as? Double { cover = max(0,us / 1e6 - seconds(range.start)) }
        }
        candidateIssue = resolved["candidate_issue"] as? String
        if let rows = resolved["candidates"] as? [[String:Any]] {
            choices = rows.compactMap { v in
                guard let i = v["index"] as? Int, let us = v["timestamp_us"] as? Int else { return nil }
                let t = (v["presented_us"] as? Double).map { max(0,$0 / 1e6 - seconds(range.start)) }
                return PreviewCoverCandidate(index:i,timestampUs:us,time:t,issue:v["issue"] as? String)
            }
        }
        if !choices.isEmpty, let folder = folder {
            candidateSession = try CoverCandidateSession.prepare(video:Array(Data(contentsOf:movie)),folder:folder)
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
    static var candidateStatus = ""
}
#endif
