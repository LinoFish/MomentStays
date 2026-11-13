import Foundation

// Value types deliberately contain no AppKit/UIKit. Each job receives a snapshot.
protocol SettingsChoice: RawRepresentable, CaseIterable, Hashable, Codable where RawValue == String {
    var title: String { get }
}

enum ConversionDirection: String, SettingsChoice {
    case toApple = "to-apple", toAndroid = "to-android", androidToAndroid = "android-to-android", honorToGoogle = "honor-to-google"
    var title: String { switch self { case .toApple: return "受支持照片 → Apple"; case .toAndroid: return "苹果实况 → 安卓"; case .androidToAndroid: return "Google → 荣耀（仅 JPEG）"; case .honorToGoogle: return "荣耀 → Google（仅 JPEG）" } }
    var exportsAndroid: Bool { self != .toApple }
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
    var supportsHEIC: Bool { self == .samsung }
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
    var direction: ConversionDirection { self == .apple ? .toApple : self == .googleToHonor ? .androidToAndroid : self == .honorToGoogle ? .honorToGoogle : .toAndroid }
    var vendor: AndroidVendor {
        switch self {
        case .xiaomi: return .xiaomi
        case .samsung: return .samsung
        case .huawei: return .huawei
        case .honor: return .honor
        case .oppo: return .oppo
        case .vivo: return .vivo
        default: return .google
        }
    }
    var detail: String {
        switch self {
        case .apple: return "受支持实况、静态 HDR 与荣耀/华为同格式 JPEG 连拍 → Apple；实验支持 Google 原始文件名及三星 SEF JPEG 连拍。"
        case .googleToHonor: return "Google → 荣耀：仅支持已验证的 Google Motion Photo JPEG；不支持 HEIC。安卓互转仅此方向和荣耀 → Google。"
        case .honorToGoogle: return "荣耀 → Google：仅支持已识别荣耀实况 JPEG；静态 HDR 独立输出单图，具体支持以照片检查结果为准；不支持 HEIC。"
        default: return "\(title)；\(vendor.supportsHEIC ? "JPEG / HEIC" : "仅 JPEG")。本地静态 HDR 独立处理，具体支持以照片检查结果为准。"
        }
    }
    static func current(_ settings: ConversionSettings) -> Self {
        if settings.direction == .toApple { return .apple }
        if settings.direction == .androidToAndroid { return .googleToHonor }
        if settings.direction == .honorToGoogle { return .honorToGoogle }
        return Self(rawValue:settings.vendor.rawValue) ?? .google
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
        case .photos: return "照片图库"
        case .folderAndPhotos: return "文件夹和照片图库"
        case .share: return "共享菜单"
        case .folderAndShare: return "文件夹和共享菜单"
        }
    }
    var usesCache: Bool { self == .photos || self == .share }
    var importsPhotos: Bool { self == .photos || self == .folderAndPhotos }
    var shares: Bool { self == .share || self == .folderAndShare }
}

enum LibraryVersion: String, SettingsChoice {
    case original, edited
    var title: String { self == .original ? "未修改原片（保留原始 HDR）" : "当前编辑版（可能无 HDR）" }
}

struct ConversionSettings: Equatable, Codable {
    var direction: ConversionDirection = .toApple
    var destination: OutputDestination = .ask
    var outputConfirmed: Bool?
    var albumTitle = "留刻转换结果"
    var albumID: String?
    var libraryVersion: LibraryVersion = .original
    var vendor: AndroidVendor = .google
    var container: PhotoContainer = .jpeg
    var imageQuality = 0.95
    var gainQuality = 0.95
    var preserveJPEG = true
    var legacyHDR = true
    // Presentation only: retain the stored preference even when this output cannot use it.
    var privateHDRExperimentAvailable: Bool {
        direction != .androidToAndroid && !(direction == .toApple && exportMode == .auxiliaryOnly)
    }
    var privateHDRExperimentEnabled: Bool { legacyHDR && privateHDRExperimentAvailable }
    var exportDataFiles = false
    var track: VideoTrack = .primary
    var exportMode: ExportMode = .livePhoto
    var missingTime: MissingTimePolicy = .error
    var timeDomain: CoverTimeDomain = .auto
    var repairAuxiliaryDuration = false
    // Optional for compatibility with task histories saved before cover selection.
    var coverSelections: [String:Int]?
    var manualMidpoints: [String:Bool]?
    var manualCovers: [String:ManualCoverChoice]?
    var candidateDelivery: CandidateDelivery?
    var burstChoices: [String:BurstChoice]?

    func coverIndex(for source: URL) -> Int? { direction == .toApple ? coverSelections?[NativeBatch.normalized(source).path] : nil }

    mutating func normalize() {
        if direction.exportsAndroid && destination == .ask { destination = .folder }
        if direction.exportsAndroid && destination.importsPhotos { destination = .folder }
        if direction == .toApple && destination.shares { destination = .folder }
        if exportMode == .auxiliaryOnly && direction == .toApple && destination.importsPhotos { destination = .folder }
        if exportMode == .withCandidates || !(coverSelections ?? [:]).isEmpty { track = .primary }
        if albumTitle.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { albumTitle = "留刻转换结果" }
        if !vendor.supportsHEIC { container = .jpeg }
        imageQuality = min(1, max(0.01, imageQuality.isFinite ? imageQuality : 0.95))
        gainQuality = min(1, max(0.01, gainQuality.isFinite ? gainQuality : 0.95))
    }

    var options: ConversionOptions {
        if direction == .androidToAndroid || direction == .honorToGoogle {
            var result = ConversionOptions(legacyHDR: legacyHDR)
            result.direction = direction.rawValue
            result.exportDataFiles = exportDataFiles
            return result
        }
        var result = ConversionOptions(legacyHDR: legacyHDR, missingTime: missingTime.rawValue,
            timeDomain: timeDomain.rawValue, videoTrack: track.rawValue, outputMode: exportMode.rawValue,
            repairAuxiliaryDuration: repairAuxiliaryDuration, exportDataFiles: exportDataFiles)
        result.direction = direction.rawValue
        result.burstChoices = burstChoices ?? [:]
        result.manualCovers = direction == .toApple ? manualCovers ?? [:] : [:]
        result.coverSelections = exportMode == .auxiliaryOnly ? [:] : coverSelections ?? [:]
        result.candidateDelivery = (candidateDelivery ?? .staticPhotos).rawValue
        result.android = AndroidOptions(vendor: vendor.rawValue, container: container.rawValue,
            hdr: container == .heic ? "preserve" : "ultra", imageQuality: imageQuality,
            gainQuality: gainQuality, preserveJPEG: preserveJPEG)
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

    var destinations: [OutputDestination] {
        direction.exportsAndroid ? [.folder, .share, .folderAndShare] : exportMode == .auxiliaryOnly ? [.ask,.folder] : [.ask,.photos,.folder,.folderAndPhotos]
    }
    var actionTitle: String {
        if destination.importsPhotos { return "转换并导入图库" }
        if destination.shares { return "转换并共享" }
        return direction.exportsAndroid ? "导出到安卓" : exportMode == .auxiliaryOnly ? "导出辅助轨" : "开始转换"
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
