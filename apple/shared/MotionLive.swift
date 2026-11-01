import Foundation
import AVFoundation
import ImageIO
import CoreGraphics
import CryptoKit

// macOS 12 / Swift 5.7. No third-party code or private frameworks.
struct Failure: LocalizedError {
    let message: String
    init(_ message: String) { self.message = message }
    var errorDescription: String? { message }
}

func emit(_ value: [String: Any]) throws {
    let data = try JSONSerialization.data(withJSONObject: value, options: [.sortedKeys])
    print(String(decoding: data, as: UTF8.self))
}
// Table lookup instead of one String(format:) per byte; same lowercase hex output.
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
// Feed one slice of a larger buffer into a running hash without copying it.
func hashRange(_ hash: inout SHA256, _ d: [UInt8], _ range: Range<Int>) {
    d.withUnsafeBufferPointer { buf in
        guard let base = buf.baseAddress else { return }
        hash.update(bufferPointer: UnsafeRawBufferPointer(start: UnsafeRawPointer(base + range.lowerBound), count: range.count))
    }
}
func seconds(_ t: CMTime) -> Double { CMTimeGetSeconds(t) }
func textItem(_ key: String, _ value: String) -> AVMetadataItem {
    let item = AVMutableMetadataItem()
    item.keySpace = .quickTimeMetadata
    item.key = key as NSString
    item.value = value as NSString
    item.dataType = "com.apple.metadata.datatype.UTF-8"
    return item
}
func timedItem() -> AVMetadataItem {
    let item = AVMutableMetadataItem()
    item.keySpace = .quickTimeMetadata
    item.key = "com.apple.quicktime.still-image-time" as NSString
    item.value = NSNumber(value: Int8(0))
    item.dataType = "com.apple.metadata.datatype.int8"
    return item
}

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

func timedSamples(_ asset: AVAsset) throws -> [(CMTime, CMTime)] {
    var times: [(CMTime, CMTime)] = []
    for track in asset.tracks(withMediaType: .metadata) {
        let reader = try AVAssetReader(asset: asset)
        let output = AVAssetReaderTrackOutput(track: track, outputSettings: nil)
        reader.add(output)
        let adaptor = AVAssetReaderOutputMetadataAdaptor(assetReaderTrackOutput: output)
        guard reader.startReading() else { throw Failure("Cannot read metadata track") }
        while let group = adaptor.nextTimedMetadataGroup() {
            let markers = group.items.filter { ($0.key as? String) == "com.apple.quicktime.still-image-time" }
            if !markers.isEmpty {
                guard markers.count == 1, let value = markers[0].numberValue?.intValue, [-1,0,255].contains(value) else { throw Failure("Invalid still-image-time marker") }
                times.append((group.timeRange.start, group.timeRange.duration))
            }
        }
        guard reader.status == .completed else { throw Failure("Metadata verification failed") }
    }
    return times
}

func waitReady(_ input: AVAssetWriterInput, _ writer: AVAssetWriter) throws {
    let deadline = Date().addingTimeInterval(90)
    while !input.isReadyForMoreMediaData {
        if writer.status == .failed || writer.status == .cancelled || Date() > deadline {
            throw Failure(writer.error?.localizedDescription ?? "Writer readiness timed out")
        }
        Thread.sleep(forTimeInterval: 0.001)
    }
}

struct SourceCoverTiming {
    let requested: CMTime
    let mapped: CMTime
    let domain: String
    let frameTime: CMTime
    let frameDuration: CMTime
    let error: Double
    let policy: String
}

