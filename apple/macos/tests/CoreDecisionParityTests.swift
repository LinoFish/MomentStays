import Foundation
import AVFoundation
import CryptoKit

// Parity of the Swift decisions with the shared Rust core (core/src/engine/decisions):
// filename capture time, action blocking reason, Live Photo still selection and
// per-track sample facts. Every corpus is deterministic; each decision also prints
// a SHA-256 digest of its results so a later build can be compared to this one.
@main
struct CoreDecisionParityTests {
    static var mismatches: [String: Int] = [:]
    static var examples: [String] = []
    static func mismatch(_ area: String, _ text: String) {
        mismatches[area, default: 0] += 1
        if mismatches[area]! <= 25 { examples.append(area + ": " + text) }
    }

    struct Rng {
        var state: UInt64
        mutating func next() -> UInt64 {
            state = state &* 6364136223846793005 &+ 1442695040888963407
            return state >> 33
        }
        mutating func int(_ n: Int) -> Int { Int(next() % UInt64(n)) }
        mutating func pick<T>(_ values: [T]) -> T { values[int(values.count)] }
    }

    final class Digest {
        var hash = SHA256(), count = 0
        func add(_ text: String) { hash.update(data: Data((text + "\n").utf8)); count += 1 }
        var value: String { hex(hash.finalize()) }
    }

    // MARK: filename-time

    static func filenameCorpus() -> [String] {
        var names = [
            "IMG_20240229_123456.jpg", "20240229123456.heic", "2024-02-29_123456.jpg", "20240229_12-34-56.jpg",
            "2024-02-29 12-34-56.png", "2024_02_29_12_34_56.tif", "2024-02-29T12:34:56+08:00.jpg",
            "2024-02-29T04:34:56Z.jpg", "2024-02-29t04:34:56z.jpg", "20240229_123456.123.jpg",
            "2024-02-29T12:34:56-0530.jpg", "1.jpg", "2024-02-29.jpg", "2023-02-29_12-34-56.jpg",
            "2024-02-30_12-34-56.jpg", "2024-01-01_24-00-00.jpg", "2024-01-01T12:00:00+25:00.jpg",
            "12345678901234567890.jpg", "123456789012.jpg", "202402291234567.jpg", "1700000000_1800000000.jpg",
            "20240229_123456_1700000000.jpg", "20240229_123456_20240301_123456.jpg", "2024-03-10_02-30-00.jpg",
            "2024-11-03_01-30-00.jpg", "2024-04-07_01-45-00.jpg", "2024-10-06_02-15-00.jpg",
            "1709181296.jpg", "1709181296123.jpg", "1709181296123456.jpg", "1709181296123456789.jpg",
            "photo_1709181296.jpg", "1709181296.123.jpg", "PXL_20240229_123456789.jpg",
            "IMG_20240229_123456.", ".20240229_123456", "dir/IMG_20240229_123456.jpg", "IMG_20240229_123456.jpg/",
            "IMG_20240229_123456", "IMG_20240229_123456.tar.gz", "IMG_20240229_123456.j pg", "20240229_123456.1234567891.jpg",
            "20240229_123456.5+08:00.jpg", "2024-02-29T12:34:56+14:00.jpg", "2024-02-29T12:34:56+14:01.jpg",
            "2024-02-29T12:34:56-1400.jpg", "2024-02-29T12:34:56+0860.jpg", "2024-02-29T12:34:56+08.jpg",
            "照片 2024-02-29 12-34-56.jpg", "📷20240229_123456.jpg", "é20240229123456é.jpg", "0000-01-01_00-00-00.jpg",
            "9999-12-31_23-59-59.jpg", "1970-01-01T00:00:00Z.jpg", "20240229T123456Z.jpg", "20240229t1234.56.jpg",
            "", ".", "..", "/", "a.b.c", "IMG_1234.JPG"
        ]
        let prefixes = ["", "IMG_", "PXL_", "Screenshot ", "a1", "x-", "VID_", "9"]
        let years = ["2024", "2023", "0000", "9999", "1970", "2000", "1900"]
        let months = ["02", "13", "00", "12", "03", "11", "10", "04"]
        let days = ["29", "30", "31", "01", "10", "03", "06", "07"]
        let times = ["123456", "12-34-56", "12:34:56", "24-00-00", "23-59-60", "02-30-00", "01-30-00", "1234-56", "12_3456", "023000", "013000"]
        let dateSeps = ["-", "_", "", ":", "."]
        let tSeps = ["T", "t", " ", "_", "-", "", "x"]
        let suffixes = ["", ".123", ".1234567891", "Z", "z", "+08:00", "-0530", "+1400", "+1401", "+2500", ".5+08:00",
                        "1", "_1700000000", "+08", "-00:00", "Z1", ".", ".12x", "_20240301_101010", "000"]
        let exts = [".jpg", ".HEIC", "", ".tar.gz", ".", ".1", ".j pg"]
        var rng = Rng(state: 0x5eed_0001)
        for _ in 0..<40_000 {
            var parts: [String] = [rng.pick(prefixes), rng.pick(years), rng.pick(dateSeps), rng.pick(months)]
            parts += [rng.pick(dateSeps), rng.pick(days), rng.pick(tSeps), rng.pick(times)]
            parts += [rng.pick(suffixes), rng.pick(exts)]
            names.append(parts.joined())
        }
        let alphabet = Array("0123456789012345678901234567890123456789-_:.T tZz+abcé中/📷")
        for _ in 0..<20_000 {
            let length = 1 + rng.int(30)
            names.append(String((0..<length).map { _ in rng.pick(alphabet) }))
        }
        // Unix digit runs of every length near the accepted ones.
        for length in 8...21 { names.append(String(repeating: "1", count: length) + ".jpg") }
        return names
    }

