import Foundation

// Value types deliberately contain no AppKit/UIKit. Each job receives a snapshot.
protocol SettingsChoice: RawRepresentable, CaseIterable, Hashable, Codable where RawValue == String {
    var title: String { get }
}

enum ConversionDirection: String, SettingsChoice {
    case toApple = "to-apple", toAndroid = "to-android", androidToAndroid = "android-to-android", honorToGoogle = "honor-to-google"
    var title: String { switch self { case .toApple: return "受支持照片 → Apple"; case .toAndroid: return "苹果实况 → 安卓"; case .androidToAndroid: return "Google → 荣耀（仅 JPEG）"; case .honorToGoogle: return "荣耀 → Google（仅 JPEG）" } }
    var exportsAndroid: Bool { CoreConversionSettings.tables.direction(rawValue).exportsAndroid }
}

enum AndroidVendor: String, SettingsChoice {
    case google, xiaomi, samsung, huawei, honor, oppo, vivo
    var title: String {
        switch self {
        case .google: return "Google / 通用（新款荣耀推荐）"
        case .xiaomi: return "小米"
        case .samsung: return "三星"
        case .huawei: return "华为"
        case .honor: return "荣耀（备用）"
        case .oppo: return "OPPO"
        case .vivo: return "vivo V2"
        }
    }
    var supportsHEIC: Bool { CoreConversionSettings.tables.vendor(rawValue).supportsHEIC }
}

// Destination presets describe the actual supported format route. Keep the
// engine directions internal and retain their stored values for old tasks.
enum MigrationTargetPreset: String, SettingsChoice {
    case apple, google, xiaomi, samsung, huawei, honor, oppo, vivo, googleToHonor, honorToGoogle
    var title: String {
        switch self {
        case .apple: return "受支持照片 → Apple（实况 / HDR / 连拍）"
        case .google: return "苹果实况 → Google / 通用"
        case .xiaomi: return "苹果实况 → 小米"
        case .samsung: return "苹果实况 → 三星"
        case .huawei: return "苹果实况 → 华为"
        case .honor: return "苹果实况 → 荣耀（备用）"
        case .oppo: return "苹果实况 → OPPO"
        case .vivo: return "苹果实况 → vivo V2"
        case .googleToHonor: return "Google → 荣耀（JPEG）"
        case .honorToGoogle: return "荣耀 → Google（JPEG）"
        }
    }
    var direction: ConversionDirection { CoreConversionSettings.tables.preset(rawValue).direction }
    var vendor: AndroidVendor { CoreConversionSettings.tables.preset(rawValue).vendor }
    var detail: String {
        switch self {
        case .apple: return "受支持实况、静态 HDR 与荣耀/华为同格式 JPEG 连拍 → Apple；实验支持 Google 原始文件名及三星 SEF JPEG 连拍。"
        case .googleToHonor: return "Google → 荣耀：仅支持已验证的 Google Motion Photo JPEG；不支持 HEIC。安卓互转仅此方向和荣耀 → Google。"
        case .honorToGoogle: return "荣耀 → Google：仅支持已识别荣耀实况 JPEG；静态 HDR 独立输出单图，具体支持以照片检查结果为准；不支持 HEIC。"
        default: return "\(title)；\(vendor.supportsHEIC ? "JPEG / HEIC" : "仅 JPEG")。本地静态 HDR 独立处理，具体支持以照片检查结果为准。"
        }
    }
    static func current(_ settings: ConversionSettings) -> Self {
        CoreConversionSettings.tables.currentPreset(settings.direction, settings.vendor)
    }
}

enum UnconvertedExportChoice: String, SettingsChoice {
    case none, sameLocation, separateLocation
    var title: String {
        switch self {
        case .none: return "否"
        case .sameLocation: return "导出到相同位置"
        case .separateLocation: return "导出到不同位置"
        }
    }
}

enum PhotoContainer: String, SettingsChoice {
    case jpeg, heic
    var title: String { self == .jpeg ? "JPEG" : "HEIC" }
}

