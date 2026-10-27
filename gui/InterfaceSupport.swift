import SwiftUI

#if os(macOS)
import AppKit

#if UI_DIAGNOSTICS
enum SourceMenuDiagnostics {
    static var opened = false
    static var titles: [String] = []
}
#endif

// Use a system menu anchored to the prominent add button, including for Command-O.
struct SourceMenuButton: NSViewRepresentable {
    @Binding var requested: Bool
    let disabled: Bool
    let addFiles: () -> Void
    let addLibrary: () -> Void

    func makeCoordinator() -> Coordinator { Coordinator() }
    func makeNSView(context: Context) -> NSButton {
        let button = NSButton(title:"添加照片",target:context.coordinator,action:#selector(Coordinator.openMenu(_:)))
        button.bezelStyle = .rounded
        button.controlSize = .regular
        button.font = .systemFont(ofSize:NSFont.systemFontSize)
        button.bezelColor = InterfaceStyle.nativeAccent
        button.contentTintColor = InterfaceStyle.nativeAccentForeground
        button.attributedTitle = NSAttributedString(string: "添加照片", attributes: [
            .foregroundColor: InterfaceStyle.nativeAccentForeground,
            .font: NSFont.systemFont(ofSize: NSFont.systemFontSize)
        ])
        button.image = NSImage(systemSymbolName:"plus",accessibilityDescription:nil)?
            .withSymbolConfiguration(.init(paletteColors: [InterfaceStyle.nativeAccentForeground]))
        button.imagePosition = .imageLeading
        button.identifier = NSUserInterfaceItemIdentifier("import-button")
        button.setAccessibilityIdentifier("import-button")
        button.setAccessibilityLabel("添加照片")
        button.toolTip = "添加照片、文件夹或系统照片图库"
        button.setContentHuggingPriority(.required,for:.horizontal)
        button.sizeToFit()
        context.coordinator.menu.delegate = context.coordinator
        context.coordinator.menu.minimumWidth = 230
        for (title,symbol,action) in [
            ("选择照片或文件夹…","folder",#selector(Coordinator.files)),
            ("扫描系统照片图库…","photo.on.rectangle",#selector(Coordinator.library))
        ] {
            let item = NSMenuItem(title:title,action:action,keyEquivalent:"")
            item.target = context.coordinator
            item.image = NSImage(systemSymbolName:symbol,accessibilityDescription:nil)
            context.coordinator.menu.addItem(item)
        }
        return button
    }
    func updateNSView(_ button: NSButton, context: Context) {
        button.isEnabled = !disabled
        context.coordinator.addFiles = addFiles
        context.coordinator.addLibrary = addLibrary
        if requested {
            DispatchQueue.main.async {
                requested = false
                if button.isEnabled { button.performClick(nil) }
            }
        }
    }
    final class Coordinator: NSObject, NSMenuDelegate {
        let menu = NSMenu()
        var addFiles: (() -> Void)?
        var addLibrary: (() -> Void)?
        @objc func openMenu(_ button: NSButton) {
            guard button.isEnabled else { return }
            menu.popUp(positioning:nil,at:NSPoint(x:0,y:button.bounds.minY - 4),in:button)
        }
        @objc func files() { addFiles?() }
        @objc func library() { addLibrary?() }
        #if UI_DIAGNOSTICS
        func menuWillOpen(_ menu: NSMenu) {
            SourceMenuDiagnostics.opened = true
            SourceMenuDiagnostics.titles = menu.items.map(\.title)
            if CommandLine.arguments.contains("--ui-source-menu") {
                let timer = Timer(timeInterval:0.3,repeats:false) { _ in menu.cancelTracking() }
                RunLoop.main.add(timer,forMode:.common)
            }
        }
        #endif
    }
}
#else
struct SourceMenuButton: View {
    @Binding var requested: Bool
    let disabled: Bool
    let addFiles: () -> Void
    let addLibrary: () -> Void
    var body: some View {
        Menu {
            Button("选择照片或文件夹…",action:addFiles)
            Button("扫描系统照片图库…",action:addLibrary)
        } label: { Label("添加照片",systemImage:"plus") }
        .disabled(disabled).accessibilityIdentifier("import-button")
    }
}
#endif

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
    // System orange is the native semantic accent closest to the brand amber.
    #if os(macOS)
    static var nativeAccent: NSColor { .systemOrange }
    static let nativeAccentForeground = NSColor(srgbRed: 31.0 / 255, green: 41.0 / 255, blue: 51.0 / 255, alpha: 1)
    static var accent: Color { Color(nsColor: nativeAccent) }
    #else
    static var accent: Color { .orange }
    #endif
    static var accentForeground: Color { Color(red: 31.0 / 255, green: 41.0 / 255, blue: 51.0 / 255) }
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
    case ready, all, still, completed, attention
    var id: String { rawValue }
    var title: String {
        switch self {
        case .all: return "全部"
        case .ready: return "待转换"
        case .still: return "静态照片"
        case .completed: return "已完成"
        case .attention: return "需检查"
        }
    }
    var symbol: String {
        switch self {
        case .all: return "photo.on.rectangle"
        case .ready: return "circle"
        case .still: return "photo"
        case .completed: return "checkmark.circle"
        case .attention: return "exclamationmark.circle"
        }
    }
    var emptyTitle: String {
        switch self {
        case .all: return "照片会显示在这里"
        case .ready: return "暂无待转换照片"
        case .still: return "当前来源中没有静态照片"
        case .completed: return "暂无已完成结果"
        case .attention: return "没有需要检查的照片"
        }
    }
    func includes(_ item: PhotoQueueItem, settings: ConversionSettings = ConversionSettings()) -> Bool {
        let category = item.category(settings:settings)
        switch self {
        case .all: return true
        case .ready: return category == .ready
        case .still: return category == .static
        case .completed: return category == .converted
        case .attention: return category == .inspect
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