    static func describe(_ value: PhotoTimeRepair.InferredTime?) -> String {
        guard let value = value else { return "nil" }
        return value.source.rawValue + "@" + String(value.date.timeIntervalSinceReferenceDate.bitPattern, radix: 16)
    }

    static func filenameTime() -> String {
        let names = filenameCorpus(), digest = Digest()
        let zones = ["Asia/Shanghai", "America/Los_Angeles", "UTC", "Australia/Lord_Howe"].map { TimeZone(identifier: $0)! }
        var found = 0
        for zone in zones {
            let core = PhotoTimeRepair.coreInferredTimes(filenames: names, zone: zone)
            for (name, coreValue) in zip(names, core) {
                let swift = describe(PhotoTimeRepair.inferredTime(filename: name, zone: zone)), rust = describe(coreValue)
                if swift != rust { mismatch("filename-time", "\(zone.identifier) \(name.debugDescription): swift=\(swift) core=\(rust)") }
                if swift != "nil" { found += 1 }
                digest.add(zone.identifier + "|" + name + "|" + swift)
            }
        }
        print("filename-time: \(digest.count) name×zone cases (\(names.count) names, \(zones.count) zones, \(found) inferred), digest \(digest.value)")
        return digest.value
    }

    // MARK: action-blocking-reason