enum VideoTrack: String, SettingsChoice {
    case primary, secondary
    var title: String { self == .primary ? "动态视频" : "辅助视频" }
}

enum ExportMode: String, SettingsChoice {
    case livePhoto = "livephoto", withAuxiliary = "livephoto+auxiliary", auxiliaryOnly = "auxiliary-only", withCandidates = "livephoto+candidates"
    var title: String {
        switch self {
        case .livePhoto: return "Live Photo"
        case .withAuxiliary: return "Live Photo ＋ 辅助视频"
        case .auxiliaryOnly: return "仅辅助视频"
        case .withCandidates: return "Live Photo ＋ 高清封面"
        }
    }
}

enum MissingTimePolicy: String, SettingsChoice {
    case error, midpoint
    var title: String { self == .error ? "报告错误（推荐）" : "使用视频中点" }
}

enum CandidateDelivery: String, SettingsChoice {
    case staticPhotos = "static", livePhotos = "livephoto"
    var title: String { self == .livePhotos ? "每张都导出为 Live Photo" : "主照片实况，其余静态照片" }
}

enum CoverTimeDomain: String, SettingsChoice {
    case auto, media, presentation
    var title: String {
        switch self {
        case .auto: return "自动检查（推荐）"
        case .media: return "视频样本时间（media）"
        case .presentation: return "播放时间（presentation）"
        }
    }
}

enum OutputDestination: String, SettingsChoice {
    case ask, folder, photos, folderAndPhotos, share, folderAndShare
    var title: String {
        switch self {
        case .ask: return "询问位置"
        case .folder: return "文件夹"
        case .photos: return "相册"
        case .folderAndPhotos: return "相册和文件夹"
        case .share: return "共享菜单"
        case .folderAndShare: return "文件夹和共享菜单"
        }
    }
    var usesCache: Bool { CoreConversionSettings.tables.destination(rawValue).usesCache }
    var importsPhotos: Bool { CoreConversionSettings.tables.destination(rawValue).importsPhotos }
    var shares: Bool { CoreConversionSettings.tables.destination(rawValue).shares }
}

enum LibraryVersion: String, SettingsChoice {
    case original, edited
    var title: String { self == .original ? "未修改原片（保留原始 HDR）" : "当前编辑版（可能无 HDR）" }
}

// Defaults, constraints between options, the engine options and the offered
// destinations are decided by the shared Rust core
// (core/src/engine/decisions/settings.rs); this struct stores the values and
// keeps their Codable form (UserDefaults and task histories).
struct ConversionSettings: Equatable, Codable {
    var direction: ConversionDirection = CoreConversionSettings.defaultChoice("direction")
    var destination: OutputDestination = CoreConversionSettings.defaultChoice("destination")
    var outputConfirmed: Bool?
    var albumTitle: String = CoreConversionSettings.defaultValue("album_title")
    var albumID: String?
    var libraryVersion: LibraryVersion = .original
    var vendor: AndroidVendor = CoreConversionSettings.defaultChoice("vendor")
    var container: PhotoContainer = CoreConversionSettings.defaultChoice("container")
    var imageQuality: Double = CoreConversionSettings.defaultValue("image_quality")
    var gainQuality: Double = CoreConversionSettings.defaultValue("gain_quality")
    var preserveJPEG: Bool = CoreConversionSettings.defaultValue("preserve_jpeg")
    var liveConversion: Bool?
    var burstConversion: Bool?
    var timeOptionsEnabled: Bool?
    var legacyHDR: Bool = CoreConversionSettings.defaultValue("legacy_hdr")
    // Presentation only: retain the stored preference even when this output cannot use it.
    var privateHDRExperimentAvailable: Bool { CoreConversionSettings.decide(self).privateHDRAvailable }
    var privateHDRExperimentEnabled: Bool { CoreConversionSettings.decide(self).privateHDREnabled }
    var exportDataFiles: Bool = CoreConversionSettings.defaultValue("export_data_files")
    var track: VideoTrack = CoreConversionSettings.defaultChoice("track")
    var exportMode: ExportMode = CoreConversionSettings.defaultChoice("export_mode")
    var missingTime: MissingTimePolicy = CoreConversionSettings.defaultChoice("missing_time")
    var timeDomain: CoverTimeDomain = CoreConversionSettings.defaultChoice("time_domain")
    var repairAuxiliaryDuration: Bool = CoreConversionSettings.defaultValue("repair_auxiliary_duration")
    // Optional for compatibility with task histories saved before this switch.
    var exifFileTimes: Bool?
    var photoTimeRepair: Bool?
    var photoTimeSynthetic: Bool?
    var photoTimeZone: String?
    var photoTimeReference: Double?
    // Optional for compatibility with task histories saved before cover selection.
    var coverSelections: [String:Int]?
    var manualMidpoints: [String:Bool]?
    var manualCovers: [String:ManualCoverChoice]?
    var candidateDelivery: CandidateDelivery?
    var burstChoices: [String:BurstChoice]?

