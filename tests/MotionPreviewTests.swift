import Foundation

@main
struct MotionPreviewTests {
    static var checks = 0
    static func check(_ value: @autoclosure () -> Bool, _ message: String) {
        guard value() else { fputs("FAIL: \(message)\n", stderr); exit(1) }
        checks += 1
    }
    static func main() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("motion-preview-tests-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
        var regularSource: URL?
        for (name, options) in [("regular", [:] as [String: Any]), ("offset", ["start_us": 2_000_000]), ("vfr", ["variable": true]), ("fractional", ["fractional": true])] {
            let directory = root.appendingPathComponent(name)
            var request = options; request["folder"] = directory.path
            let fixtureInfo = try fixture(request)
            let stamp = (fixtureInfo["cover_us"] as! Int64)
            let still = Array(try Data(contentsOf: directory.appendingPathComponent("still.jpg")))
            let video = Array(try Data(contentsOf: directory.appendingPathComponent("clip.mp4")))
            let bytes = try AndroidExport.assemble(still: still, video: video, options: AndroidOptions(), time: Int(stamp), frame: 12)
            let source = directory.appendingPathComponent("motion.jpg")
            try Data(bytes).write(to: source)
            let session = try MotionPreviewSession.prepare(source: source, direction: "to-apple", token: StopToken())
            let times = fixtureInfo["frame_times_us"] as! [Int64]
            let expected = Double(times[12] - times[0]) / 1e6
            check(abs((session.coverTime ?? -100) - expected) < 0.00001, name + " cover uses presentation timeline")
            check(session.frameTimes.count == 30, name + " retains every visible frame")
            check(abs(session.frameTimes[0]) < 0.00001, name + " preview starts at zero")
            let first = try session.frame(at: -1), last = try session.frame(at: 1)
            check(first.time < last.time && last.time < session.duration, name + " hover endpoints decode real frames")
            check(first.image.width > 0 && last.image.height > 0, name + " decoded images exist")
            let firstData = first.image.dataProvider!.data! as Data, lastData = last.image.dataProvider!.data! as Data
            check(firstData != lastData, name + " hovering changes the actual image")
            let unchanged = try Data(contentsOf: source)
            check(unchanged == Data(bytes), name + " original remains untouched")
            check(MotionPreviewSession.frameIndex(fraction: .nan, duration: session.duration, times: session.frameTimes) == 0, name + " nonfinite pointer is bounded")
            if name == "regular" {
                regularSource = source
                for (label, timestamp) in [("missing", -1), ("outside", 99_000_000)] {
                    let candidate = directory.appendingPathComponent(label + ".jpg")
                    try Data(AndroidExport.assemble(still: still, video: video, options: AndroidOptions(), time: timestamp, frame: 12)).write(to: candidate)
                    let unknown = try MotionPreviewSession.prepare(source: candidate, direction: "to-apple", token: StopToken())
                    check(unknown.coverTime == nil && unknown.coverIssue != nil, label + " cover is never fabricated")
                    let unknownFrame = try unknown.frame(at: 0.5)
                    check(unknownFrame.image.width > 0, label + " still allows video preview")
                }
                let output = root.appendingPathComponent("live")
                let result = try NativeBatch.run(sources: [source], output: output, options: ConversionOptions(), token: StopToken()) { _ in }
                check(result.counts["converted"] == 1, "conversion retains shared cover mapping")
                let pair = output.appendingPathComponent("motion.jpg.livephoto/motion.jpg")
                let apple = try MotionPreviewSession.prepare(source: pair, direction: "to-android", token: StopToken())
                check(abs((apple.coverTime ?? -100) - expected) < 0.00001, "reverse paired Live Photo cover")
                let appleFrame = try apple.frame(at: 0.8)
                check(appleFrame.image.width > 0, "reverse hover decodes paired video")
            }
        }
        let cancelled = StopToken(); cancelled.cancel()
        do {
            _ = try MotionPreviewSession.prepare(source: regularSource!, direction: "to-apple", token: cancelled)
            check(false, "cancelled preview must stop")
        } catch { check(true, "cancelled preview stopped") }
        let before = try cacheFolders()
        var session: MotionPreviewSession? = try MotionPreviewSession.prepare(source: regularSource!, direction: "to-apple", token: StopToken())
        check(session?.duration != nil, "owned preview cache created")
        let created = try cacheFolders().subtracting(before)
        check(created.count == 1, "one private cache per motion-photo session")
        session = nil
        let remaining = try cacheFolders().intersection(created)
        check(remaining.isEmpty, "preview cache cleaned when session released")
        print("PASS \(checks) motion preview checks")
    }

    static func cacheFolders() throws -> Set<String> {
        Set(try FileManager.default.contentsOfDirectory(atPath: FileManager.default.temporaryDirectory.path).filter { $0.hasPrefix("MomentStays-preview-") })
    }
}
