import Foundation
import AVFoundation
import CryptoKit

// Parity of the Swift decisions with the shared Rust core (core/src/engine/decisions):
// filename capture time, action blocking reason, queue state, Live Photo still
// selection, input selection / output validation and per-track sample facts. Every
// corpus is deterministic, and each decision prints a SHA-256 digest of its results.
// A decision whose Swift implementation was removed is checked against the digest the
// removed Swift code produced on the same corpus (recorded by this test in CI before
// the switch).
//   action-blocking-reason, live-selection, filename-time: legacy Swift digests from
//   run 46057504970 (filename-time legacy == core per case in run 46059778452).
//   queue-state: legacy Swift digest from run 46066645615 (legacy == core on all
//   4,324,320 cases there, 0 mismatches).
@main
struct CoreDecisionParityTests {
    static let legacySwiftDigests = [
        "action-blocking-reason": "75e5b2de2635069980c680baf3df66ca33acb89355016ac0d460d15af7f9b0c7",
        "live-selection": "4859f98b15ef2ba1784bd0928c4f17d00e7fb5d7257c105b600c658d62dd7eea",
        "filename-time": "f74b464e2a909efa69bf714106935f1697adccfc6535120ee0380b1d98d7e21b",
        "queue-state": "d25639bae39519462904fcc61f63b0860333e96d891587d639518125e35b94aa",
    ]
    static func golden(_ area: String, _ digest: String) {
        guard let expected = legacySwiftDigests[area] else { return }
        if digest != expected { mismatch(area, "digest \(digest) differs from the removed Swift implementation's \(expected)") }
        else { print("\(area): identical to the removed Swift implementation on the whole corpus") }
    }
    static var mismatches: [String: Int] = [:]
    static var examples: [String] = []
    static func mismatch(_ area: String, _ text: String) {
        mismatches[area, default: 0] += 1
        if mismatches[area]! <= (area == "filename-time" ? 300 : area == "input-selection" ? 1500 : 25) { examples.append(area + ": " + text) }
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
            let core = PhotoTimeRepair.inferredTimes(filenames: names, zone: zone)
            if core.count != names.count { mismatch("filename-time", "\(core.count) answers for \(names.count) names") }
            for (index, (name, value)) in zip(names, core).enumerated() {
                let result = describe(value)
                // The single-name entry point answers like the batch one.
                if index % 50 == 0, describe(PhotoTimeRepair.inferredTime(filename: name, zone: zone)) != result {
                    mismatch("filename-time", "\(zone.identifier) \(name.debugDescription): single and batch answers differ")
                }
                if result != "nil" { found += 1 }
                digest.add(zone.identifier + "|" + name + "|" + result)
            }
        }
        print("filename-time: \(digest.count) name×zone cases (\(names.count) names, \(zones.count) zones, \(found) inferred), digest \(digest.value)")
        golden("filename-time", digest.value)
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
            if core.count != chosen.count { mismatch("action-blocking-reason", "settings#\(index): \(core.count) answers for \(chosen.count) items") }
            for (item, coreValue) in zip(chosen, core) {
                compared += 1
                if coreValue != nil { blocked += 1 }
                digest.add("\(index)|\(item.name)|\(coreValue ?? "nil")")
            }
        }
        print("action-blocking-reason: \(compared) item×settings cases (\(items.count) items, \(settingsList.count) settings, \(blocked) blocked), digest \(digest.value)")
        golden("action-blocking-reason", digest.value)
        return digest.value
    }

    // MARK: queue-state

    /// Every scan-row fact the queue state reads (`initialState`, `isConvertible`,
    /// `ScanRow.isStaticHDR`), each under every result state and cover label, plus a
    /// cross of cover choices; all of it under every direction, missing-time policy and
    /// export mode.
    static func queueCorpus() throws -> (entries: [(row: ScanRow, allowsStaticHDR: Bool, resultState: PhotoState?)],
                                         settings: [ConversionSettings]) {
        var labels: [(String, String?)] = []
        for (label, code) in [("HEIC（原样保留）","heic_preserved"),("Ultra HDR","ultra_hdr"),("Ultra HDR · 保留","ultra_hdr_preserved"),
                              ("荣耀私有 HDR","honor_private"),("ISO HDR","iso_hdr"),("旧 Apple HDR","apple_legacy"),("HEIC HDR","heic_hdr"),
                              ("OPPO UHDR（实验）","oppo_uhdr"),("Apple HDR","apple_hdr"),("SDR / 其他","sdr"),("SDR","sdr"),
                              ("原始图像","sdr"),("原始 JPEG","sdr"),("—","none")] {
            labels.append((label, code)); labels.append((label, nil))
        }
        // Codes the table does not know, codes that disagree with the label, near-miss labels.
        labels += [("某 HDR 变体", "other"), ("某 HDR 变体", nil), ("未知", "other"), ("", nil), ("Ultra HDR", "other"),
                   ("Ultra HDR", "bogus"), ("SDR", "ultra_hdr"), ("荣耀私有 HDR", "ULTRA_HDR"), ("ultra hdr", nil), ("Ultra HDR ", nil)]
        let decoder = JSONDecoder()
        func frame(_ supported: Bool) throws -> BurstFrame {
            try decoder.decode(BurstFrame.self, from: Data(#"{"group_id":"g","source_group":"s","index":0,"source_cover":false,"source_sha256":null,"supported":\#(supported),"reason":""}"#.utf8))
        }
        let bursts: [BurstFrame?] = [nil, try frame(true), try frame(false)]
        let results: [PhotoState?] = [nil] + PhotoState.allCases
        var entries: [(row: ScanRow, allowsStaticHDR: Bool, resultState: PhotoState?)] = []
        var manualCovers: [String:ManualCoverChoice] = [:], midpoints: [String:Bool] = [:], coverPaths: [String] = []
        var serial = 0
        func add(_ label: (String, String?), motion: Bool, burst: BurstFrame?, confirmed: Bool, allows: Bool, cover: String,
                 result: PhotoState?, manual: Bool, midpoint: Bool?, selected: Bool) {
            serial += 1
            let url = URL(fileURLWithPath: "/tmp/momentstays-queue-parity/item-\(serial).jpg")
            var row = ScanRow(source: url, relative: url.lastPathComponent, isMotion: motion, hdr: label.0, cover: cover,
                              detail: "parity", confirmedStatic: confirmed)
            row.burst = burst; row.hdrKind = label.1
            entries.append((row, allows, result))
            if manual { manualCovers[url.path] = ManualCoverChoice(source_sha256: "s", source_version: "v", track_index: 0, frame_index: 1) }
            if let midpoint = midpoint { midpoints[url.path] = midpoint }
            if selected { coverPaths.append(NativeBatch.normalized(url).path) }
        }
        // Part 1: every row fact × result state × the cover labels the category tests.
        for label in labels { for motion in [false, true] { for burst in bursts { for confirmed in [false, true] { for allows in [false, true] {
            for cover in ["0.4 秒", "未确定（需选帧）", "未记录"] { for result in results {
                add(label, motion: motion, burst: burst, confirmed: confirmed, allows: allows, cover: cover, result: result,
                    manual: false, midpoint: nil, selected: false)
            } }
        } } } } }
        // Part 2: representative rows × more cover labels × every cover choice.
        let some: [(String, String?)] = [("Ultra HDR","ultra_hdr"),("Ultra HDR",nil),("SDR","sdr"),("—",nil),("Ultra HDR","bogus"),("OPPO UHDR（实验）","oppo_uhdr")]
        for label in some { for motion in [false, true] { for burst in bursts { for confirmed in [false, true] { for allows in [false, true] {
            for cover in ["0.4 秒", "未确定（需选帧）", "未记录", "", "—", "未记录 "] { for result in results {
                for manual in [false, true] { for midpoint in [nil, false, true] as [Bool?] { for selected in [false, true] {
                    add(label, motion: motion, burst: burst, confirmed: confirmed, allows: allows, cover: cover, result: result,
                        manual: manual, midpoint: midpoint, selected: selected)
                } } }
            } }
        } } } } }
        var settingsList: [ConversionSettings] = []
        for direction in ConversionDirection.allCases { for missing in MissingTimePolicy.allCases { for mode in ExportMode.allCases {
            var s = ConversionSettings()
            s.direction = direction; s.missingTime = missing; s.exportMode = mode
            s.manualCovers = manualCovers; s.manualMidpoints = midpoints
            s.coverSelections = Dictionary(uniqueKeysWithValues: coverPaths.map { ($0, 1) })
            settingsList.append(s)
        } } }
        // And without any cover choice recorded.
        var plain = ConversionSettings(); plain.direction = .toApple
        settingsList.append(plain)
        return (entries, settingsList)
    }

    static func queueLine(_ index: Int, _ name: String, _ v: CoreQueueState) -> String {
        "\(index)|\(name)|\(v.isStaticHDR)|\(v.isConvertible)|\(v.initialState.rawValue)|\(v.category.rawValue)"
    }

    static func queueState() throws -> String {
        let (entries, settingsList) = try queueCorpus()
        var items = PhotoQueueItem.make(entries.map { (row: $0.row, allowsStaticHDR: $0.allowsStaticHDR) })
        for index in items.indices { items[index].resultState = entries[index].resultState }
        if items.count != entries.count { mismatch("queue-state", "\(items.count) items for \(entries.count) rows") }
        // The single-row initializer answers like the batch one.
        for index in stride(from: 0, to: items.count, by: 97) {
            let one = PhotoQueueItem(entries[index].row, allowsStaticHDR: entries[index].allowsStaticHDR)
            if (one.initialState, one.isConvertible, one.isStaticHDR) != (items[index].initialState, items[index].isConvertible, items[index].isStaticHDR) {
                mismatch("queue-state", "\(items[index].name): single and batch initializers differ")
            }
        }
        let digest = Digest()
        for (index, settings) in settingsList.enumerated() {
            let categories = PhotoQueueItem.categories(items, settings: settings)
            if categories.count != items.count { mismatch("queue-state", "settings#\(index): \(categories.count) answers for \(items.count) items"); continue }
            for (offset, (item, category)) in zip(items, categories).enumerated() {
                digest.add(queueLine(index, item.name, CoreQueueState(isStaticHDR: item.isStaticHDR, isConvertible: item.isConvertible,
                                                                      initialState: item.initialState, category: category)))
                // The single-item call answers like the batch one.
                if offset % 997 == 0, item.category(settings: settings) != category {
                    mismatch("queue-state", "settings#\(index) \(item.name): single and batch answers differ")
                }
            }
        }
        print("queue-state: \(digest.count) item×settings cases (\(items.count) items, \(settingsList.count) settings), digest \(digest.value)")
        golden("queue-state", digest.value)
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
            let core = result { try AndroidExport.selection([url]).entries }
            print("  live-selection \(name): \(core)")
            digest.add(name + "|" + core)
        }
        print("live-selection: \(digest.count) cases, digest \(digest.value)")
        golden("live-selection", digest.value)
        return digest.value
    }

    // MARK: input-selection

    /// The file system the input selection reads, under the parity root (a
    /// temporary folder reached through the /var → /private/var firmlink):
    /// nested folders; every photo extension spelling and its near misses;
    /// hidden entries (dot names and the hidden flag, files and folders);
    /// packages (by extension, upper-case extension, Finder bundle bit) and a
    /// folder named like a photo; symlinks to a file, a folder, nowhere, and a
    /// loop; hard links; a FIFO; an unreadable folder and an unreadable photo;
    /// names that differ only in case or Unicode normalization (one file on
    /// APFS); names starting with a combining mark; CJK, fullwidth, emoji,
    /// bidi-mark and ligature names; empty and photo-less folders.
    struct SelectionCorpus {
        var root: URL
        var inputs: [URL] = []
        var movies: [URL] = []
        var outputs: [URL] = []
        var names: [String] = []
        var locked: [URL] = []
    }

    static func selectionCorpus(root: URL) throws -> SelectionCorpus {
        let fm = FileManager.default
        let sel = root.appendingPathComponent("sel", isDirectory: true)
        var corpus = SelectionCorpus(root: sel)
        func url(_ rel: String) -> URL { URL(fileURLWithPath: sel.path + "/" + rel) }
        func folder(_ rel: String) throws { try fm.createDirectory(atPath: sel.path + "/" + rel, withIntermediateDirectories: true) }
        func touch(_ rel: String) throws {
            let path = sel.path + "/" + rel
            try fm.createDirectory(atPath: (path as NSString).deletingLastPathComponent, withIntermediateDirectories: true)
            if !fm.createFile(atPath: path, contents: Data("x".utf8)) { throw Failure("cannot create \(rel)") }
        }
        func hide(_ rel: String) throws {
            var values = URLResourceValues(); values.isHidden = true
            var target = url(rel); try target.setResourceValues(values)
        }
        let names = ["IMG_1.jpg", "IMG_2.JPG", "IMG_10.jpeg", "IMG_010.JPEG", "img_3.heic", "IMG_4.HEIC", "IMG_5.heif", "IMG_6.HeIf",
                     "IMG 7.jpg", "IMG-8.jpg", "IMG_9.jpg.png", "IMG_11.png", "IMG_12.mov", "IMG_13.mp4", "notes.txt", "noext", "a.",
                     "..jpg", "...jpg", "a..jpg", "a.jpg.", "a. jpg", "a.jp g", "a.jpg ", "a.tar.jpg", "\u{E9}.jpg", "e\u{301}2.jpg",
                     "照片1.jpg", "照片10.jpg", "照片2.jpg", "ＩＭＧ１.jpg", "ＩＭＧ２.JPG", "📷.jpg", "\u{301}lead.jpg", "x\u{200F}.jpg",
                     "a.jpg\u{200E}", "\u{C4}.jpg", "a\u{308}2.jpg", "Zebra.jpg", "apple.jpg", "_under.jpg", "-dash.jpg", "1.jpg", "01.jpg",
                     "001.jpg", "2.jpg", "10.jpg", "a1b2.jpg", "a1b10.jpg", "A1B3.jpg", "\u{DF}.jpg", "ss.jpg", "\u{130}.jpg", "i.jpg",
                     "\u{1C5}.jpg", "\u{FB01}.jpg", "fi.jpg", "x.jpg.jpg", "x.JPG.heic", "x.jpg~", "~x.jpg", "#x.jpg", "x%20y.jpg", "x:y.jpg"]
        for name in names { try touch("A/" + name) }
        try touch("A/sub/IMG_1.jpg"); try touch("A/sub/deep/x.jpg"); try touch("A/sub/deep/Y.HEIC")
        try touch("A/.hidden/h.jpg"); try touch("A/.h.jpg"); try touch("A/flagged.jpg"); try hide("A/flagged.jpg")
        try touch("A/flaggeddir/f.jpg"); try hide("A/flaggeddir")
        for package in ["P.app", "Q.APP", "P.bundle", "P.photoslibrary", "P.rtfd", "P.lproj", "P.xcodeproj", "P.key", "P.pages",
                        "P.framework", "P.plugin", "P.kext", "P.xcassets", "P.playground", "folder.jpg"] {
            try touch("A/" + package + "/p.jpg")
        }
        try touch("A/bundlebit/b.jpg")
        var info = [UInt8](repeating: 0, count: 32); info[8] = 0x20 // kHasBundle
        _ = setxattr(url("A/bundlebit").path, "com.apple.FinderInfo", info, info.count, 0, 0)
        try touch("A/\u{301}sub/m.jpg"); try touch("A/sub\u{301}/n.jpg")
        try touch("B/b1.jpg"); try touch("B/b2.heic"); try touch("B/sub/b3.jpg")
        try fm.createSymbolicLink(atPath: url("A/link.jpg").path, withDestinationPath: "IMG_1.jpg")
        try fm.createSymbolicLink(atPath: url("A/abslink.jpg").path, withDestinationPath: url("B/b1.jpg").path)
        try fm.createSymbolicLink(atPath: url("A/dirlink").path, withDestinationPath: url("B").path)
        try fm.createSymbolicLink(atPath: url("A/dead.jpg").path, withDestinationPath: "nowhere.jpg")
        try fm.createSymbolicLink(atPath: url("A/loop1.jpg").path, withDestinationPath: "loop2.jpg")
        try fm.createSymbolicLink(atPath: url("A/loop2.jpg").path, withDestinationPath: "loop1.jpg")
        try fm.linkItem(atPath: url("A/IMG_1.jpg").path, toPath: url("A/hard.jpg").path)
        try fm.linkItem(atPath: url("A/IMG_2.JPG").path, toPath: url("B/hardB.jpg").path)
        _ = mkfifo(url("A/pipe.jpg").path, 0o644)
        try touch("NF/\u{E9}.jpg"); try touch("NF/e\u{301}.jpg")            // one file on APFS
        try touch("NF/e\u{301}x.jpg"); try touch("CASE/Case.jpg"); try touch("CASE/case.JPG")
        try touch("U/ok.jpg"); try touch("U/locked/x.jpg"); try touch("U/noread.jpg")
        try touch("D/1/2/3/4/5/6/d.jpg"); try folder("E"); try touch("N/n.txt"); try touch("Ax/ax.jpg")
        try fm.setAttributes([.posixPermissions: 0], ofItemAtPath: url("U/noread.jpg").path)
        try fm.setAttributes([.posixPermissions: 0], ofItemAtPath: url("U/locked").path)
        corpus.locked = [url("U/locked"), url("U/noread.jpg")]

        let p = sel.path
        func dirs(_ path: String) -> [URL] { [URL(fileURLWithPath: path, isDirectory: true), URL(fileURLWithPath: path, isDirectory: false)] }
        var inputs: [URL] = []
        for rel in ["A", "B", "D", "E", "N", "U", "U/locked", "Ax", "A/sub", "A/P.app", "A/folder.jpg", "A/dirlink", "A/.hidden",
                    "A/flaggeddir", "A/\u{301}sub", "A/sub\u{301}", "NF", "CASE", "A/missingdir"] { inputs += dirs(p + "/" + rel) }
        inputs += dirs(p)
        for rel in ["A/IMG_1.jpg", "A/IMG_2.JPG", "A/link.jpg", "A/abslink.jpg", "A/dead.jpg", "A/loop1.jpg", "A/hard.jpg", "A/pipe.jpg",
                    "A/notes.txt", "A/..jpg", "A/a.jpg.", "A/IMG_11.png", "A/P.app/p.jpg", "A/.h.jpg", "A/flagged.jpg", "A/\u{301}lead.jpg",
                    "A/\u{301}sub/m.jpg", "B/b1.jpg", "B/hardB.jpg", "U/noread.jpg", "U/locked/x.jpg", "A/missing.jpg", "A/dirlink/b1.jpg",
                    "A/sub/deep/Y.HEIC", "NF/\u{E9}.jpg", "NF/e\u{301}.jpg", "NF/e\u{301}x.jpg", "NF/\u{E9}x.jpg", "CASE/CASE.JPG",
                    "A/IMG_12.mov"] {
            inputs.append(URL(fileURLWithPath: p + "/" + rel))
        }
        for path in ["/private" + p + "/A/IMG_1.jpg", p + "/A/./IMG_1.jpg", p + "/A/sub/../IMG_1.jpg", p + "//A//IMG_1.jpg",
                     p.replacingOccurrences(of: "/sel", with: "/SEL") + "/a/img_1.JPG", p + "/A/nothere/../IMG_2.JPG"] {
            inputs.append(URL(fileURLWithPath: path))
        }
        inputs += dirs("/private" + p + "/B") + dirs(p + "/A/sub/..") + dirs(p.replacingOccurrences(of: "/sel", with: "/SEL") + "/b")
        corpus.inputs = inputs
        corpus.movies = [root.appendingPathComponent("pair/IMG_0001.mov"), root.appendingPathComponent("pair/other.MOV"),
                         root.appendingPathComponent("duplicate/a.mov"), root.appendingPathComponent("missing-still/a.mov"),
                         root.appendingPathComponent("upper-and-mp4/IMG_0002.mp4"), root.appendingPathComponent("symlink/a.mov"),
                         root.appendingPathComponent("nothere/a.mov")]
        var outputs: [URL] = []
        for rel in ["A", "A/sub", "A/newout", "A/P.app", "B", "B/sub", "Ax", "A/\u{301}sub", "A/sub\u{301}", "A/dirlink", "E", "U/locked"] {
            outputs += dirs(p + "/" + rel)
        }
        outputs += dirs(p) + dirs("/private" + p + "/A") + dirs(p + "/A/../A") + dirs(p.replacingOccurrences(of: "/sel", with: "/SEL") + "/a")
        outputs += dirs("/") + dirs("/nonexistent/out") + dirs(root.path) + dirs(p + "/A/IMG_1.jpg")
        corpus.outputs = outputs
        corpus.names = names + ["IMG_1.jpg", "img_1.jpg", "IMG_01.jpg", "IMG_1.JPG", "IMG1.jpg", "IMG_1 .jpg", "IMG_1-.jpg", "IMG_1_.jpg",
                                "IMG_1(1).jpg", "IMG_1 (2).jpg", "IMG_1 (10).jpg", "Ab", "aB", "ab", "AB", "a b", "a-b", "a_b", "a.b",
                                "résumé", "resume", "Resume", "re\u{301}sume\u{301}", "１０", "10", "９", "9", "٣", "3", "三", "〇",
                                "a\u{0}b", "", " ", "\t", "Ⅻ", "12", "½", "0.5", "x²", "x2", "Ω", "ω", "Ω", "ǆ", "dž", "ﬀ", "ff"]
        return corpus
    }

    static func selectionText(_ text: String, _ corpus: SelectionCorpus) -> String {
        let p = corpus.root.path
        let parent = (p as NSString).deletingLastPathComponent
        return text.replacingOccurrences(of: "/private" + p, with: "<private-sel>").replacingOccurrences(of: p, with: "<sel>")
            .replacingOccurrences(of: "/private" + parent, with: "<private-root>").replacingOccurrences(of: parent, with: "<root>")
            .replacingOccurrences(of: p.replacingOccurrences(of: "/sel", with: "/SEL"), with: "<SEL>")
    }

    static func showURL(_ url: URL, _ corpus: SelectionCorpus) -> String {
        selectionText(url.path, corpus) + (url.hasDirectoryPath ? "/" : "")
    }

    static func selectionOutcome(_ corpus: SelectionCorpus, _ work: () throws -> (base: URL, entries: [URL])) -> String {
        do {
            let r = try work()
            return "ok base=" + showURL(r.base, corpus) + " entries=" + r.entries.map { showURL($0, corpus) }.joined(separator: ",")
        } catch { return "error:" + selectionText(error.localizedDescription, corpus) }
    }

    static func inputSelection(root: URL) throws -> String {
        let corpus = try selectionCorpus(root: root)
        defer {
            for url in corpus.locked { try? FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: url.path) }
        }
        let legacyDigest = Digest(), coreDigest = Digest()
        var counts: [String: Int] = [:], kindMismatches: [String: Int] = [:]
        func check(_ kind: String, _ label: String, legacy: String, core: String) {
            counts[kind, default: 0] += 1
            legacyDigest.add(kind + "|" + label + "|" + legacy); coreDigest.add(kind + "|" + label + "|" + core)
            if legacy != core {
                kindMismatches[kind, default: 0] += 1
                mismatch("input-selection", "\(kind) \(label): swift \(legacy) core \(core)")
            }
        }
        let inputs = corpus.inputs
        func label(_ urls: [URL]) -> String { urls.map { showURL($0, corpus) }.joined(separator: " + ") }
        // normalized (the core's own copy of NativeBatch.normalized, which the decisions use)
        let normalizedCandidates = inputs + corpus.outputs + corpus.movies +
            ["/private/var", "/private/tmp", "/tmp", "/var/automount/x", "/private/var/automount/x", "/private/etc/hosts", "/etc",
             "/private/nonexistent", "/Users/../private/var", "/", "/.."].flatMap { [URL(fileURLWithPath: $0, isDirectory: true), URL(fileURLWithPath: $0, isDirectory: false)] }
        let normalizedCore = (try? RustCore.engine(["mode":"normalized-paths","inputs":normalizedCandidates.map { CoreInputSelection.json($0) }]))?["paths"] as? [[String:Any]] ?? []
        for (index, url) in normalizedCandidates.enumerated() {
            let legacy = NativeBatch.normalized(url)
            let core = index < normalizedCore.count ? normalizedCore[index] : [:]
            let coreText = selectionText(core["path"] as? String ?? "<missing>", corpus) + ((core["dir"] as? Bool ?? false) ? "/" : "")
            check("normalized", label([url]), legacy: showURL(legacy, corpus), core: coreText)
        }
        // Finder ordering
        let names = corpus.names
        let legacyOrder = names.sorted { $0.localizedStandardCompare($1) == .orderedAscending }
        let coreOrder = (try? RustCore.engine(["mode":"standard-order","names":names]))?["names"] as? [String] ?? []
        check("order", "\(names.count) names", legacy: legacyOrder.joined(separator: "|"), core: coreOrder.joined(separator: "|"))
        for i in 0..<names.count { for j in (i + 1)..<names.count {
            let a = names[i], b = names[j]
            // A stable sort keeps an equal pair in place, so "=" reads as "<".
            let legacy = a.localizedStandardCompare(b) == .orderedDescending ? ">" : "<"
            let pair = (try? RustCore.engine(["mode":"standard-order","names":[a, b]]))?["names"] as? [String] ?? []
            check("compare", a.debugDescription + " " + b.debugDescription, legacy: legacy, core: pair == [a, b] ? "<" : ">")
        } }
        // files
        for url in inputs {
            func run(_ work: () throws -> [URL]) -> String {
                do { return "ok " + (try work()).map { showURL($0, corpus) }.joined(separator: ",") }
                catch { return "error:" + selectionText(error.localizedDescription, corpus) }
            }
            check("files", label([url]), legacy: run { try NativeBatch.files(url) }, core: run { try CoreInputSelection.files(url) })
        }
        // selection: every input, every ordered pair, and random triples
        var sets: [[URL]] = [[]] + inputs.map { [$0] }
        for a in inputs { for b in inputs { sets.append([a, b]) } }
        var rng = Rng(state: 0x5e1e_c710)
        for _ in 0..<1500 { sets.append([rng.pick(inputs), rng.pick(inputs), rng.pick(inputs)]) }
        for set in sets {
            check("selection", label(set), legacy: selectionOutcome(corpus) { try NativeBatch.selection(set) },
                  core: selectionOutcome(corpus) { try CoreInputSelection.selection(set) })
        }
        // Android export selection: movies stand for their stills
        let androidPool = corpus.movies + [root.appendingPathComponent("pair"), root.appendingPathComponent("pair/IMG_0001.jpg"),
                                           root.appendingPathComponent("pair/notes.txt")] + Array(inputs.prefix(12)) + Array(inputs.suffix(8))
        var androidSets: [[URL]] = [[]] + androidPool.map { [$0] }
        for a in androidPool { for b in androidPool { androidSets.append([a, b]) } }
        for _ in 0..<300 { androidSets.append([rng.pick(androidPool), rng.pick(androidPool), rng.pick(androidPool)]) }
        for set in androidSets {
            check("android-selection", label(set), legacy: selectionOutcome(corpus) { try AndroidExport.selection(set) },
                  core: selectionOutcome(corpus) { try CoreInputSelection.selection(set, android: true) })
        }
        // output validation: every input and 300 random pairs, against every output
        var sourceSets: [[URL]] = [[]] + inputs.map { [$0] }
        for _ in 0..<300 { sourceSets.append([rng.pick(inputs), rng.pick(inputs)]) }
        for sources in sourceSets { for output in corpus.outputs {
            func run(_ work: () throws -> Void) -> String {
                do { try work(); return "ok" } catch { return "error:" + selectionText(error.localizedDescription, corpus) }
            }
            check("validate-output", label(sources) + " -> " + showURL(output, corpus),
                  legacy: run { try NativeBatch.validateOutput(sources: sources, output: output) },
                  core: run { try CoreInputSelection.validateOutput(sources: sources, output: output) })
        } }
        let total = counts.values.reduce(0, +)
        print("input-selection: \(total) cases \(counts.sorted { $0.key < $1.key }.map { "\($0.key)=\($0.value)" }.joined(separator: " ")), " +
              "legacy Swift digest \(legacyDigest.value), core digest \(coreDigest.value), mismatches \(kindMismatches.sorted { $0.key < $1.key })")
        let value = coreDigest.value
        golden("input-selection", value)
        return value
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
            digests["input-selection"] = try inputSelection(root: root)
            digests["queue-state"] = try queueState()
            let tracks = try trackParity(root: root, repository: repository)
            for line in examples { print("MISMATCH " + line) }
            let data = try JSONSerialization.data(withJSONObject: ["digests": digests, "mismatches": mismatches,
                "track_info": ["tracks": tracks.cases, "identical": tracks.equal]], options: [.sortedKeys])
            print("SUMMARY " + String(decoding: data, as: UTF8.self))
            let failed = mismatches.filter { $0.value > 0 }
            if !failed.isEmpty { fputs("FAIL: core decision parity mismatches \(failed)\n", stderr); exit(1) }
            print("PASS core decision parity (filename-time, action-blocking-reason, live-selection, input-selection, queue-state)")
        } catch {
            fputs("FAIL: \(error.localizedDescription)\n", stderr); exit(1)
        }
    }
}

/// Per-track sample facts read from the MP4/MOV sample tables by the shared
/// Rust core (`track-info`, core/src/engine/decisions/track_info.rs). Test-only:
/// the app keeps AVAssetReader (MotionLive.swift `trackInfo`) until the
/// timelines agree on audio and timed-metadata tracks too.
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