// Shared by conversion and read-only preview; never infer a missing cover location.
func sourceCoverTiming(_ req: [String: Any], video: TrackInfo, selection: String,
                       primaryInfo: () throws -> TrackInfo) throws -> SourceCoverTiming {
    guard !video.pts.isEmpty else { throw Failure("Video has no samples") }
    let sorted = zip(video.pts, video.durations).sorted { CMTimeCompare($0.0, $1.0) < 0 }
    let requested: CMTime
    let policy: String
    if let index = req["cover_frame_index"] as? Int {
        let primaryInfo = try primaryInfo()
        let primaryTimes = primaryInfo.pts.sorted { CMTimeCompare($0,$1) < 0 }
        guard primaryTimes.indices.contains(index) else { throw Failure("华为/荣耀原始封面帧号超出视频范围，未推测封面位置") }
        requested = primaryTimes[index]; policy = "source-huawei-frame"
    } else if let us = req["timestamp_us"] as? NSNumber, us.int64Value >= 0 {
        requested = CMTime(value: us.int64Value, timescale: 1_000_000)
        let source = req["timestamp_source"] as? String
        policy = source == "high-res-candidate" ? "selected-high-res-candidate" : source == "video-metadata" ? "source-video-metadata" : "source-xmp"
    } else if req["missing_time"] as? String == "midpoint" {
        requested = CMTimeAdd(sorted.first!.0, CMTimeMultiplyByFloat64(CMTimeSubtract(sorted.last!.0, sorted.first!.0), multiplier: 0.5))
        policy = "explicit-midpoint"
    } else {
        throw Failure("No valid source cover timestamp; use --missing-time midpoint only if intended")
    }
    // Monterey readers can expose media PTS despite a nonzero movie edit-list offset.
    // Test direct media and explicit edit-list mappings, and reject ambiguous frames.
    func durationAt(_ i: Int) throws -> CMTime {
        if i + 1 < sorted.count { return CMTimeSubtract(sorted[i + 1].0, sorted[i].0) }
        if sorted[i].1.isNumeric && seconds(sorted[i].1) > 0 { return sorted[i].1 }
        if i > 0 { return CMTimeSubtract(sorted[i].0, sorted[i - 1].0) }
        throw Failure("Cannot determine cover frame duration")
    }
    let domain = req["time_domain"] as? String ?? "auto"
    var candidates: [(CMTime, String)] = []
    if domain != "presentation" || policy == "explicit-midpoint" || policy == "source-huawei-frame" { candidates.append((requested, "media")) }
    if domain != "media" && policy != "explicit-midpoint" && policy != "source-huawei-frame" {
        for segment in video.track.segments where !segment.isEmpty {
            let mapping = segment.timeMapping
            if CMTimeRangeContainsTime(mapping.target, time: requested) {
                candidates.append((CMTimeMapTimeFromRangeToRange(requested, fromRange: mapping.target, toRange: mapping.source), "presentation-edit-list"))
            }
        }
        if video.track.segments.isEmpty { candidates.append((requested, "presentation-identity")) }
    }
    var matches: [(Int, CMTime, String, Double)] = []
    for (candidate, label) in candidates {
        guard let index = sorted.indices.min(by: {
            abs(seconds(CMTimeSubtract(sorted[$0].0, candidate))) < abs(seconds(CMTimeSubtract(sorted[$1].0, candidate)))
        }) else { continue }
        let delta = abs(seconds(CMTimeSubtract(sorted[index].0, candidate)))
        if delta <= seconds(try durationAt(index)) / 2 + 0.000002,
           !matches.contains(where: { $0.0 == index }) { matches.append((index, candidate, label, delta)) }
    }
    guard matches.count == 1, let match = matches.first else {
        if matches.count > 1 { throw Failure("Ambiguous cover time: media and presentation timelines point to different frames; select --time-domain media or presentation") }
        if selection == "secondary" { throw Failure("辅助轨无法对应原始封面时间；未修改封面或替换为主轨。可单独导出辅助轨") }
        throw Failure("Cover PTS outside the video media/edit-list timelines; no automatic midpoint fallback")
    }
    let frameTime = sorted[match.0].0
    let frameDuration = try durationAt(match.0)
    return SourceCoverTiming(requested: requested, mapped: match.1, domain: match.2,
        frameTime: frameTime, frameDuration: frameDuration, error: match.3, policy: policy)
}

