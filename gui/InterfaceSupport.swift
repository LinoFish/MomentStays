import SwiftUI

#if os(macOS)
import AppKit

#if UI_DIAGNOSTICS
enum OutputMenuDiagnostics {
    static var titles: [String] = []
    static var selected = ""
}
enum SourceMenuDiagnostics {
    static var opened = false
    static var titles: [String] = []
}
#endif

// Use a system menu anchored to the prominent add button, including for Command-O.
struct SourceMenuButton: NSViewRepresentable {
    @Binding var requested: Bool
    let disabled: Bool
    var title = "添加照片"
    var details = ""
    var hasSources = false
    var allowsLibrary = true
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
        button.cell?.lineBreakMode = .byTruncatingMiddle
        button.setContentCompressionResistancePriority(.defaultLow,for:.horizontal)
        button.setContentHuggingPriority(.defaultLow,for:.horizontal)
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
        button.title = title
        let paragraph = NSMutableParagraphStyle(); paragraph.lineBreakMode = .byTruncatingMiddle
        button.attributedTitle = NSAttributedString(string:title,attributes:[
            .foregroundColor:InterfaceStyle.nativeAccentForeground,
            .font:NSFont.systemFont(ofSize:NSFont.systemFontSize),.paragraphStyle:paragraph
        ])
        button.toolTip = details.isEmpty ? "添加照片、文件夹或系统照片图库" : details + "\n点击继续添加照片、文件夹或图库照片"
        button.setAccessibilityLabel(hasSources ? title + "，继续添加照片" : "添加照片")
        context.coordinator.menu.items[0].title = hasSources ? "继续添加照片或文件夹…" : "选择照片或文件夹…"
        context.coordinator.menu.items[1].isHidden = !allowsLibrary
        context.coordinator.menu.items[1].title = hasSources ? "继续从系统图库添加…" : "扫描系统照片图库…"
        button.invalidateIntrinsicContentSize()
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
            SourceMenuDiagnostics.titles = menu.items.filter { !$0.isHidden }.map(\.title)
            if CommandLine.arguments.contains("--ui-source-menu") {
                let timer = Timer(timeInterval:0.3,repeats:false) { _ in menu.cancelTracking() }
                RunLoop.main.add(timer,forMode:.common)
            }
        }
        #endif
    }
}
// A constrained native split control keeps long paths inside their toolbar slot.
struct OutputLocationControl: NSViewRepresentable {
    let title: String
    let details: String
    let symbol: String
    let disabled: Bool
    @Binding var destination: OutputDestination
    let destinations: [OutputDestination]
    let outputPath: String?
    let changeLocation: () -> Void
    let chooseFolder: () -> Void
    let chooseAlbum: () -> Void
    let copyPath: () -> Void
    var chooseDestination: ((OutputDestination) -> Void)? = nil