    func coverIndex(for source: URL) -> Int? { direction == .toApple ? coverSelections?[NativeBatch.normalized(source).path] : nil }

    mutating func normalize() {
        let n = CoreConversionSettings.decide(self).normalized
        destination = CoreConversionSettings.choice(n["destination"])
        track = CoreConversionSettings.choice(n["track"])
        albumTitle = n["album_title"] as! String
        container = CoreConversionSettings.choice(n["container"])
        imageQuality = (n["image_quality"] as! NSNumber).doubleValue
        gainQuality = (n["gain_quality"] as! NSNumber).doubleValue
    }

    var options: ConversionOptions {
        let d = CoreConversionSettings.decide(self).options
        var result = ConversionOptions()
        result.direction = d["direction"] as! String
        result.legacyHDR = d["legacy_hdr"] as! Bool
        result.missingTime = d["missing_time"] as! String
        result.timeDomain = d["time_domain"] as! String
        result.videoTrack = d["video_track"] as! String
        result.outputMode = d["output_mode"] as! String
        result.repairAuxiliaryDuration = d["repair_auxiliary_duration"] as! Bool
        result.exportDataFiles = d["export_data_files"] as! Bool
        result.exifFileTimes = d["exif_file_times"] as! Bool
        result.photoTime = d["photo_time"] as? [String:Any]
        result.candidateDelivery = d["candidate_delivery"] as! String
        if let android = d["android"] as? [String:Any] {
            // The qualities are the stored values, passed through unchanged.
            result.android = AndroidOptions(vendor: android["vendor"] as! String, container: android["container"] as! String,
                hdr: android["hdr"] as! String, imageQuality: imageQuality, gainQuality: gainQuality,
                preserveJPEG: android["preserve_jpeg"] as! Bool)
        }
        let carry = d["carry"] as! [String:Bool]
        if carry["cover_selections"] == true { result.coverSelections = coverSelections ?? [:] }
        if carry["manual_covers"] == true { result.manualCovers = manualCovers ?? [:] }
        if carry["burst_choices"] == true { result.burstChoices = burstChoices ?? [:] }
        return result
    }

    var summary: String {
        if direction == .androidToAndroid { return "Google → 荣耀 · JPEG 原编码" }
        if direction == .honorToGoogle { return "荣耀 → Google · JPEG" }
        if direction == .toAndroid { return "苹果实况 → \(vendor.title) · \(container == .heic ? "HEIC 原编码" : "JPEG")" }
        var parts = [direction.title,track.title,exportMode.title]
        if exportMode == .withCandidates { parts.append((candidateDelivery ?? .staticPhotos).title) }
        if !(coverSelections ?? [:]).isEmpty && exportMode != .auxiliaryOnly { parts.append("已选择高清候选封面") }
        return parts.joined(separator:" · ")
    }

    var destinations: [OutputDestination] { CoreConversionSettings.decide(self).destinations }
    var actionTitle: String {
        switch CoreConversionSettings.decide(self).action {
        case "import-photos": return "转换并导入图库"
        case "share": return "转换并共享"
        case "export-android": return "导出到安卓"
        case "export-auxiliary": return "导出辅助轨"
        default: return "开始转换"
        }
    }
}

