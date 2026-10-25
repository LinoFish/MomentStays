import SwiftUI

enum UserGuide {
    static func title(for locale: Locale) -> String {
        locale.languageCode == "zh" ? "使用手册" : "User Guide"
    }

    static func url(for locale: Locale) -> URL {
        let page = locale.languageCode == "zh" ? "guide-zh.html" : "guide.html"
        return URL(string: "https://linofish.github.io/MomentStays/" + page)!
    }
}

// Platform integrations are injected by the host; shared views never open NSWindow
// or access PhotoKit/file permissions directly. An iOS host supplies equivalent actions.
struct WorkspaceActions {
    var chooseSources: () -> Void = {}
    var chooseOutput: () -> Void = {}
    var importPhotos: () -> Void = {}
    var start: () -> Void = {}
    var openOutput: () -> Void = {}
    var showReport: () -> Void = {}
    var reveal: (URL) -> Void = { _ in }
    var copy: (String) -> Void = { _ in }
    var acceptDrop: ([NSItemProvider], Bool) -> Bool = { _, _ in false }
    var shareResults: () -> Void = {}
    var chooseAlbum: () -> Void = {}
    var recentResults: () -> Void = {}
    var toggleLibraryMonitoring: () -> Void = {}
}

enum InterfaceStyle {
    // Follow the user's macOS accent preference in both light and dark appearances.
    static var accent: Color { .accentColor }
    static var canvas: Color {
        #if os(macOS)
        return Color(nsColor: .windowBackgroundColor)
        #else
        return Color(uiColor: .systemGroupedBackground)
        #endif
    }
    static var content: Color {
        #if os(macOS)
        return Color(nsColor: .controlBackgroundColor)
        #else
        return Color(uiColor: .systemBackground)
        #endif
    }
    static var separator: Color {
        #if os(macOS)
        return Color(nsColor: .separatorColor)
        #else
        return Color(uiColor: .separator)
        #endif
    }
}

struct WorkspaceLayout {
    let size: CGSize
    var compact: Bool { size.width < 640 }
    var stackedFooter: Bool { size.width < 800 }
    var supportsInspector: Bool { size.width >= 960 }
    var supportsSidebar: Bool { size.width >= 1100 }
    var sidebarWidth: CGFloat { 168 }
    var short: Bool { size.height < 560 }
    var inspectorWidth: CGFloat { min(320, max(280, size.width * 0.25)) }
    var margin: CGFloat { compact ? 12 : 16 }
}

extension PhotoState {
    var color: Color {
        switch self {
        case .failed: return .red
        case .inspect: return .orange
        case .converted, .resumed: return .green
        default: return .secondary
        }
    }
}

struct PhotoStatusLabel: View {
    let state: PhotoState
    var body: some View {
        Label(state.title, systemImage: state.symbol)
            .font(.callout).foregroundColor(state.color)
            .lineLimit(1).accessibilityElement(children: .combine)
    }
}

struct LayoutFramesKey: PreferenceKey {
    static var defaultValue: [String: CGRect] = [:]
    static func reduce(value: inout [String: CGRect], nextValue: () -> [String: CGRect]) {
        value.merge(nextValue(), uniquingKeysWith: { _, new in new })
    }
}

extension View {
    @ViewBuilder func dismissOnEscape(_ action: @escaping () -> Void) -> some View {
        #if os(macOS)
        onExitCommand(perform: action)
        #else
        self
        #endif
    }

    @ViewBuilder func recordLayout(_ name: String) -> some View {
        #if UI_DIAGNOSTICS
        background(GeometryReader { proxy in
            Color.clear.preference(key: LayoutFramesKey.self, value: [name: proxy.frame(in: .named("workspace"))])
        })
        #else
        self
        #endif
    }
}

// Width is measured inside the queue, after the inspector has taken its space.
struct QueueLayout {
    let width: CGFloat
    var twoLines: Bool { width < 560 }
    var showsCover: Bool { width >= 800 }
}

// Scope affects only the presentation; conversions always use the complete source job.
enum PhotoQueueScope: String, CaseIterable, Identifiable {
    case all, completed, attention
    var id: String { rawValue }
    var title: String {
        switch self {
        case .all: return "全部照片"
        case .completed: return "已完成"
        case .attention: return "需处理"
        }
    }
    var symbol: String {
        switch self {
        case .all: return "photo.on.rectangle"
        case .completed: return "checkmark.circle"
        case .attention: return "exclamationmark.circle"
        }
    }
    func includes(_ item: PhotoQueueItem) -> Bool {
        switch self {
        case .all: return true
        case .completed: return [.converted, .resumed].contains(item.state)
        case .attention: return [.inspect, .failed].contains(item.state) || (item.isMotion && item.cover == "未记录")
        }
    }
}

// Uses the same official artwork as the Dock icon and the GitHub/website brand assets.
struct MomentStaysBrandIcon: View {
    @Environment(\.colorScheme) private var colorScheme

    var body: some View {
        #if os(macOS)
        let name = NSImage.Name(colorScheme == .dark ? "AppIconDark" : "AppIcon")
        if let icon = Bundle.main.image(forResource: name) {
            Image(nsImage: icon)
                .renderingMode(.original)
                .resizable()
                .interpolation(.high)
                .scaledToFit()
        }
        #else
        Image(colorScheme == .dark ? "AppIconDark" : "AppIcon")
            .renderingMode(.original)
            .resizable()
            .scaledToFit()
        #endif
    }
}
