import Foundation
import AVFoundation
import ImageIO
import CoreGraphics

// Real system-codec input generation, linked into headless tests only.
func waitReady(_ input: AVAssetWriterInput, _ writer: AVAssetWriter) throws {
    let deadline = Date().addingTimeInterval(90)
    while !input.isReadyForMoreMediaData {
        if writer.status == .failed || writer.status == .cancelled || Date() > deadline {
            throw Failure(writer.error?.localizedDescription ?? "Writer readiness timed out")
        }
        Thread.sleep(forTimeInterval: 0.001)
    }
}

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

func fixtureMotion(still: [UInt8], video: [UInt8], options: AndroidOptions, time: Int, frame: Int) throws -> [UInt8] {
    let result = try RustCore.engine(["mode":"android-assemble", "still_base64":Data(still).base64EncodedString(),
        "video_base64":Data(video).base64EncodedString(), "vendor":options.vendor,"time_us":max(0,time),"frame":frame])
    guard let text = result["data_base64"] as? String, let data = Data(base64Encoded:text) else { throw Failure("Missing assembled fixture") }
    var bytes = Array(data)
    if time < 0 {
        // Deliberately corrupt a valid SDR test fixture's cover metadata.
        // Production writers correctly refuse a negative cover; never disable that check.
        let needle = Array("GCamera:MotionPhotoPresentationTimestampUs=\"0\"".utf8)
        var at = 2, changed = false
        while at + 4 <= bytes.count && bytes[at] == 255 && bytes[at + 1] != 218 {
            let length = Int(bytes[at + 2]) * 256 + Int(bytes[at + 3])
            guard length >= 2, at + 2 + length <= bytes.count else { throw Failure("Invalid test JPEG") }
            if bytes[at + 1] == 225, length >= needle.count + 2 {
                for start in (at + 4)...(at + 2 + length - needle.count) where Array(bytes[start..<(start + needle.count)]) == needle {
                    bytes[start + needle.count - 2] = 49
                    bytes.insert(45,at:start + needle.count - 2)
                    bytes[at + 2] = UInt8((length + 1) >> 8); bytes[at + 3] = UInt8((length + 1) & 255)
                    changed = true; break
                }
            }
            if changed { break }
            at += 2 + length
        }
        guard changed else { throw Failure("Missing fixture cover metadata") }
    }
    return bytes
}
func fixtureStill(_ still: [UInt8], uuid: String, updateIdentifier: Bool = true) throws -> [UInt8] {
    let result = try RustCore.engine(["mode":"live-still", "data_base64":Data(still).base64EncodedString(),
        "identifier":uuid,"update_identifier":updateIdentifier])
    guard let text = result["data_base64"] as? String, let data = Data(base64Encoded:text) else { throw Failure("Missing still fixture") }
    return Array(data)
}