func movie(_ req: [String: Any]) throws -> [String: Any] {
    guard let inputPath = req["input"] as? String, let outputPath = req["output"] as? String,
          let uuid = req["identifier"] as? String else { throw Failure("Missing movie arguments") }
    if let stillPath = req["still"] as? String {
        guard let source = CGImageSourceCreateWithURL(URL(fileURLWithPath: stillPath) as CFURL, nil),
              let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [String: Any],
              let maker = properties[kCGImagePropertyMakerAppleDictionary as String] as? [String: Any],
              maker["17"] as? String == uuid else {
            throw Failure("ImageIO could not read back the still ContentIdentifier")
        }
    }
    let asset = AVURLAsset(url: URL(fileURLWithPath: inputPath))
    // Google Motion Photo permits a secondary image-substitution video track.
    // The primary motion track precedes that optional track in container order:
    // https://developer.android.com/media/platform/motion-photo-format
    // Keep the original high-resolution JPEG as the still, and remux only the
    // primary motion video plus audio into the single-video Live Photo movie.
    let videoTracks = asset.tracks(withMediaType: .video)
    guard let primary = videoTracks.first, videoTracks.count <= 2 else {
        throw Failure("Motion Photo requires a primary video track and at most one secondary track (found \(videoTracks.count))")
    }
    let selection = req["video_track"] as? String ?? "primary"
    guard ["primary","secondary"].contains(selection) else { throw Failure("未知的视频轨选择") }
    if selection == "secondary" && videoTracks.count < 2 { throw Failure("原片没有辅助视频轨，请选择主轨") }
    let chosen = selection == "secondary" ? videoTracks[1] : primary
    let tracks = [chosen] + asset.tracks(withMediaType: .audio)
    let infos = try tracks.map { try trackInfo(asset, $0) }
    guard let video = infos.first, !video.pts.isEmpty else { throw Failure("Video has no samples") }
    if selection == "secondary" && video.pts.count < 2 {
        throw Failure("辅助轨只有 \(video.pts.count) 帧，不能作为动态视频；请使用主轨或单独导出辅助轨")
    }
    let cover = try sourceCoverTiming(req, video: video, selection: selection,
        primaryInfo: { selection == "primary" ? video : try trackInfo(asset, primary) })
    let frameTime = cover.frameTime, frameDuration = cover.frameDuration
    // Session starts at zero, without shifting source PTS. Refuse negative visible PTS.
    guard infos.allSatisfy({ $0.pts.allSatisfy { seconds($0) >= 0 } }) else {
        throw Failure("Negative media sample PTS requires explicit edit-list handling; original retained")
    }
    let destination = URL(fileURLWithPath: outputPath)
    let writer = try AVAssetWriter(outputURL: destination, fileType: .mov)
    writer.movieTimeScale = 1_000_000
    writer.metadata = [textItem("com.apple.quicktime.content.identifier", uuid)]
    var pairs: [(AVAssetReader, AVAssetReaderTrackOutput, AVAssetWriterInput)] = []
    for track in tracks {
        let reader = try AVAssetReader(asset: asset)
        let output = AVAssetReaderTrackOutput(track: track, outputSettings: nil)
        reader.add(output)
        let formatHint = track.formatDescriptions.first as! CMFormatDescription?
        let input = AVAssetWriterInput(mediaType: track.mediaType, outputSettings: nil, sourceFormatHint: formatHint)
        if track.mediaType == .video { input.mediaTimeScale = track.naturalTimeScale }
        input.expectsMediaDataInRealTime = false
        if track.mediaType == .video { input.transform = track.preferredTransform }
        guard writer.canAdd(input) else { throw Failure("Passthrough codec unsupported") }
        writer.add(input)
        pairs.append((reader, output, input))
    }
    let spec: NSDictionary = [
        kCMMetadataFormatDescriptionMetadataSpecificationKey_Identifier as NSString: "mdta/com.apple.quicktime.still-image-time",
        kCMMetadataFormatDescriptionMetadataSpecificationKey_DataType as NSString: "com.apple.metadata.datatype.int8"
    ]
    var description: CMFormatDescription?
    let status = CMMetadataFormatDescriptionCreateWithMetadataSpecifications(allocator: kCFAllocatorDefault,
        metadataType: kCMMetadataFormatType_Boxed, metadataSpecifications: [spec] as CFArray,
        formatDescriptionOut: &description)
    guard status == noErr else { throw Failure("Cannot construct timed metadata format") }
    let metadataInput = AVAssetWriterInput(mediaType: .metadata, outputSettings: nil, sourceFormatHint: description)
    metadataInput.mediaTimeScale = 1_000_000
    let adaptor = AVAssetWriterInputMetadataAdaptor(assetWriterInput: metadataInput)
    guard writer.canAdd(metadataInput) else { throw Failure("Cannot add timed metadata track") }
    writer.add(metadataInput)
    guard writer.startWriting() else { throw Failure(writer.error?.localizedDescription ?? "Writer start failed") }
    writer.startSession(atSourceTime: .zero)
    do {
        try waitReady(metadataInput, writer)
        guard adaptor.append(AVTimedMetadataGroup(items: [timedItem()], timeRange: CMTimeRange(start: frameTime, duration: frameDuration))) else {
            throw Failure("Failed to append cover timestamp")
        }
        metadataInput.markAsFinished()
        // Drain tracks concurrently: one full track at a time can deadlock muxing.
        let group = DispatchGroup()
        let lock = NSLock()
        var errors: [String] = []
        for (reader, output, input) in pairs {
            group.enter()
            DispatchQueue.global().async {
                defer { group.leave() }
                do {
                    guard reader.startReading() else { throw Failure("Track reader start failed") }
                    while let sample = output.copyNextSampleBuffer() {
                        guard isMediaSample(sample) else { continue }
                        try waitReady(input, writer)
                        guard input.append(sample) else { throw Failure(writer.error?.localizedDescription ?? "Packet append failed") }
                    }
                    guard reader.status == .completed else { throw Failure(reader.error?.localizedDescription ?? "Track read failed") }
                    input.markAsFinished()
                } catch {
                    reader.cancelReading()
                    writer.cancelWriting()
                    lock.lock(); errors.append(error.localizedDescription); lock.unlock()
                }
            }
        }
        guard group.wait(timeout: .now() + 180) == .success else { throw Failure("Movie write timed out") }
        guard errors.isEmpty else { throw Failure(errors.joined(separator: "; ")) }
        let done = DispatchSemaphore(value: 0)
        writer.finishWriting { done.signal() }
        guard done.wait(timeout: .now() + 90) == .success, writer.status == .completed else {
            throw Failure(writer.error?.localizedDescription ?? "Movie finalization failed")
        }
    } catch { writer.cancelWriting(); throw error }

    let output = AVURLAsset(url: destination)
    let outTracks = output.tracks(withMediaType: .video) + output.tracks(withMediaType: .audio)
    guard outTracks.count == tracks.count else { throw Failure("Output lost an audio/video track") }
    var trackReports: [[String: Any]] = []
    for (source, track) in zip(infos, outTracks) {
        let result = try trackInfo(output, track)
        guard result.hash == source.hash, result.sizes == source.sizes, result.pts.count == source.pts.count else {
            throw Failure("Compressed audio/video packet verification failed")
        }
        let maxShift = zip(source.pts, result.pts).map { abs(seconds(CMTimeSubtract($0, $1))) }.max() ?? 0
        // MOV may quantize to track timescale; permit at most one tick plus 1 μs.
        let tolerance = 1.0 / Double(max(track.naturalTimeScale, 1)) + 0.000001
        guard maxShift <= tolerance else { throw Failure("Movie presentation timeline shifted (\(track.mediaType.rawValue): \(maxShift) s, tolerance \(tolerance) s)") }
        trackReports.append(["type": track.mediaType.rawValue, "packets": result.pts.count,
                             "sha256": result.hash, "max_pts_shift_seconds": maxShift])
    }
    let covers = try timedSamples(output)
    guard covers.count == 1, abs(seconds(CMTimeSubtract(covers[0].0, frameTime))) <= 0.000002 else {
        throw Failure("Written cover timestamp did not survive MOV readback")
    }
    guard abs(seconds(CMTimeSubtract(covers[0].1, frameDuration))) <= 0.000002 else {
        throw Failure("Written cover duration differs from actual frame duration")
    }
    let identifier = output.metadata(forFormat: .quickTimeMetadata).first {
        ($0.key as? String) == "com.apple.quicktime.content.identifier"
    }?.stringValue
    guard identifier == uuid else { throw Failure("Output content identifier verification failed") }
    return ["requested_seconds": seconds(cover.requested), "mapped_source_seconds": seconds(cover.mapped),
            "timestamp_domain": cover.domain, "cover_seconds": seconds(covers[0].0),
            "source_frame_duration_seconds": seconds(frameDuration),
            "cover_duration_seconds": seconds(covers[0].1), "snap_error_seconds": cover.error,
            "timing_policy": cover.policy, "tracks": trackReports, "identifier": uuid,
            "source_video_track_count": videoTracks.count, "primary_source_track_id": primary.trackID,
            "secondary_source_track_ids": videoTracks.dropFirst().map { $0.trackID },
            "video_selection_policy": selection == "primary" ? "google-primary-first" : "explicit-secondary",
            "selected_source_track_id": chosen.trackID, "selected_video_role": selection,
            "selected_frame_count": video.pts.count,
            "selected_width": chosen.naturalSize.width, "selected_height": chosen.naturalSize.height]
}