    static func blockingReason() throws -> String {
        // core/src/engine/decisions/hdr_kind.rs TABLE: every label with its code, and without
        // one (rows built in Swift fall back to the label).
        var labels: [(String, String?)] = []
        for (label, code) in [("HEIC（原样保留）","heic_preserved"),("Ultra HDR","ultra_hdr"),("Ultra HDR · 保留","ultra_hdr_preserved"),
                              ("荣耀私有 HDR","honor_private"),("ISO HDR","iso_hdr"),("旧 Apple HDR","apple_legacy"),("HEIC HDR","heic_hdr"),
                              ("OPPO UHDR（实验）","oppo_uhdr"),("Apple HDR","apple_hdr"),("SDR / 其他","sdr"),("SDR","sdr"),
                              ("原始图像","sdr"),("原始 JPEG","sdr"),("—","none")] {
            labels.append((label, code)); labels.append((label, nil))
        }
        labels += [("某 HDR 变体", "other"), ("某 HDR 变体", nil), ("未知", "other"), ("", nil)]
        let burst = try JSONDecoder().decode(BurstFrame.self, from: Data(#"{"group_id":"g","source_group":"s","index":0,"source_cover":false,"source_sha256":null,"supported":true,"reason":""}"#.utf8))
        var items: [PhotoQueueItem] = [], coverPaths: [String] = []
        var serial = 0
        for (label, code) in labels { for motion in [false, true] { for hasBurst in [false, true] { for confirmed in [false, true] {
            for supported in [nil, false, true] as [Bool?] { for reason in [nil, "自定义原因"] as [String?] {
                for ext in ["jpg", "heic", "HEIF", "png"] { for cover in [false, true] {
                    serial += 1
                    let url = URL(fileURLWithPath: "/tmp/momentstays-parity/item-\(serial).\(ext)")
                    var row = ScanRow(source: url, relative: url.lastPathComponent, isMotion: motion, hdr: label, cover: "0.4 秒",
                                      detail: "parity", confirmedStatic: confirmed)
                    row.burst = hasBurst ? burst : nil
                    row.privateHDRSupported = supported; row.privateHDRReason = reason; row.hdrKind = code
                    items.append(PhotoQueueItem(row))
                    if cover { coverPaths.append(NativeBatch.normalized(url).path) }
                } }
            } }
        } } } }
        var settingsList: [(ConversionSettings, Bool)] = []
        for legacy in [false, true] { for direction in ConversionDirection.allCases { for container in [PhotoContainer.jpeg, .heic] {
            for burstConversion in [nil, false, true] as [Bool?] { for liveConversion in [nil, false, true] as [Bool?] {
                for mode in ExportMode.allCases { for track in [VideoTrack.primary, .secondary] { for available in [false, true] {
                    var s = ConversionSettings()
                    s.legacyHDR = legacy; s.direction = direction; s.container = container
                    s.burstConversion = burstConversion; s.liveConversion = liveConversion
                    s.exportMode = mode; s.track = track
                    s.coverSelections = Dictionary(uniqueKeysWithValues: coverPaths.map { ($0, 1) })
                    settingsList.append((s, available))
                } } }
            } }
        } } }
        var rng = Rng(state: 0x5eed_0002)
        let digest = Digest()
        var blocked = 0, compared = 0
        for (index, entry) in settingsList.enumerated() {
            let (settings, available) = entry
            // Every item under a sweep of settings; a seeded sample under the rest.
            let chosen = index % 97 == 0 ? items : (0..<400).map { _ in items[rng.int(items.count)] }
            let core = WorkspaceModel.coreActionBlockingReasons(chosen, settings: settings, availableLive: available)
            for (item, coreValue) in zip(chosen, core) {
                let swift = WorkspaceModel.swiftActionBlockingReason(item, settings: settings, availableLive: available)
                compared += 1
                if swift != nil { blocked += 1 }
                if swift != coreValue {
                    mismatch("action-blocking-reason", "settings#\(index) item=\(item.name) hdr=\(item.hdr)/\(item.row.hdrKind ?? "nil") motion=\(item.isMotion) burst=\(item.row.burst != nil) static=\(item.row.confirmedStatic) supported=\(String(describing: item.row.privateHDRSupported)) cover=\(settings.coverIndex(for: item.source) != nil) legacy=\(settings.legacyHDR) dir=\(settings.direction.rawValue) container=\(settings.container.rawValue) burstConv=\(String(describing: settings.burstConversion)) liveConv=\(String(describing: settings.liveConversion)) mode=\(settings.exportMode.rawValue) track=\(settings.track.rawValue) live=\(available): swift=\(swift ?? "nil") core=\(coreValue ?? "nil")")
                }
                digest.add("\(index)|\(item.name)|\(swift ?? "nil")")
            }
        }
        print("action-blocking-reason: \(compared) item×settings cases (\(items.count) items, \(settingsList.count) settings, \(blocked) blocked), digest \(digest.value)")
        return digest.value
    }

    // MARK: live-selection

    static func liveSelection(root: URL) throws -> String {
        let fm = FileManager.default
        let clipFolder = root.appendingPathComponent("clip")
        let made = try fixture(["folder": clipFolder.path])
        let cover = made["cover_us"] as! Int64
        let still = Array(try Data(contentsOf: clipFolder.appendingPathComponent("still.jpg")))
        func movie(_ id: String, _ url: URL) throws {
            _ = try RustCore.engine(["mode":"movie","input":clipFolder.appendingPathComponent("clip.mp4").path,
                                     "identifier":id,"timestamp_us":cover,"output":url.path])
        }
        func photo(_ id: String?, _ url: URL) throws {
            let bytes: [UInt8]
            if let id = id { bytes = try fixtureStill(still, uuid: id) } else { bytes = still }
            try Data(bytes).write(to: url)
        }
        let x = "6F2D7C2A-0000-4000-8000-00000000000A", y = "6F2D7C2A-0000-4000-8000-00000000000B"
        var cases: [(String, URL)] = []
        func folder(_ name: String) throws -> URL {
            let url = root.appendingPathComponent(name); try fm.createDirectory(at: url, withIntermediateDirectories: true); return url
        }
        var d = try folder("pair")
        try photo(x, d.appendingPathComponent("IMG_0001.jpg")); try movie(x, d.appendingPathComponent("IMG_0001.mov"))
        try photo(nil, d.appendingPathComponent("plain.jpg")); try photo(y, d.appendingPathComponent("other.jpeg"))
        try Data("notes".utf8).write(to: d.appendingPathComponent("notes.txt"))
        try fm.createDirectory(at: d.appendingPathComponent("folder.jpg"), withIntermediateDirectories: true)
        try movie(y, d.appendingPathComponent("other.MOV"))
        cases += [("pair", d.appendingPathComponent("IMG_0001.mov")), ("pair-other", d.appendingPathComponent("other.MOV"))]
        d = try folder("duplicate")
        try photo(x, d.appendingPathComponent("a.jpg")); try photo(x, d.appendingPathComponent("b.JPG")); try movie(x, d.appendingPathComponent("a.mov"))
        cases.append(("duplicate", d.appendingPathComponent("a.mov")))
        d = try folder("missing-still")
        try photo(y, d.appendingPathComponent("a.jpg")); try movie(x, d.appendingPathComponent("a.mov"))
        cases.append(("missing-still", d.appendingPathComponent("a.mov")))
        d = try folder("no-identifier")
        try photo(x, d.appendingPathComponent("a.jpg"))
        try fm.copyItem(at: clipFolder.appendingPathComponent("clip.mp4"), to: d.appendingPathComponent("a.mov"))
        cases.append(("no-identifier", d.appendingPathComponent("a.mov")))
        d = try folder("upper-and-mp4")
        try photo(x, d.appendingPathComponent("IMG_0002.JPEG")); try movie(x, d.appendingPathComponent("IMG_0002.mp4"))
        cases.append(("upper-and-mp4", d.appendingPathComponent("IMG_0002.mp4")))
        d = try folder("truncated-sibling")
        try photo(x, d.appendingPathComponent("a.jpg")); try movie(x, d.appendingPathComponent("a.mov"))
        let full = try fixtureStill(still, uuid: x)
        try Data(full.prefix(full.count * 2 / 3)).write(to: d.appendingPathComponent("cut.jpg"))
        cases.append(("truncated-sibling", d.appendingPathComponent("a.mov")))
        d = try folder("hidden-and-symlink")
        try photo(x, d.appendingPathComponent(".hidden.jpg")); try movie(x, d.appendingPathComponent("a.mov"))
        cases.append(("hidden", d.appendingPathComponent("a.mov")))
        d = try folder("symlink")
        try photo(x, root.appendingPathComponent("outside.jpg"))
        try fm.createSymbolicLink(at: d.appendingPathComponent("link.jpg"), withDestinationURL: root.appendingPathComponent("outside.jpg"))
        try movie(x, d.appendingPathComponent("a.mov"))
        cases.append(("symlink", d.appendingPathComponent("a.mov")))
        let digest = Digest()
        func result(_ work: () throws -> [URL]) -> String {
            do { return "ok:" + (try work()).map { $0.path.replacingOccurrences(of: NativeBatch.normalized(root).path, with: "<root>") }.joined(separator: ",") }
            catch { return "error:" + error.localizedDescription }
        }
        for (name, url) in cases {
            let swift = result { try AndroidExport.selection([url]).entries }
            let core = result { try NativeBatch.selection([try AndroidExport.coreLiveStill(url)]).entries }
            if swift != core { mismatch("live-selection", "\(name): swift=\(swift) core=\(core)") }
            print("  live-selection \(name): \(swift)")
            digest.add(name + "|" + swift)
        }
        print("live-selection: \(digest.count) cases, digest \(digest.value)")
        return digest.value
    }

    // MARK: track-info (reported; parity decides whether the preview may switch)

    static func trackParity(root: URL, repository: URL) throws -> (cases: Int, equal: Int) {
        var movies: [(String, URL)] = []
        for (name, request) in [("plain", [:]), ("variable", ["variable": true]), ("fractional", ["fractional": true]),
                                ("offset-2s", ["start_us": 2_000_000]), ("audio", ["audio": true])] as [(String, [String:Any])] {
            let folder = root.appendingPathComponent("track-" + name)
            var req = request; req["folder"] = folder.path
            let made = try fixture(req)
            let clip = folder.appendingPathComponent("clip.mp4")
            movies.append((name, clip))
            let mov = folder.appendingPathComponent("live.mov")
            do {
                _ = try RustCore.engine(["mode":"movie","input":clip.path,"identifier":"6F2D7C2A-0000-4000-8000-0000000000C0",
                                         "timestamp_us":made["cover_us"] as! Int64,"output":mov.path])
                movies.append((name + "-live-mov", mov))
            } catch { print("  track-info \(name): live mov not made: \(error.localizedDescription)") }
            let still = Array(try Data(contentsOf: folder.appendingPathComponent("still.jpg")))
            if let motion = try? fixtureMotion(still: still, video: Array(Data(contentsOf: clip)), options: AndroidOptions(),
                                               time: Int(made["cover_us"] as! Int64), frame: 12) {
                let photo = try PhotoFormat.parse(motion)
                let embedded = folder.appendingPathComponent("embedded.mp4")
                try Data(photo.video).write(to: embedded)
                movies.append((name + "-motion-photo", embedded))
            }
        }
        let sample = repository.appendingPathComponent("core/tests/fixtures-samples/real.mp4.b64")
        if let text = try? String(contentsOf: sample, encoding: .utf8),
           let data = Data(base64Encoded: text.components(separatedBy: .whitespacesAndNewlines).joined()) {
            let url = root.appendingPathComponent("real.mp4"); try data.write(to: url); movies.append(("real-sample", url))
        }
        var cases = 0, equal = 0
        for (name, url) in movies {
            let asset = AVURLAsset(url: url)
            let tracks = asset.tracks
            let core: [CoreTrackInfo]
            do { core = try coreTrackInfo(url) } catch {
                print("TRACK-INFO \(name): core error \(error.localizedDescription)"); cases += 1; continue
            }
            if tracks.count != core.count {
                print("TRACK-INFO \(name): AVFoundation \(tracks.count) tracks, core \(core.count)")
            }
            for (index, (track, rust)) in zip(tracks, core).enumerated() {
                cases += 1
                let label = "\(name)#\(index) \(track.mediaType.rawValue)/\(rust.handler)"
                guard let swift = try? CoreDecisionParityTests.readerInfo(asset, track) else {
                    print("TRACK-INFO \(label): AVAssetReader cannot read this track"); continue
                }
                let ts = rust.timescale
                func ticks(_ t: CMTime) -> Int64? {
                    guard t.isNumeric else { return nil }
                    let v = CMTimeConvertScale(t, timescale: ts, method: .default)
                    return CMTimeCompare(CMTime(value: v.value, timescale: ts), t) == 0 ? v.value : nil
                }
                let pts = swift.pts.map(ticks), durations = swift.durations.map(ticks)
                let n = swift.pts.count
                func agree<T: Equatable>(_ a: [T], _ b: [T]) -> Int { zip(a, b).filter { $0 == $1 }.count }
                let media = agree(pts, rust.mediaPTS.map { Optional($0) })
                let presentedByReader = agree(pts, rust.presentedPTS)
                // Swift MotionPreview maps reader timestamps through track.segments.
                let mapped: [Int64?] = swift.pts.map { point in
                    if track.segments.isEmpty { return ticks(point) }
                    let hits = track.segments.filter { !$0.isEmpty && CMTimeRangeContainsTime($0.timeMapping.source, time: point) }
                    guard hits.count == 1 else { return nil }
                    return ticks(CMTimeMapTimeFromRangeToRange(point, fromRange: hits[0].timeMapping.source, toRange: hits[0].timeMapping.target))
                }
                let presentedMapping = agree(mapped, rust.presentedPTS)
                let duration = agree(durations, rust.durations.map { Optional($0) })
                let sizes = agree(swift.sizes, rust.sizes)
                let same = n == rust.mediaPTS.count && media == n && presentedMapping == n && duration == n && sizes == n && swift.hash == rust.hash
                if same { equal += 1 }
                print("\(same ? "track-info" : "TRACK-INFO") \(label): samples swift=\(n) core=\(rust.mediaPTS.count) ts=\(ts) segments=\(track.segments.count) " +
                      "readerPTS==mediaPTS \(media)/\(n) readerPTS==presentedPTS \(presentedByReader)/\(n) segmentMapped==presentedPTS \(presentedMapping)/\(n) " +
                      "durations \(duration)/\(n) sizes \(sizes)/\(n) hash \(swift.hash == rust.hash ? "equal" : "differs")")
                if !same {
                    let shown = min(4, n)
                    print("    first reader PTS \(pts.prefix(shown)) core media \(rust.mediaPTS.prefix(shown)) core presented \(rust.presentedPTS.prefix(shown)) segment-mapped \(mapped.prefix(shown))")
                    var edits: [String] = []
                    for segment in track.segments {
                        let source = segment.timeMapping.source, target = segment.timeMapping.target
                        edits.append(String(format: "%.6f+%.6f->%.6f", seconds(source.start), seconds(source.duration),
                                            seconds(target.start)) + (segment.isEmpty ? " empty" : ""))
                    }
                    print("    first reader durations \(durations.prefix(shown)) core \(rust.durations.prefix(shown)); edits \(edits)")
                }
            }
        }
        print("track-info: \(equal)/\(cases) tracks identical between AVAssetReader+segments and core")
        return (cases, equal)
    }

    static func readerInfo(_ asset: AVAsset, _ track: AVAssetTrack) throws -> TrackInfo { try trackInfo(asset, track) }

    static func main() {
        let repository = URL(fileURLWithPath: CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : FileManager.default.currentDirectoryPath)
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("MomentStays-core-parity-" + UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        do {
            try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
            var digests: [String: String] = [:]
            digests["filename-time"] = filenameTime()
            digests["action-blocking-reason"] = try blockingReason()
            digests["live-selection"] = try liveSelection(root: root)
            let tracks = try trackParity(root: root, repository: repository)
            for line in examples { print("MISMATCH " + line) }
            let data = try JSONSerialization.data(withJSONObject: ["digests": digests, "mismatches": mismatches,
                "track_info": ["tracks": tracks.cases, "identical": tracks.equal]], options: [.sortedKeys])
            print("SUMMARY " + String(decoding: data, as: UTF8.self))
            let failed = mismatches.filter { $0.value > 0 }
            if !failed.isEmpty { fputs("FAIL: core decision parity mismatches \(failed)\n", stderr); exit(1) }
            print("PASS core decision parity (filename-time, action-blocking-reason, live-selection)")
        } catch {
            fputs("FAIL: \(error.localizedDescription)\n", stderr); exit(1)
        }
    }
}
