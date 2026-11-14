// Adapted from the sibling PhotoTime project's PhotoEngine.swift.
// Second-stage support only: no UI, automatic pipeline invocation or manifest mutation.
import Foundation
import ImageIO
import CryptoKit

struct TimeRepairError: LocalizedError {
    let message: String
    var errorDescription: String? { message }
    init(_ message: String) { self.message = message }
}

enum PhotoTimeRepair {
    struct Input {
        // The first stage's still output, never the original Motion Photo container.
        let photo: URL
        // Original source basename, independent of conversion/temporary output names.
        let originalFilename: String
    }
    enum TimeSource: String { case filenameDateTime, unixTimestamp, filenameOrder }
    struct Plan {
        let input: Input
        let date: Date
        let source: TimeSource
        let zone: TimeZone
        fileprivate let sourceDigest: Data
    }
    struct Rejection {
        let input: Input
        let reason: String
    }
    struct PlanningResult {
        var plans: [Plan] = []
        var preserved: [Input] = []
        var rejected: [Rejection] = []
    }

    /// Run after conversion, before hashing outputs, sealing manifests and Photos import.
    /// The caller publishes the new copy only after its format-specific verification succeeds.
    static func planSecondStep(inputs: [Input], now: Date = Date(), zone: TimeZone = .current) -> PlanningResult {
        var result = PlanningResult()
        var seen = Set<URL>()
        var fallback = 0
        let ordered = inputs.sorted {
            let order = $0.originalFilename.compare($1.originalFilename,
                options: [.numeric, .caseInsensitive], locale: Locale(identifier: "en_US_POSIX"))
            return order == .orderedSame ? $0.photo.path < $1.photo.path : order == .orderedAscending
        }
        for input in ordered {
            let url = input.photo.standardizedFileURL
            guard seen.insert(url).inserted else { continue }
            do {
                let info = try url.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey])
                guard info.isRegularFile == true, info.isSymbolicLink != true else {
                    throw TimeRepairError("仅支持普通照片文件。")
                }
                let data = try Data(contentsOf: url)
                guard let image = CGImageSourceCreateWithData(data as CFData, nil),
                      CGImageSourceGetCount(image) == 1,
                      let properties = CGImageSourceCopyPropertiesAtIndex(image, 0, nil) as? [String: Any],
                      (properties[kCGImagePropertyPixelWidth as String] as? Int ?? 0) > 0,
                      (properties[kCGImagePropertyPixelHeight as String] as? Int ?? 0) > 0,
                      let type = CGImageSourceGetType(image),
                      ["public.jpeg", "public.heic", "public.heif", "public.png", "public.tiff"].contains(type as String)
                else { throw TimeRepairError("不支持的格式、损坏或多帧照片。") }
                let exif = properties[kCGImagePropertyExifDictionary as String] as? [String: Any] ?? [:]
                let tiff = properties[kCGImagePropertyTIFFDictionary as String] as? [String: Any] ?? [:]
                // Be conservative: never replace existing nonempty capture/digitization/TIFF dates,
                // even when the text is malformed or only XMP exposes the date.
                let iptc = properties[kCGImagePropertyIPTCDictionary as String] as? [String:Any] ?? [:]
                let times = [iptc["DateCreated"] as? String,iptc["DigitalCreationDate"] as? String,
                             exif[kCGImagePropertyExifDateTimeOriginal as String] as? String,
                             exif[kCGImagePropertyExifDateTimeDigitized as String] as? String,
                             tiff[kCGImagePropertyTIFFDateTime as String] as? String, xmpDate(image)]
                if times.compactMap({ $0 }).contains(where: { !$0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }) {
                    result.preserved.append(input); continue
                }
                let inferred = inferredTime(filename: input.originalFilename, zone: zone)
                let date = inferred?.date ?? now.addingTimeInterval(Double(fallback))
                guard date.timeIntervalSince1970.isFinite else { throw TimeRepairError("时间无效。") }
                var calendar = Calendar(identifier: .gregorian); calendar.timeZone = zone
                guard (1...9999).contains(calendar.component(.year, from: date)) else {
                    throw TimeRepairError("时间超出 EXIF 日期范围。")
                }
                if inferred == nil { fallback += 1 }
                result.plans.append(Plan(input: input, date: date, source: inferred?.source ?? .filenameOrder,
                                         zone: zone, sourceDigest: Data(SHA256.hash(data: data))))
            } catch { result.rejected.append(Rejection(input: input, reason: error.localizedDescription)) }
        }
        return result
    }

    static func formatter(zone: TimeZone, format: String = "yyyy:MM:dd HH:mm:ss") -> DateFormatter {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.calendar = Calendar(identifier: .gregorian)
        formatter.timeZone = zone
        formatter.dateFormat = format
        return formatter
    }

    static func write(_ photo: Plan, to destination: URL,
                      verifyFeatures: (URL, URL) throws -> Void) throws {
        let zone = photo.zone
        guard !FileManager.default.fileExists(atPath: destination.path) else {
            throw TimeRepairError("输出文件已存在，未覆盖。")
        }
        guard let source = CGImageSourceCreateWithURL(photo.input.photo as CFURL, nil),
              CGImageSourceGetCount(source) == 1,
              let type = CGImageSourceGetType(source) else { throw TimeRepairError("无法读取单张静态照片。") }
        if Data(SHA256.hash(data: try Data(contentsOf: photo.input.photo))) != photo.sourceDigest {
            throw TimeRepairError("照片在计划后发生变化，请重新生成计划。")
        }
        let stamp = formatter(zone: zone).string(from: photo.date)
        // Image I/O's property-to-XMP bridge requires ISO 8601, not EXIF's colon format.
        let xmpStamp = formatter(zone: zone, format: "yyyy-MM-dd'T'HH:mm:ssXXXXX").string(from: photo.date)
        let metadata = CGImageMetadataCreateMutable()
        let fields: [(CFString, CFString, String)] = [
            (kCGImagePropertyExifDictionary, kCGImagePropertyExifDateTimeOriginal, xmpStamp),
            (kCGImagePropertyExifDictionary, kCGImagePropertyExifDateTimeDigitized, xmpStamp),
            (kCGImagePropertyTIFFDictionary, kCGImagePropertyTIFFDateTime, xmpStamp),
            (kCGImagePropertyExifDictionary, kCGImagePropertyExifOffsetTimeOriginal, offset(for: photo.date, zone: zone)),
            (kCGImagePropertyExifDictionary, kCGImagePropertyExifOffsetTimeDigitized, offset(for: photo.date, zone: zone)),
            (kCGImagePropertyExifDictionary, kCGImagePropertyExifOffsetTime, offset(for: photo.date, zone: zone))
        ]
        for (dictionary, property, value) in fields {
            guard CGImageMetadataSetValueMatchingImageProperty(metadata, dictionary, property, value as CFString)
            else { throw TimeRepairError("无法设置拍摄时间元数据。") }
        }

        let temporary = destination.deletingLastPathComponent()
            .appendingPathComponent(".writing-\(UUID().uuidString).\(destination.pathExtension)")
        defer { try? FileManager.default.removeItem(at: temporary) }
        guard let writer = CGImageDestinationCreateWithURL(temporary as CFURL, type, 1, nil)
        else { throw TimeRepairError("系统不支持写入此图片格式。") }
        if ["public.png", "public.tiff"].contains(type as String) {
            // On Monterey the copy API leaves existing PNG EXIF dates unchanged.
            // PNG/TIFF instead use lossless encoding with explicit EXIF properties.
            var properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [String: Any] ?? [:]
            var exif = properties[kCGImagePropertyExifDictionary as String] as? [String: Any] ?? [:]
            exif[kCGImagePropertyExifDateTimeOriginal as String] = stamp
            exif[kCGImagePropertyExifDateTimeDigitized as String] = stamp
            for key in [kCGImagePropertyExifOffsetTime, kCGImagePropertyExifOffsetTimeOriginal, kCGImagePropertyExifOffsetTimeDigitized] {
                exif[key as String] = offset(for: photo.date, zone: zone)
            }
            properties[kCGImagePropertyExifDictionary as String] = exif
            var tiff = properties[kCGImagePropertyTIFFDictionary as String] as? [String: Any] ?? [:]
            tiff[kCGImagePropertyTIFFDateTime as String] = stamp
            if type as String == "public.tiff" { tiff[kCGImagePropertyTIFFCompression as String] = 5 }
            properties[kCGImagePropertyTIFFDictionary as String] = tiff
            CGImageDestinationAddImageFromSource(writer, source, 0, properties as CFDictionary)
            guard CGImageDestinationFinalize(writer) else { throw TimeRepairError("无损格式导出失败。") }
        } else {
            var error: Unmanaged<CFError>?
            let options: [CFString: Any] = [kCGImageDestinationMetadata: metadata,
                                          kCGImageDestinationMergeMetadata: true]
            guard CGImageDestinationCopyImageSource(writer, source, options as CFDictionary, &error) else {
                let detail = error?.takeRetainedValue().localizedDescription ?? "系统不支持此格式的无损元数据写入。"
                throw TimeRepairError(detail)
            }
        }
        // Read the actual output back before publishing it. Never silently produce an untagged copy.
        guard let check = CGImageSourceCreateWithURL(temporary as CFURL, nil),
              let properties = CGImageSourceCopyPropertiesAtIndex(check, 0, nil) as? [String: Any],
              let exif = properties[kCGImagePropertyExifDictionary as String] as? [String: Any],
              exif[kCGImagePropertyExifDateTimeOriginal as String] as? String == stamp,
              exif[kCGImagePropertyExifDateTimeDigitized as String] as? String == stamp else {
            throw TimeRepairError("写入后校验失败，未导出此照片。")
        }
        // Monterey's HEIC writer stores the offset in the XMP date, without exposing
        // EXIF OffsetTimeOriginal. Accept that representation only after reading it back.
        let exifOffset = exif[kCGImagePropertyExifOffsetTimeOriginal as String] as? String
        guard exifOffset == offset(for: photo.date, zone: zone) || xmpDate(check) == xmpStamp
        else { throw TimeRepairError("时区写入校验失败，未导出此照片。") }
        try verifyFeatures(photo.input.photo, temporary)
        guard Data(SHA256.hash(data: try Data(contentsOf: photo.input.photo))) == photo.sourceDigest else {
            throw TimeRepairError("写入期间来源发生变化，请重新生成计划。")
        }
        try FileManager.default.moveItem(at: temporary, to: destination)
    }

    static func xmpDate(_ source: CGImageSource) -> String? {
        guard let metadata = CGImageSourceCopyMetadataAtIndex(source, 0, nil),
              let tag = CGImageMetadataCopyTagMatchingImageProperty(metadata,
                kCGImagePropertyExifDictionary, kCGImagePropertyExifDateTimeOriginal)
        else { return nil }
        return CGImageMetadataTagCopyValue(tag) as? String
    }

    static func offset(for date: Date, zone: TimeZone) -> String {
        let seconds = zone.secondsFromGMT(for: date)
        return String(format: "%@%02d:%02d", seconds < 0 ? "-" : "+", abs(seconds) / 3600, abs(seconds) % 3600 / 60)
    }

}