// Export the auxiliary compressed stream as an independent, silent MOV. One
// frame is valid here: no fabricated motion, cover marker, looping or retiming.
func exportAuxiliary(_ req: [String:Any]) throws -> [String:Any] {
    guard let path = req["input"] as? String, let outputPath = req["output"] as? String else { throw Failure("缺少辅助轨导出路径") }
    let asset = AVURLAsset(url:URL(fileURLWithPath:path)), destination = URL(fileURLWithPath:outputPath)
    let videos = asset.tracks(withMediaType:.video)
    guard videos.count == 2 else { throw Failure("原片没有唯一的辅助视频轨，无法单独导出") }
    let track = videos[1], source = try trackInfo(asset,track)
    guard !source.pts.isEmpty, source.pts.allSatisfy({ $0.isNumeric && seconds($0) >= 0 }) else { throw Failure("系统无法读取辅助轨有效样本；若为零时长样本，可在高级设置中允许修复后单独导出") }
    let writer = try AVAssetWriter(outputURL:destination,fileType:.mov)
    writer.movieTimeScale = 1_000_000
    let hint = track.formatDescriptions.first as! CMFormatDescription?
    let input = AVAssetWriterInput(mediaType:.video,outputSettings:nil,sourceFormatHint:hint)
    input.mediaTimeScale = track.naturalTimeScale; input.transform = track.preferredTransform
    guard writer.canAdd(input) else { throw Failure("系统不支持直接封装此辅助视频编码") }
    writer.add(input)
    let reader = try AVAssetReader(asset:asset), output = AVAssetReaderTrackOutput(track:track,outputSettings:nil)
    reader.add(output)
    guard writer.startWriting() else { throw Failure(writer.error?.localizedDescription ?? "辅助视频写入失败") }
    writer.startSession(atSourceTime:.zero)
    do {
        guard reader.startReading() else { throw Failure("辅助视频读取失败") }
        while let sample = output.copyNextSampleBuffer() {
            guard isMediaSample(sample) else { continue }
            try waitReady(input,writer)
            guard input.append(sample) else { throw Failure(writer.error?.localizedDescription ?? "辅助视频样本写入失败") }
        }
        guard reader.status == .completed else { throw Failure(reader.error?.localizedDescription ?? "辅助视频读取未完成") }
        input.markAsFinished()
        let done = DispatchSemaphore(value:0); writer.finishWriting { done.signal() }
        guard done.wait(timeout:.now() + 90) == .success, writer.status == .completed else { throw Failure(writer.error?.localizedDescription ?? "辅助视频封装未完成") }
    } catch { reader.cancelReading(); writer.cancelWriting(); throw error }
    let result = AVURLAsset(url:destination), tracks = result.tracks
    guard tracks.count == 1, tracks[0].mediaType == .video else { throw Failure("辅助视频轨数校验失败") }
    let checked = try trackInfo(result,tracks[0])
    guard checked.hash == source.hash, checked.sizes == source.sizes, checked.pts.count == source.pts.count else { throw Failure("辅助视频压缩数据校验失败") }
    let shift = zip(source.pts,checked.pts).map { abs(seconds(CMTimeSubtract($0,$1))) }.max() ?? 0
    guard shift <= 1 / Double(max(tracks[0].naturalTimeScale,1)) + 0.000001 else { throw Failure("辅助视频时间轴发生位移") }
    return ["source_track_id":track.trackID,"frames":source.pts.count,"width":track.naturalSize.width,
        "height":track.naturalSize.height,"duration_seconds":seconds(track.timeRange.duration),
        "compressed_sha256":checked.hash,"max_pts_shift_seconds":shift,"audio_included":false]
}