/// Bridge to the conversion settings model of the shared Rust core (request
/// modes `conversion-settings-tables` and `conversion-settings`). The core
/// sees the scalar settings; the per-source maps stay here and the core says
/// which of them reach the engine options.
enum CoreConversionSettings {
    struct Tables {
        struct Destination { let usesCache: Bool; let importsPhotos: Bool; let shares: Bool }
        struct Direction { let exportsAndroid: Bool }
        struct Vendor { let supportsHEIC: Bool }
        struct Preset { let direction: ConversionDirection; let vendor: AndroidVendor }
        let defaults: [String:Any]
        let directions: [String:Direction]
        let vendors: [String:Vendor]
        let destinations: [String:Destination]
        let presets: [String:Preset]
        let currentPresets: [String:MigrationTargetPreset]
        func direction(_ raw: String) -> Direction { directions[raw]! }
        func vendor(_ raw: String) -> Vendor { vendors[raw]! }
        func destination(_ raw: String) -> Destination { destinations[raw]! }
        func preset(_ raw: String) -> Preset { presets[raw]! }
        func currentPreset(_ direction: ConversionDirection, _ vendor: AndroidVendor) -> MigrationTargetPreset {
            currentPresets[direction.rawValue + "/" + vendor.rawValue]!
        }
    }
    static let tables: Tables = {
        let v: [String:Any]
        do { v = try RustCore.engine(["mode":"conversion-settings-tables"]) }
        catch { fatalError("Rust core conversion-settings-tables: \(error.localizedDescription)") }
        func table<T>(_ key: String, _ make: ([String:Any]) -> T) -> [String:T] {
            (v[key] as! [String:[String:Any]]).mapValues(make)
        }
        return Tables(defaults: v["defaults"] as! [String:Any],
            directions: table("directions") { Tables.Direction(exportsAndroid: $0["exports_android"] as! Bool) },
            vendors: table("vendors") { Tables.Vendor(supportsHEIC: $0["supports_heic"] as! Bool) },
            destinations: table("destinations") { Tables.Destination(usesCache: $0["uses_cache"] as! Bool,
                importsPhotos: $0["imports_photos"] as! Bool, shares: $0["shares"] as! Bool) },
            presets: table("presets") { Tables.Preset(direction: choice($0["direction"]), vendor: choice($0["vendor"])) },
            currentPresets: (v["current_preset"] as! [String:String]).mapValues { choice($0) })
    }()
    static func choice<T: SettingsChoice>(_ raw: Any?) -> T {
        guard let raw = raw as? String, let value = T(rawValue: raw) else { fatalError("Rust core: unknown \(T.self) \(String(describing: raw))") }
        return value
    }
    static func defaultChoice<T: SettingsChoice>(_ key: String) -> T { choice(tables.defaults[key]) }
    static func defaultValue<T>(_ key: String) -> T {
        if T.self == Double.self { return (tables.defaults[key] as! NSNumber).doubleValue as! T }
        return tables.defaults[key] as! T
    }

