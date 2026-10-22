import Foundation

// Value types deliberately contain no AppKit/UIKit. Each job receives a snapshot.
protocol SettingsChoice: RawRepresentable, CaseIterable, Hashable, Codable where RawValue == String {
    var title: String { get }
}

enum ConversionDirection: String, SettingsChoice {
    case toApple = "to-apple", toAndroid = "to-android"
    var title: String { self == .toApple ? "安卓 → 苹果实况" : "苹果实况 → 安卓" }
}

enum AndroidVendor: String, SettingsChoice {
    case google, xiaomi, samsung, huawei, honor, oppo, vivo
    var title: String {
        switch self {
        case .google: return "Google / 通用"
        case .xiaomi: return "小米"
        case .samsung: return "三星"
        case .huawei: return "华为"
        case .honor: return "荣耀"
        case .oppo: return "OPPO"
        case .vivo: return "vivo V2"
        }
    }
    var supportsHEIC: Bool { self == .samsung }
}

enum PhotoContainer: String, SettingsChoice {
    case jpeg, heic
    var title: String { self == .jpeg ? "JPEG · Ultra HDR" : "HEIC · 保留原编码" }
}

enum VideoTrack: String, SettingsChoice {
    case primary, secondary
    var title: String { self == .primary ? "主运动轨" : "辅助影像轨" }
}

enum ExportMode: String, SettingsChoice {
    case livePhoto = "livephoto", withAuxiliary = "livephoto+auxiliary", auxiliaryOnly = "auxiliary-only"
    var title: String {
        switch self {
        case .livePhoto: return "仅 Live Photo"
        case .withAuxiliary: return "Live Photo ＋ 辅助轨"
        case .auxiliaryOnly: return "仅导出辅助轨"
        }
    }
}

enum MissingTimePolicy: String, SettingsChoice {
    case error, midpoint
    var title: String { self == .error ? "报告错误（推荐）" : "使用视频中点" }
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
    case folder, photos, folderAndPhotos, share, folderAndShare
    var title: String {
        switch self {
        case .folder: return "保存到文件夹"
        case .photos: return "转换后导入系统图库"
        case .folderAndPhotos: return "文件夹并导入系统图库"
        case .share: return "转换后直接共享"
        case .folderAndShare: return "文件夹并共享"
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
    var destination: OutputDestination = .folder
    var albumTitle = "MomentStays 转换结果"
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

    mutating func normalize() {
        if direction == .toAndroid && destination.importsPhotos { destination = .folder }
        if direction == .toApple && destination.shares { destination = .folder }
        if exportMode == .auxiliaryOnly && direction == .toApple && destination.importsPhotos { destination = .folder }
        if albumTitle.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { albumTitle = "MomentStays 转换结果" }
        if !vendor.supportsHEIC { container = .jpeg }
        imageQuality = min(1, max(0.01, imageQuality.isFinite ? imageQuality : 0.95))
        gainQuality = min(1, max(0.01, gainQuality.isFinite ? gainQuality : 0.95))
    }

    var options: ConversionOptions {
        var result = ConversionOptions(legacyHDR: legacyHDR, missingTime: missingTime.rawValue,
            timeDomain: timeDomain.rawValue, videoTrack: track.rawValue, outputMode: exportMode.rawValue,
            repairAuxiliaryDuration: repairAuxiliaryDuration, exportDataFiles: exportDataFiles)
        result.direction = direction.rawValue
        result.android = AndroidOptions(vendor: vendor.rawValue, container: container.rawValue,
            hdr: container == .heic ? "preserve" : "ultra", imageQuality: imageQuality,
            gainQuality: gainQuality, preserveJPEG: preserveJPEG)
        return result
    }

    var summary: String {
        direction == .toAndroid ? "\(vendor.title) · \(container == .heic ? "HEIC 原编码" : "JPEG")" : "\(track.title) · \(exportMode.title)"
    }

    var destinations: [OutputDestination] {
        direction == .toAndroid ? [.folder, .share, .folderAndShare] : exportMode == .auxiliaryOnly ? [.folder] : [.folder, .photos, .folderAndPhotos]
    }
    var actionTitle: String {
        if destination.importsPhotos { return "转换并导入图库" }
        if destination.shares { return "转换并共享" }
        return direction == .toAndroid ? "导出到安卓" : exportMode == .auxiliaryOnly ? "导出辅助轨" : "开始转换"
    }
}