func legacyGain(_ req: [String: Any]) throws -> [String: Any] {
    guard let path = req["input"] as? String, let gainPath = req["gain_output"] as? String,
          let stops = req["stops"] as? Double, stops.isFinite, stops > 0, stops <= 16,
          let source = CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil),
          let image = CGImageSourceCreateImageAtIndex(source, 0, nil),
          image.bitsPerComponent == 8, image.bitsPerPixel == 8, image.colorSpace?.model == .monochrome,
          let raw = image.dataProvider?.data as Data? else {
        return ["status": "unchanged", "reason": "Legacy map cannot be decoded as an 8-bit monochrome JPEG"]
    }
    let width = image.width, height = image.height, stride = image.bytesPerRow
    guard stride >= width, raw.count >= height * stride else { throw Failure("Truncated gain-map pixels") }
    let headroom = pow(2, stops)
    // Apple Rec.709-encoded linear boost -> Ultra HDR logarithmic gain.
    let lookup: [UInt8] = (0...255).map { i in
        let v = Double(i) / 255
        let linear = v < 0.081 ? v / 4.5 : pow((v + 0.099) / 1.099, 1 / 0.45)
        return UInt8(max(0, min(255, (log2(1 + (headroom - 1) * linear) / stops * 255).rounded())))
    }
    var pixels = [UInt8](repeating: 0, count: width * height)
    // Identical lookup math; raw pointers only remove per-pixel bounds
    // checks. Bounds were validated above (stride >= width, count >= height * stride).
    raw.withUnsafeBytes { rawPtr in
        let bytes = rawPtr.bindMemory(to: UInt8.self)
        pixels.withUnsafeMutableBufferPointer { out in
            guard let src = bytes.baseAddress, let dst = out.baseAddress else { return }
            for y in 0..<height {
                let row = src + y * stride, target = dst + y * width
                for x in 0..<width { target[x] = lookup[Int(row[x])] }
            }
        }
    }
    guard let provider = CGDataProvider(data: Data(pixels) as CFData),
          let converted = CGImage(width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 8,
              bytesPerRow: width, space: CGColorSpaceCreateDeviceGray(), bitmapInfo: CGBitmapInfo(rawValue: 0),
              provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent),
          let destination = CGImageDestinationCreateWithURL(URL(fileURLWithPath: gainPath) as CFURL, "public.jpeg" as CFString, 1, nil) else {
        throw Failure("Cannot encode converted gain map")
    }
    CGImageDestinationAddImage(destination, converted, [kCGImageDestinationLossyCompressionQuality: 1.0] as CFDictionary)
    guard CGImageDestinationFinalize(destination) else { throw Failure("Gain map encode failed") }
    return ["status": "converted", "stops": stops, "width": width, "height": height,
            "srgb_icc_base64": (CGColorSpace(name: CGColorSpace.sRGB)!.copyICCData()! as Data).base64EncodedString(),
            "reason": "Existing Apple gain map converted; SDR base is never re-encoded"]
}