    struct Decision {
        let normalized: [String:Any]
        let options: [String:Any]
        let privateHDRAvailable: Bool
        let privateHDREnabled: Bool
        let destinations: [OutputDestination]
        let action: String
    }
    static func request(_ s: ConversionSettings) -> [String:Any] {
        // JSON cannot carry NaN or infinity; the core reads null as "not a usable value".
        func number(_ value: Double) -> Any { value.isFinite ? value : NSNull() }
        var v: [String:Any] = ["direction": s.direction.rawValue, "destination": s.destination.rawValue, "album_title": s.albumTitle,
            "vendor": s.vendor.rawValue, "container": s.container.rawValue, "image_quality": number(s.imageQuality),
            "gain_quality": number(s.gainQuality), "preserve_jpeg": s.preserveJPEG, "legacy_hdr": s.legacyHDR,
            "export_data_files": s.exportDataFiles, "track": s.track.rawValue, "export_mode": s.exportMode.rawValue,
            "missing_time": s.missingTime.rawValue, "time_domain": s.timeDomain.rawValue,
            "repair_auxiliary_duration": s.repairAuxiliaryDuration, "has_cover_selections": !(s.coverSelections ?? [:]).isEmpty]
        if let value = s.exifFileTimes { v["exif_file_times"] = value }
        if let value = s.photoTimeRepair { v["photo_time_repair"] = value }
        if let value = s.photoTimeSynthetic { v["photo_time_synthetic"] = value }
        if let value = s.photoTimeZone { v["photo_time_zone"] = value }
        if let value = s.photoTimeReference { v["photo_time_reference"] = number(value) }
        if let value = s.candidateDelivery { v["candidate_delivery"] = value.rawValue }
        return v
    }
    static func decide(_ s: ConversionSettings) -> Decision {
        let v: [String:Any]
        do { v = try RustCore.engine(["mode":"conversion-settings","settings":request(s),"current_zone":TimeZone.current.identifier]) }
        catch { fatalError("Rust core conversion-settings: \(error.localizedDescription)") }
        return Decision(normalized: v["normalized"] as! [String:Any], options: v["options"] as! [String:Any],
            privateHDRAvailable: v["private_hdr_available"] as! Bool, privateHDREnabled: v["private_hdr_enabled"] as! Bool,
            destinations: (v["destinations"] as! [String]).map { choice($0) }, action: v["action"] as! String)
    }
}

enum AppName {
    static func display(languages: [String]) -> String {
        languages.first?.lowercased().hasPrefix("zh") == true ? "留刻" : "MomentStays"
    }
    static var display: String { display(languages:Bundle.main.preferredLocalizations) }
}

// Preserve the legacy preference domain, copy only keys absent from the new
// identity, and retain the existing Motion-Live task/cache root without moving it.
enum AppIdentityMigration {
    static func merged(previous:[String:Any], current:[String:Any], oldID:String) -> [String:Any] {
        let marker = "MomentStaysIdentityMigration1"
        guard current[marker] == nil else { return current }
        var result = current
        let keys = previous.keys.filter { current[$0] == nil }.sorted()
        for key in keys { result[key] = previous[key] }
        result[marker] = ["from":oldID,"date":ISO8601DateFormatter().string(from:Date()),"copiedKeys":keys] as [String:Any]
        return result
    }
    static func migrate(defaults: UserDefaults = .standard, oldID: String = "app.motionlive.converter", newID: String = "com.linofish.momentstays") {
        let marker = "MomentStaysIdentityMigration1"
        let current = defaults.persistentDomain(forName:newID) ?? [:]
        guard current[marker] == nil else { return }
        let previous = defaults.persistentDomain(forName:oldID) ?? [:]
        defaults.setPersistentDomain(merged(previous:previous,current:current,oldID:oldID),forName:newID)
    }
}

