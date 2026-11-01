import Foundation

// Value types deliberately contain no AppKit/UIKit. Each job receives a snapshot.
protocol SettingsChoice: RawRepresentable, CaseIterable, Hashable, Codable where RawValue == String {
    var title: String { get }
}

enum ConversionDirection: String, SettingsChoice {
    case toApple = "to-apple", toAndroid = "to-android", androidToAndroid = "android-to-android"
    var title: String { switch self { case .toApple: return "安卓 → 苹果实况"; case .toAndroid: return "苹果实况 → 安卓"; case .androidToAndroid: return "安卓 → 安卓" } }
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
    var exportDataFiles = false
    var track: VideoTrack = .primary
    var exportMode: ExportMode = .livePhoto
    var missingTime: MissingTimePolicy = .error
    var timeDomain: CoverTimeDomain = .auto
    var repairAuxiliaryDuration = false
    // Optional for compatibility with task histories saved before cover selection.
    var coverSelections: [String:Int]?
    var candidateDelivery: CandidateDelivery?

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
        if direction == .androidToAndroid {
            var result = ConversionOptions()
            result.direction = direction.rawValue
            result.exportDataFiles = exportDataFiles
            return result
        }
        var result = ConversionOptions(legacyHDR: legacyHDR, missingTime: missingTime.rawValue,
            timeDomain: timeDomain.rawValue, videoTrack: track.rawValue, outputMode: exportMode.rawValue,
            repairAuxiliaryDuration: repairAuxiliaryDuration, exportDataFiles: exportDataFiles)
        result.direction = direction.rawValue
        result.coverSelections = exportMode == .auxiliaryOnly ? [:] : coverSelections ?? [:]
        result.candidateDelivery = (candidateDelivery ?? .staticPhotos).rawValue
        result.android = AndroidOptions(vendor: vendor.rawValue, container: container.rawValue,
            hdr: container == .heic ? "preserve" : "ultra", imageQuality: imageQuality,
            gainQuality: gainQuality, preserveJPEG: preserveJPEG)
        return result
    }

    var summary: String {
        if direction == .androidToAndroid { return "暂仅支持 Google → 荣耀 · JPEG 原编码" }
        if direction == .toAndroid { return "\(vendor.title) · \(container == .heic ? "HEIC 原编码" : "JPEG")" }
        var parts = [track.title,exportMode.title]
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