// Deterministic local fixtures for end-to-end checks on the target OS.
func fixture(_ req: [String: Any]) throws -> [String: Any] {
    guard let folder = req["folder"] as? String else { throw Failure("Missing fixture folder") }
    let base = URL(fileURLWithPath: folder)
    try FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
    let start = (req["start_us"] as? NSNumber)?.int64Value ?? 0
    let writer = try AVAssetWriter(outputURL: base.appendingPathComponent("clip.mp4"), fileType: .mp4)
    let input = AVAssetWriterInput(mediaType: .video, outputSettings: [AVVideoCodecKey: AVVideoCodecType.h264,
        AVVideoWidthKey: 64, AVVideoHeightKey: 48, AVVideoCompressionPropertiesKey: [AVVideoAllowFrameReorderingKey: false]])
    input.mediaTimeScale = 30000
    let adaptor = AVAssetWriterInputPixelBufferAdaptor(assetWriterInput: input, sourcePixelBufferAttributes: [
        kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32ARGB,
        kCVPixelBufferWidthKey as String: 64, kCVPixelBufferHeightKey as String: 48,
        kCVPixelBufferCGImageCompatibilityKey as String: true, kCVPixelBufferCGBitmapContextCompatibilityKey as String: true])
    writer.add(input)
    guard writer.startWriting() else { throw Failure("Fixture encoder cannot start: " + String(describing: writer.error)) }
    writer.startSession(atSourceTime: .zero)
    let variable = req["variable"] as? Bool ?? false
    var time = CMTime(value: start, timescale: 1_000_000)
    var frameTimes: [Int64] = []
    for i in 0..<30 {
        try waitReady(input, writer)
        var buffer: CVPixelBuffer?
        guard CVPixelBufferPoolCreatePixelBuffer(nil, adaptor.pixelBufferPool!, &buffer) == kCVReturnSuccess, let b = buffer else {
            throw Failure("Fixture pixel allocation failed")
        }
        CVPixelBufferLockBaseAddress(b, [])
        let ptr = CVPixelBufferGetBaseAddress(b)!.assumingMemoryBound(to: UInt8.self)
        let stride = CVPixelBufferGetBytesPerRow(b)
        for y in 0..<48 { for x in 0..<64 {
            let p = y * stride + x * 4
            ptr[p] = 255; ptr[p + 1] = UInt8(i * 7); ptr[p + 2] = UInt8(x * 3); ptr[p + 3] = UInt8(y * 4)
        } }
        if i == 12 {
            let ctx = CGContext(data: CVPixelBufferGetBaseAddress(b), width: 64, height: 48, bitsPerComponent: 8,
                bytesPerRow: stride, space: CGColorSpaceCreateDeviceRGB(),
                bitmapInfo: CGImageAlphaInfo.noneSkipFirst.rawValue)!
            let dest = CGImageDestinationCreateWithURL(base.appendingPathComponent("still.jpg") as CFURL, "public.jpeg" as CFString, 1, nil)!
            let legacy = req["legacy_hdr"] as? Bool ?? false
            let properties: [String: Any] = legacy ? [kCGImagePropertyMakerAppleDictionary as String:
                ["33": NSNumber(value: Float(0.5)), "48": NSNumber(value: Float(0.1))]] : [:]
            CGImageDestinationAddImage(dest, ctx.makeImage()!, properties as CFDictionary)
            if legacy {
                let metadata = CGImageMetadataCreateMutable()
                CGImageMetadataRegisterNamespaceForPrefix(metadata, "http://ns.apple.com/HDRGainMap/1.0/" as CFString,
                    "HDRGainMap" as CFString, nil)
                CGImageMetadataSetValueWithPath(metadata, nil, "HDRGainMap:HDRGainMapVersion" as CFString, NSNumber(value: 65536))
                let gainData = Data((0..<192).map { UInt8($0) })
                let aux: [String: Any] = [kCGImageAuxiliaryDataInfoData as String: gainData,
                    kCGImageAuxiliaryDataInfoDataDescription as String: [kCGImagePropertyWidth as String: 16,
                        kCGImagePropertyHeight as String: 12, kCGImagePropertyBytesPerRow as String: 16,
                        kCGImagePropertyPixelFormat as String: kCVPixelFormatType_OneComponent8],
                    kCGImageAuxiliaryDataInfoMetadata as String: metadata]
                CGImageDestinationAddAuxiliaryDataInfo(dest, kCGImageAuxiliaryDataTypeHDRGainMap, aux as CFDictionary)
            }
            guard CGImageDestinationFinalize(dest) else { throw Failure("Fixture JPEG encode failed") }
        }
        CVPixelBufferUnlockBaseAddress(b, [])
        guard adaptor.append(b, withPresentationTime: time) else { throw Failure("Fixture append failed") }
        frameTimes.append(CMTimeConvertScale(time, timescale: 1_000_000, method: .roundHalfAwayFromZero).value)
        let fractional = req["fractional"] as? Bool ?? false
        let step = fractional ? CMTime(value: 1001, timescale: 30000)
                              : CMTime(value: variable && i % 3 == 0 ? 2 : 1, timescale: 30)
        time = CMTimeAdd(time, step)
    }
    input.markAsFinished()
    let sema = DispatchSemaphore(value: 0)
    writer.finishWriting { sema.signal() }
    guard sema.wait(timeout: .now() + 60) == .success, writer.status == .completed else {
        throw Failure(writer.error?.localizedDescription ?? "Fixture finalize failed")
    }
    if req["audio"] as? Bool == true {
        let format = AVAudioFormat(standardFormatWithSampleRate: 44100, channels: 1)!
        let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: 44100)!
        buffer.frameLength = 44100
        for i in 0..<44100 { buffer.floatChannelData![0][i] = Float(sin(Double(i) * 2 * .pi * 440 / 44100) * 0.1) }
        let audioURL = base.appendingPathComponent("tone.m4a")
        var file: AVAudioFile? = try AVAudioFile(forWriting: audioURL,
            settings: [AVFormatIDKey: kAudioFormatMPEG4AAC, AVSampleRateKey: 44100, AVNumberOfChannelsKey: 1, AVEncoderBitRateKey: 64000],
            commonFormat: .pcmFormatFloat32, interleaved: false)
        try file!.write(from: buffer)
        file = nil
        let silentURL = base.appendingPathComponent("silent.mp4")
        try FileManager.default.moveItem(at: base.appendingPathComponent("clip.mp4"), to: silentURL)
        let silent = AVURLAsset(url: silentURL), sound = AVURLAsset(url: audioURL)
        let composition = AVMutableComposition()
        let v = silent.tracks(withMediaType: .video)[0], a = sound.tracks(withMediaType: .audio)[0]
        try composition.addMutableTrack(withMediaType: .video, preferredTrackID: kCMPersistentTrackID_Invalid)!
            .insertTimeRange(v.timeRange, of: v, at: .zero)
        try composition.addMutableTrack(withMediaType: .audio, preferredTrackID: kCMPersistentTrackID_Invalid)!
            .insertTimeRange(a.timeRange, of: a, at: .zero)
        let export = AVAssetExportSession(asset: composition, presetName: AVAssetExportPresetPassthrough)!
        export.outputURL = base.appendingPathComponent("clip.mp4")
        export.outputFileType = .mp4
        let done = DispatchSemaphore(value: 0)
        export.exportAsynchronously { done.signal() }
        guard done.wait(timeout: .now() + 60) == .success, export.status == .completed else {
            throw Failure(export.error?.localizedDescription ?? "Audio fixture mux failed")
        }
    }
    return ["frame_times_us": frameTimes, "cover_us": frameTimes[12]]
}

#if !MOTIONLIVE_LIBRARY
@main
struct MotionLiveHelper {
static func main() {
do {
    let data = FileHandle.standardInput.readDataToEndOfFile()
    guard let request = try JSONSerialization.jsonObject(with: data) as? [String: Any], let mode = request["mode"] as? String else {
        throw Failure("Expected JSON request on stdin")
    }
    switch mode {
    case "movie": try emit(movie(request))
    case "legacy-gain": try emit(legacyGain(request))
    case "fixture": try emit(fixture(request))
    default: throw Failure("Unknown helper mode")
    }
} catch {
    try? emit(["error": error.localizedDescription])
    exit(1)
}
}
}
#endif