extension PhotoTimeRepair {
    struct InferredTime {
        let date: Date
        let source: TimeSource
    }

    /// Complete date+time or Unix tokens only. Invalid or ambiguous names use the
    /// caller's filename-order fallback, never file modification/creation dates.
    static func inferredTime(filename: String, zone: TimeZone) -> InferredTime? {
        let stem = ((filename as NSString).lastPathComponent as NSString).deletingPathExtension
        let text = stem as NSString
        let range = NSRange(location: 0, length: text.length)
        let suffix = #"(?:\.(\d{1,9}))?(Z|[+-]\d{2}:?\d{2})?(?!\d)"#
        let patterns = [
            #"(?i)(?<!\d)(\d{4})[-_](\d{2})[-_](\d{2})[T _-](\d{2})[-_:]?(\d{2})[-_:]?(\d{2})"# + suffix,
            #"(?i)(?<!\d)(\d{4})(\d{2})(\d{2})[T _-]?(\d{2})[-_:]?(\d{2})[-_:]?(\d{2})"# + suffix
        ]
        var candidates: [InferredTime] = []
        var calendarRanges: [NSRange] = []
        var invalidCalendar = false
        for pattern in patterns {
            let regex = try! NSRegularExpression(pattern: pattern)
            for match in regex.matches(in: stem, range: range) {
                calendarRanges.append(match.range)
                let values = (1...6).map { Int(text.substring(with: match.range(at: $0)))! }
                var calendar = Calendar(identifier: .gregorian)
                calendar.timeZone = zone
                if match.range(at: 8).location != NSNotFound {
                    let token = text.substring(with: match.range(at: 8)).uppercased()
                    if token == "Z" { calendar.timeZone = TimeZone(secondsFromGMT: 0)! }
                    else {
                        let digits = token.dropFirst().replacingOccurrences(of: ":", with: "")
                        let hours = Int(digits.prefix(2))!, minutes = Int(digits.suffix(2))!
                        guard hours <= 14, minutes < 60, hours < 14 || minutes == 0,
                              let explicitZone = TimeZone(secondsFromGMT: (token.hasPrefix("-") ? -1 : 1) * (hours * 3600 + minutes * 60))
                        else { invalidCalendar = true; continue }
                        calendar.timeZone = explicitZone
                    }
                }
                let parts = DateComponents(year: values[0], month: values[1], day: values[2],
                                           hour: values[3], minute: values[4], second: values[5])
                guard (1...9999).contains(values[0]), let date = calendar.date(from: parts),
                      calendar.dateComponents([.year,.month,.day,.hour,.minute,.second], from: date) == parts
                else { invalidCalendar = true; continue }
                let fraction = match.range(at: 7).location == NSNotFound ? 0 : Double("0." + text.substring(with: match.range(at: 7)))!
                candidates.append(InferredTime(date: date.addingTimeInterval(fraction), source: .filenameDateTime))
            }
        }
        let unix = try! NSRegularExpression(pattern: #"(?<!\d)(\d{10}|\d{13}|\d{16}|\d{19})(?!\d)"#)
        for match in unix.matches(in: stem, range: range) {
            guard !calendarRanges.contains(where: { NSIntersectionRange($0, match.range).length > 0 }) else { continue }
            let token = text.substring(with: match.range(at: 1))
            guard let value = Double(token) else { continue }
            candidates.append(InferredTime(date: Date(timeIntervalSince1970: value / pow(10, Double(token.count - 10))), source: .unixTimestamp))
        }
        return !invalidCalendar && candidates.count == 1 ? candidates[0] : nil
    }
}
