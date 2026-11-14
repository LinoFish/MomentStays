import Foundation
import ImageIO
import CoreGraphics

@main
struct PhotoTimeRepairTests {
    static let zone = TimeZone(identifier: "Asia/Shanghai")!
    static func require(_ value: @autoclosure () throws -> Bool, _ message: String) throws {
        if try !value() { throw TimeRepairError("FAIL: " + message) }
    }
    static func fails(_ message: String, _ work: () throws -> Void) throws {
        do { try work() } catch { return }
        throw TimeRepairError("FAIL: expected rejection: " + message)
    }
    static func fixture(_ url: URL, type: String = "public.jpeg", date: String? = nil, count: Int = 1, includeMetadata: Bool = true, dateKey: CFString = kCGImagePropertyExifDateTimeOriginal) throws {
        let context = CGContext(data: nil, width: 32, height: 24, bitsPerComponent: 8,
            bytesPerRow: 128, space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)!
        context.setFillColor(CGColor(red: 0.2, green: 0.6, blue: 0.8, alpha: 1))
        context.fill(CGRect(x: 0, y: 0, width: 32, height: 24))
        context.setFillColor(CGColor(red: 0.8, green: 0.2, blue: 0.1, alpha: 1))
        context.fill(CGRect(x: 3, y: 4, width: 7, height: 9))
        guard let writer = CGImageDestinationCreateWithURL(url as CFURL, type as CFString, count, nil) else {
            throw TimeRepairError("Cannot create fixture: " + type)
        }
        var props: [String: Any] = [
            kCGImagePropertyOrientation as String: 6,
            kCGImagePropertyTIFFDictionary as String: [kCGImagePropertyTIFFMake as String: "Time repair tests"],
            kCGImagePropertyGPSDictionary as String: [kCGImagePropertyGPSLatitude as String: 31.2,
                kCGImagePropertyGPSLatitudeRef as String: "N", kCGImagePropertyGPSLongitude as String: 121.4,
                kCGImagePropertyGPSLongitudeRef as String: "E"]
        ]
        if let date = date { props[kCGImagePropertyExifDictionary as String] = [dateKey as String: date] }
        for _ in 0..<count { CGImageDestinationAddImage(writer, context.makeImage()!, includeMetadata ? props as CFDictionary : nil) }
        guard CGImageDestinationFinalize(writer) else { throw TimeRepairError("Cannot encode fixture: " + type) }
    }
    static func properties(_ url: URL) -> [String: Any] {
        let image = CGImageSourceCreateWithURL(url as CFURL, nil)!
        return CGImageSourceCopyPropertiesAtIndex(image, 0, nil) as! [String: Any]
    }
    static func pixels(_ url: URL) -> Data {
        let image = CGImageSourceCreateImageAtIndex(CGImageSourceCreateWithURL(url as CFURL, nil)!, 0, nil)!
        let context = CGContext(data: nil, width: image.width, height: image.height, bitsPerComponent: 8,
            bytesPerRow: image.width * 4, space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
        context.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
        return Data(bytes: context.data!, count: image.width * image.height * 4)
    }
    static func verify(_ before: URL, _ after: URL) throws {
        try require(pixels(before) == pixels(after), "decoded pixels unchanged")
        let a = properties(before), b = properties(after)
        try require(a[kCGImagePropertyOrientation as String] as? Int == b[kCGImagePropertyOrientation as String] as? Int, "orientation preserved")
        try require(a[kCGImagePropertyGPSDictionary as String] as? NSDictionary == b[kCGImagePropertyGPSDictionary as String] as? NSDictionary, "GPS preserved")
        let at = a[kCGImagePropertyTIFFDictionary as String] as? [String: Any]
        let bt = b[kCGImagePropertyTIFFDictionary as String] as? [String: Any]
        try require(at?[kCGImagePropertyTIFFMake as String] as? String == bt?[kCGImagePropertyTIFFMake as String] as? String, "camera make preserved")
    }
    static func parsing() throws {
        let expected = ISO8601DateFormatter().date(from: "2024-02-29T04:34:56Z")!
        for name in ["IMG_20240229_123456.jpg", "20240229123456.heic", "2024-02-29_123456.jpg", "20240229_12-34-56.jpg", "2024-02-29 12-34-56.png",
                     "2024_02_29_12_34_56.tif", "2024-02-29T12:34:56+08:00.jpg", "2024-02-29T04:34:56Z.jpg"] {
            let inferred = PhotoTimeRepair.inferredTime(filename: name, zone: zone)
            try require(inferred?.date == expected && inferred?.source == .filenameDateTime, "calendar: " + name)
        }
        let epoch = Int64(expected.timeIntervalSince1970)
        for (digits, factor) in [(10, Int64(1)), (13, 1000), (16, 1_000_000), (19, 1_000_000_000)] {
            let name = "photo_\(epoch * factor).jpg"
            let inferred = PhotoTimeRepair.inferredTime(filename: name, zone: TimeZone(secondsFromGMT: 0)!)
            try require(inferred?.date == expected && inferred?.source == .unixTimestamp, "Unix \(digits)")
        }
        let milli = PhotoTimeRepair.inferredTime(filename: "\(epoch * 1000 + 123).jpg", zone: zone)!
        try require(abs(milli.date.timeIntervalSince(expected) - 0.123) < 0.00001, "Unix milliseconds")
        let fraction = PhotoTimeRepair.inferredTime(filename: "20240229_123456.123.jpg", zone: zone)!
        try require(abs(fraction.date.timeIntervalSince(expected) - 0.123) < 0.00001, "calendar fraction")
        for name in ["1.jpg", "2024-02-29.jpg", "2023-02-29_12-34-56.jpg", "2024-02-30_12-34-56.jpg",
                     "2024-01-01_24-00-00.jpg", "2024-01-01T12:00:00+25:00.jpg", "12345678901234567890.jpg",
                     "123456789012.jpg", "202402291234567.jpg", "1700000000_1800000000.jpg",
                     "20240229_123456_1700000000.jpg", "20240229_123456_20240301_123456.jpg"] {
            try require(PhotoTimeRepair.inferredTime(filename: name, zone: zone) == nil, "invalid/ambiguous: " + name)
        }
        let la = TimeZone(identifier: "America/Los_Angeles")!
        try require(PhotoTimeRepair.inferredTime(filename: "2024-03-10_02-30-00.jpg", zone: la) == nil, "nonexistent DST wall time")
        print("PASS filename dates, timezone, leap years, fractions, Unix units, invalid and ambiguous names")
    }
    static func run() throws {
        try parsing()
        let fm = FileManager.default
        let root = fm.temporaryDirectory.appendingPathComponent("MomentStays-time-tests-" + UUID().uuidString)
        try fm.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? fm.removeItem(at: root) }
        let now = ISO8601DateFormatter().date(from: "2024-12-31T15:59:59Z")!
        var inputs: [PhotoTimeRepair.Input] = []
        for (index, name) in ["10.jpg", "2.jpg", "1.jpg", "IMG_20240229_123456.jpg", "1700000000000.jpg", "dated.jpg"].enumerated() {
            let url = root.appendingPathComponent("stage-\(index).jpg")
            try fixture(url, date: name == "dated.jpg" ? "2001:01:01 01:01:01" : nil)
            inputs.append(.init(photo: url, originalFilename: name))
        }
        let existingBytes = try Data(contentsOf: inputs.last!.photo)
        let plan = PhotoTimeRepair.planSecondStep(inputs: inputs + [inputs[0]], now: now, zone: zone)
        try require(plan.plans.count == 5 && plan.preserved.count == 1 && plan.rejected.isEmpty, "preserve existing EXIF; deduplicate input")
        let fallback = plan.plans.filter { $0.source == .filenameOrder }
        try require(fallback.map { $0.input.originalFilename } == ["1.jpg", "2.jpg", "10.jpg"], "natural sort by original filename, not staging filename")
        try require(fallback.map(\.date) == [now, now.addingTimeInterval(1), now.addingTimeInterval(2)], "fallback increments only unknown dates")
        try require(PhotoTimeRepair.formatter(zone: zone).string(from: fallback[1].date) == "2025:01:01 00:00:00", "cross-year sequence")
        try require(try Data(contentsOf: inputs.last!.photo) == existingBytes, "planning never modifies source")
        for item in plan.plans {
            let output = root.appendingPathComponent("out-" + item.input.photo.lastPathComponent)
            let before = try Data(contentsOf: item.input.photo)
            var verified = false
            try PhotoTimeRepair.write(item, to: output) { a, b in
                verified = true; try verify(a, b)
            }
            try require(verified, "feature verification called before publication")
            let exif = properties(output)[kCGImagePropertyExifDictionary as String] as! [String: Any]
            try require(exif[kCGImagePropertyExifDateTimeOriginal as String] as? String == PhotoTimeRepair.formatter(zone: zone).string(from: item.date), "write actual EXIF")
            try require(try Data(contentsOf: item.input.photo) == before, "original bytes unchanged")
            let outputBytes = try Data(contentsOf: output)
            try fails("existing output") { try PhotoTimeRepair.write(item, to: output, verifyFeatures: verify) }
            try require(try Data(contentsOf: output) == outputBytes, "output conflict never overwrites")
            let rerun = PhotoTimeRepair.planSecondStep(inputs: [.init(photo: output, originalFilename: "different.jpg")], now: now, zone: zone)
            try require(rerun.plans.isEmpty && rerun.preserved.count == 1, "second step is idempotent")
        }
        for (ext, type) in [("png", "public.png"), ("tiff", "public.tiff"), ("heic", "public.heic")] {
            let source = root.appendingPathComponent("fixture." + ext)
            if ext == "heic" {
                if let path = ProcessInfo.processInfo.environment["PHOTOTIME_HEIC_FIXTURE"] {
                    try fm.copyItem(at: URL(fileURLWithPath: path), to: source)
                } else {
                    do { try fixture(source, type: type) }
                    catch { print("SKIP HEIC fixture encoder unavailable; set PHOTOTIME_HEIC_FIXTURE"); continue }
                }
            } else { try fixture(source, type: type) }
            let planned = PhotoTimeRepair.planSecondStep(inputs: [.init(photo: source, originalFilename: "1." + ext)], now: now, zone: zone)
            try require(planned.rejected.isEmpty, ext + " readable")
            if planned.plans.isEmpty { print("SKIP \(ext) fixture already has capture time"); continue }
            try PhotoTimeRepair.write(planned.plans[0], to: root.appendingPathComponent("written." + ext), verifyFeatures: verify)
            print("PASS \(ext) EXIF write, pixels and metadata preservation")
        }
        for (ext, type) in [("jpg", "public.jpeg"), ("png", "public.png"), ("tiff", "public.tiff")] {
            let empty = root.appendingPathComponent("no-exif." + ext)
            try fixture(empty, type: type, includeMetadata: false)
            let before = try Data(contentsOf: empty)
            let planned = PhotoTimeRepair.planSecondStep(inputs: [.init(photo: empty, originalFilename: "2024-02-29_123456." + ext)], now: now, zone: zone)
            try require(planned.plans.count == 1 && planned.plans[0].source == .filenameDateTime, "completely missing EXIF " + ext)
            try PhotoTimeRepair.write(planned.plans[0], to: root.appendingPathComponent("added-exif." + ext), verifyFeatures: verify)
            try require(try Data(contentsOf: empty) == before, "no-EXIF original unchanged " + ext)
        }
        let digitized = root.appendingPathComponent("digitized.jpg")
        try fixture(digitized, date: "2001:01:01 01:01:01", dateKey: kCGImagePropertyExifDateTimeDigitized)
        let preserved = PhotoTimeRepair.planSecondStep(inputs: [.init(photo: digitized, originalFilename: "20240229_123456.jpg")], now: now, zone: zone)
        try require(preserved.preserved.count == 1 && preserved.plans.isEmpty, "digitization date alone prevents replacement")
        let rejectedOutput = root.appendingPathComponent("rejected.jpg")
        try fails("feature verifier rejects") {
            try PhotoTimeRepair.write(fallback[0], to: rejectedOutput) { _, _ in throw TimeRepairError("Simulated identifier/HDR mismatch") }
        }
        try require(!fm.fileExists(atPath: rejectedOutput.path), "failed feature verification never publishes")
        try Data("changed".utf8).write(to: fallback[0].input.photo)
        try fails("stale plan") { try PhotoTimeRepair.write(fallback[0], to: rejectedOutput, verifyFeatures: verify) }
        try require(!fm.fileExists(atPath: rejectedOutput.path), "stale input never publishes")
        let broken = root.appendingPathComponent("broken.jpg")
        try Data("broken".utf8).write(to: broken)
        let multi = root.appendingPathComponent("multi.tiff")
        try fixture(multi, type: "public.tiff", count: 2)
        let link = root.appendingPathComponent("link.jpg")
        try fm.createSymbolicLink(at: link, withDestinationURL: inputs[1].photo)
        let errors = PhotoTimeRepair.planSecondStep(inputs: [broken, multi, link].map { .init(photo: $0, originalFilename: $0.lastPathComponent) }, now: now, zone: zone)
        try require(errors.rejected.count == 3 && errors.plans.isEmpty, "broken, multi-frame and symlink rejected individually")
        try require(try fm.contentsOfDirectory(atPath: root.path).allSatisfy { !$0.hasPrefix(".writing-") }, "temporary files cleaned")
        print("PASS second-stage planning, natural order, existing EXIF, writes, idempotence, conflicts, stale inputs and cleanup")
    }
    static func main() {
        setbuf(stdout, nil)
        do { try run() } catch { fputs("\(error.localizedDescription)\n", stderr); exit(1) }
    }
}