// Presentation-only localization. Media facts, resource names and persisted option
// values remain unchanged. Original filenames and paths are never rewritten.
enum AppText {
    static let catalog: Catalog = {
        guard let url = Bundle.main.url(forResource:"Workbench.en",withExtension:"json"),
              let data = try? Data(contentsOf:url),
              let json = try? JSONSerialization.jsonObject(with:data) as? [String:Any] else { return Catalog(strings:[:]) }
        return Catalog(strings:json["strings"] as? [String:String] ?? [:],verbatim:json["verbatimArguments"] as? [String:[Int]] ?? [:])
    }()
    static func text(_ value:String) -> String {
        let language = Bundle.main.preferredLocalizations.first ?? Locale.preferredLanguages.first ?? "en"
        return language.hasPrefix("zh") ? value : catalog.translate(value)
    }
    final class Catalog {
        struct Template { let regex:NSRegularExpression; let key:String; let value:String; let prefix:String; let slots:[Int] }
        let strings:[String:String]
        let templates:[Template]
        let fragments:[(String,String)]
        let cache=NSCache<NSString,NSString>()
        init(strings:[String:String],verbatim:[String:[Int]]=[:]) {
            self.strings=strings
            var patterns:[Template]=[]
            let slot=try! NSRegularExpression(pattern:"\\{([0-9]+)\\}")
            for (key,value) in strings {
                let range=NSRange(key.startIndex...,in:key),matches=slot.matches(in:key,range:range)
                guard !matches.isEmpty else { continue }
                var pattern="^",cursor=key.startIndex,indices:[Int]=[]
                for match in matches {
                    guard let r=Range(match.range,in:key),let number=Range(match.range(at:1),in:key),let index=Int(key[number]) else { continue }
                    pattern+=NSRegularExpression.escapedPattern(for:String(key[cursor..<r.lowerBound]))+"(.*?)"
                    cursor=r.upperBound;indices.append(index)
                }
                pattern+=NSRegularExpression.escapedPattern(for:String(key[cursor...]))+"$"
                if let regex=try? NSRegularExpression(pattern:pattern,options:.dotMatchesLineSeparators) {
                    let prefix=String(key.prefix(while:{$0 != "{"}))
                    patterns.append(Template(regex:regex,key:key,value:value,prefix:prefix,slots:indices))
                }
            }
            templates=patterns.sorted { $0.key.count > $1.key.count }
            fragments=strings.filter { !$0.key.contains("{") && $0.key.count>1 && ($0.key.hasSuffix("：") || $0.key.hasSuffix("「") || $0.key.hasPrefix("」")) }.map { ($0.key,$0.value) }.sorted {$0.0.count>$1.0.count}
            self.verbatim=verbatim;cache.countLimit=2048
        }
        private let verbatim:[String:[Int]]
        func translate(_ value:String) -> String { render(value,depth:0) }
        private func render(_ value:String,depth:Int) -> String {
            guard depth<5, !value.hasPrefix("/"),value.unicodeScalars.contains(where:{$0.value>=0x4e00 && $0.value<=0x9fff}) else { return value }
            if let result=strings[value] { return result }
            if let result=cache.object(forKey:value as NSString) { return result as String }
            let full=NSRange(value.startIndex...,in:value)
            for entry in templates where value.hasPrefix(entry.prefix) {
                guard let match=entry.regex.firstMatch(in:value,range:full) else { continue }
                var arguments:[Int:String]=[:]
                var format=entry.value
                for (capture,index) in entry.slots.enumerated() {
                    guard let r=Range(match.range(at:capture+1),in:value) else { continue }
                    let original=String(value[r])
                    arguments[index]=verbatim[entry.key]?.contains(index)==true ? original : render(original,depth:depth+1)
                    if original == "1" {
                        for (plural,singular) in [("photos","photo"),("Photos","Photo"),("frames","frame"),("sources","source"),("tasks","task"),("items","item"),("burst groups","burst group")] {
                            format=format.replacingOccurrences(of:"{\(index)} " + plural,with:"{\(index)} " + singular)
                        }
                    }
                }
                let marker=try! NSRegularExpression(pattern:"\\{([0-9]+)\\}")
                var result="",cursor=format.startIndex
                for token in marker.matches(in:format,range:NSRange(format.startIndex...,in:format)) {
                    guard let r=Range(token.range,in:format),let n=Range(token.range(at:1),in:format),let index=Int(format[n]) else { continue }
                    result+=String(format[cursor..<r.lowerBound]) + (arguments[index] ?? String(format[r]));cursor=r.upperBound
                }
                result+=String(format[cursor...])
                cache.setObject(result as NSString,forKey:value as NSString);return result
            }
            for separator in ["\n","；"," · "] where value.contains(separator) {
                let parts=value.components(separatedBy:separator),translated=parts.map { render($0,depth:depth+1) }
                if translated != parts { return translated.joined(separator:separator == "；" ? "; " : separator) }
            }
            for (source,target) in fragments {
                if value.hasPrefix(source) { return target+render(String(value.dropFirst(source.count)),depth:depth+1) }
                if value.hasSuffix(source) { return render(String(value.dropLast(source.count)),depth:depth+1)+target }
            }
            // Preserve an unknown diagnostic verbatim rather than guessing its meaning.
            return value
        }
    }
}