    func makeCoordinator() -> Coordinator { Coordinator() }
    func makeNSView(context: Context) -> NSView {
        let root = NSView()
        let primary = context.coordinator.primary, arrow = context.coordinator.arrow
        for button in [primary,arrow] {
            button.bezelStyle = .rounded; button.controlSize = .regular
            button.font = .systemFont(ofSize:NSFont.systemFontSize)
            button.translatesAutoresizingMaskIntoConstraints = false
            root.addSubview(button)
            button.centerYAnchor.constraint(equalTo:root.centerYAnchor).isActive = true
            button.heightAnchor.constraint(equalToConstant:20).isActive = true
        }
        primary.target = context.coordinator; primary.action = #selector(Coordinator.changeLocation)
        primary.cell?.lineBreakMode = .byTruncatingMiddle
        primary.setContentCompressionResistancePriority(.defaultLow,for:.horizontal)
        primary.imagePosition = .imageLeading; primary.alignment = .left
        primary.identifier = NSUserInterfaceItemIdentifier("output-location-button")
        primary.setAccessibilityIdentifier("output-location-button")
        arrow.target = context.coordinator; arrow.action = #selector(Coordinator.openMenu)
        arrow.image = NSImage(systemSymbolName:"chevron.down",accessibilityDescription:nil)
        arrow.imagePosition = .imageOnly; arrow.setAccessibilityLabel("更改保存方式")
        arrow.identifier = NSUserInterfaceItemIdentifier("output-mode-button")
        arrow.setAccessibilityIdentifier("output-mode-button")
        NSLayoutConstraint.activate([
            primary.leadingAnchor.constraint(equalTo:root.leadingAnchor),
            primary.trailingAnchor.constraint(equalTo:arrow.leadingAnchor,constant:-4),
            arrow.trailingAnchor.constraint(equalTo:root.trailingAnchor),
            arrow.widthAnchor.constraint(equalToConstant:24)
        ])
        return root
    }
    func updateNSView(_ root: NSView, context: Context) {
        let coordinator = context.coordinator
        coordinator.configuration = self
        coordinator.primary.title = title
        coordinator.primary.image = NSImage(systemSymbolName:symbol,accessibilityDescription:nil)
        coordinator.primary.toolTip = details
        coordinator.primary.setAccessibilityLabel(title + "，更改输出位置")
        coordinator.primary.isEnabled = !disabled; coordinator.arrow.isEnabled = !disabled
        coordinator.arrow.toolTip = "更改保存方式、文件夹或目标相册"
    }
    final class Coordinator: NSObject, NSMenuDelegate {
        let primary = NSButton(), arrow = NSButton()
        var configuration: OutputLocationControl?
        private var menu = NSMenu()
        @objc func changeLocation() {
            if configuration?.chooseDestination != nil { openMenu() } else { configuration?.changeLocation() }
        }
        @objc func openMenu() {
            guard let config = configuration else { return }
            menu = NSMenu()
            for destination in config.destinations where destination != .ask {
                let title = config.chooseDestination == nil ? destination.title : destination == .photos ? "相册" : destination == .folderAndPhotos ? "相册及文件夹" : "文件夹"
                let item = NSMenuItem(title:title,action:#selector(selectDestination(_:)),keyEquivalent:"")
                item.target = self; item.representedObject = destination.rawValue
                item.state = destination == config.destination ? .on : .off
                menu.addItem(item)
            }
            if config.chooseDestination == nil {
            if !config.destination.usesCache || config.destination.importsPhotos { menu.addItem(.separator()) }
            if !config.destination.usesCache {
                add(config.outputPath == nil ? "选择文件夹…" : "更改文件夹…",#selector(chooseFolder))
                if config.outputPath != nil { add("复制完整路径",#selector(copyPath)) }
            }
            if config.destination.importsPhotos { add("选择目标相册…",#selector(chooseAlbum)) }
            }
            menu.delegate = self
            let anchor = config.chooseDestination == nil ? arrow : primary
            menu.popUp(positioning:nil,at:NSPoint(x:0,y:anchor.bounds.minY - 4),in:anchor)
        }
        #if UI_DIAGNOSTICS
        func menuWillOpen(_ menu: NSMenu) {
            OutputMenuDiagnostics.titles = menu.items.map(\.title)
            OutputMenuDiagnostics.selected = configuration?.destination.rawValue ?? ""
            if CommandLine.arguments.contains("--ui-output-menu") {
                let timer = Timer(timeInterval:0.3,repeats:false) { _ in
                    menu.cancelTracking()
                    if CommandLine.arguments.contains("--ui-output-switch"),
                       let item = menu.items.first(where:{ ($0.representedObject as? String) == "photos" }) {
                        NSApp.sendAction(item.action!,to:item.target,from:item)
                    }
                }
                RunLoop.main.add(timer,forMode:.common)
            }
        }
        #endif
        private func add(_ title: String, _ action: Selector) {
            let item = NSMenuItem(title:title,action:action,keyEquivalent:""); item.target = self; menu.addItem(item)
        }
        @objc private func selectDestination(_ item: NSMenuItem) {
            guard let value = item.representedObject as? String, let destination = OutputDestination(rawValue:value) else { return }
            if let choose = configuration?.chooseDestination { choose(destination) }
            else { configuration?.destination = destination }
        }
        @objc private func chooseFolder() { configuration?.chooseFolder() }
        @objc private func chooseAlbum() { configuration?.chooseAlbum() }
        @objc private func copyPath() { configuration?.copyPath() }
    }
}
#else
struct SourceMenuButton: View {
    @Binding var requested: Bool
    let disabled: Bool
    var title = "添加照片"
    var details = ""
    var hasSources = false
    var allowsLibrary = true
    let addFiles: () -> Void
    let addLibrary: () -> Void
    var body: some View {
        Menu {
            Button("选择照片或文件夹…",action:addFiles)
            if allowsLibrary { Button("扫描系统照片图库…",action:addLibrary) }
        } label: { Label(title,systemImage:"plus") }
        .disabled(disabled).accessibilityIdentifier("import-button")
    }
}
struct OutputLocationControl: View {
    let title: String, details: String, symbol: String
    let disabled: Bool
    @Binding var destination: OutputDestination
    let destinations: [OutputDestination]
    let outputPath: String?
    let changeLocation: () -> Void, chooseFolder: () -> Void, chooseAlbum: () -> Void, copyPath: () -> Void
    var body: some View {
        Menu {
            Picker("保存方式",selection:$destination) { ForEach(destinations,id:\.self) { Text($0.title).tag($0) } }
            Button("更改输出位置…",action:changeLocation)
        } label: { Label(title,systemImage:symbol).lineLimit(1) }
        .disabled(disabled)
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
    var clearSources: () -> Void = {}
    var chooseOutput: () -> Void = {}
    var chooseDestination: (OutputDestination) -> Void = { _ in }
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
    enum Spacing {
        static let small: CGFloat = 8
        static let medium: CGFloat = 12
        static let content: CGFloat = 16
        static let section: CGFloat = 24
        static let spacious: CGFloat = 32
    }
    static let photoRadius: CGFloat = 10
    static let controlRadius: CGFloat = 8
    static var selection: Color { accent.opacity(0.14) }
    static var gallery: Color { canvas }
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

enum WindowSizing {
    // Content sizes; native title bars add their own height to the window frame.
    static let workspace = CGSize(width:720,height:600)
    static let library = CGSize(width:640,height:520)
    static let recent = CGSize(width:560,height:500)
    static let report = CGSize(width:480,height:420)
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
    var margin: CGFloat { compact ? InterfaceStyle.Spacing.medium : InterfaceStyle.Spacing.section }
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
        case .ready: return "可转换"
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
        case .ready: return "暂无可转换照片"
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
/// 面向用户的名称统一为留刻；bundle ID 和可执行文件名保持兼容。
enum AppName { static let display = "留刻" }

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
