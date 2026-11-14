import SwiftUI

#if os(macOS)
import AppKit

// MARK: - Queue marquee selection (presentation only)
// Frame stamps bind geometry to the current filter/order/viewport. No media IO.
struct QueueMarqueeLayout: Equatable {
    let filter: String
    let grid: Bool
    let size: CGSize
    let ids: [String]
    var origin: CGPoint = .zero
}
struct QueueMarqueeConfiguration: Equatable {
    let layout: QueueMarqueeLayout
    let sourceRevision: [String]
    let sources: [String]
    let selected: Set<String>
    let explicit: Bool
    let enabled: Bool
}
struct QueueMarqueeFrame: Equatable {
    let layout: QueueMarqueeLayout
    let rect: CGRect
}
struct QueueMarqueeFramesKey: PreferenceKey {
    static var defaultValue: [String:QueueMarqueeFrame] = [:]
    static func reduce(value: inout [String:QueueMarqueeFrame], nextValue: () -> [String:QueueMarqueeFrame]) {
        value.merge(nextValue(),uniquingKeysWith:{ _,new in new })
    }
}
struct QueueMarqueeFrameReader: View {
    let id: String
    let layout: QueueMarqueeLayout
    var body: some View {
        GeometryReader { proxy in
            Color.clear.preference(key:QueueMarqueeFramesKey.self,
                // List rows have separate hosting roots on macOS 12. Window
                // coordinates work across those roots; named spaces do not.
                value:[id:QueueMarqueeFrame(layout:layout,rect:proxy.frame(in:.global))])
        }.allowsHitTesting(false)
    }
}
enum QueueMarqueeMode: Equatable {
    case replace, add, toggle
    init(_ flags: NSEvent.ModifierFlags) {
        self = flags.contains(.command) ? .toggle : flags.contains(.shift) ? .add : .replace
    }
}
enum QueueMarqueeMath {
    static let space = "photo-queue-marquee"
    static func rectangle(_ a: CGPoint, _ b: CGPoint) -> CGRect {
        CGRect(x:min(a.x,b.x),y:min(a.y,b.y),width:abs(a.x-b.x),height:abs(a.y-b.y))
    }
    static func hits(_ rect: CGRect, frames: [String:CGRect]) -> Set<String> {
        Set(frames.compactMap { id,frame in
            let overlap = rect.intersection(frame)
            return !overlap.isNull && overlap.width > 0 && overlap.height > 0 ? id : nil
        })
    }
    static func selection(base: Set<String>, eligible: Set<String>, hits: Set<String>, mode: QueueMarqueeMode) -> Set<String> {
        let safeHits = hits.intersection(eligible)
        switch mode {
        case .replace: return base.subtracting(eligible).union(safeHits)
        case .add: return base.union(safeHits)
        case .toggle: return base.symmetricDifference(safeHits)
        }
    }
    static func scrollStep(y: CGFloat, viewport: CGRect) -> CGFloat {
        let edge: CGFloat = min(24,viewport.height/4)
        if y < viewport.minY+edge { return -min(14,max(2,(viewport.minY+edge-y)*0.6)) }
        if y > viewport.maxY-edge { return min(14,max(2,(y-viewport.maxY+edge)*0.6)) }
        return 0
    }
}

struct QueueMarqueeArea: NSViewRepresentable {
    let configuration: QueueMarqueeConfiguration
    let frames: [String:QueueMarqueeFrame]
    let accent: NSColor
    let draft: (Set<String>?) -> Void
    let commit: (Set<String>) -> Void
    func makeNSView(context: Context) -> QueueMarqueeView { QueueMarqueeView() }
    func updateNSView(_ view: QueueMarqueeView, context: Context) {
        view.accent = accent; view.draft = draft; view.commit = commit
        view.configure(configuration,frames:frames)
    }
    static func dismantleNSView(_ view: QueueMarqueeView, coordinator: ()) { view.detach() }
}

// Transparent overlay observes only blank-space mouse drags. It never takes hit
// testing from cards, controls, scrollbars or the inspector. Model commit is once,
// on mouse-up; Escape, layout/source changes and locking discard the draft.
final class QueueMarqueeView: NSView {
    var accent = NSColor.controlAccentColor
    var draft: ((Set<String>?) -> Void)?
    var commit: ((Set<String>) -> Void)?
    private var configuration: QueueMarqueeConfiguration?
    private var payload: [String:QueueMarqueeFrame] = [:]
    private var controlFrames: [CGRect] = []
    private var monitor: Any?
    private var keyObserver: NSObjectProtocol?
    private var resizeObserver: NSObjectProtocol?
    private var scrollObserver: NSObjectProtocol?
    private weak var scroll: NSScrollView?
    private var allowedClip: CGRect?
    private var timer: Timer?
    private var anchor: CGPoint?
    private var pointer = CGPoint.zero
    private var down = CGPoint.zero
    private var downWindowSize: CGSize?
    private var ownsMouseSequence = false
    private var mode = QueueMarqueeMode.replace
    private var started = false
    private var documentFrames: [String:CGRect] = [:]
    private var drawn: CGRect?
    private var lastDraft: Set<String>?
    private var publication = 0
    #if UI_DIAGNOSTICS
    var marqueeDiagnostics: [String:Any] {
        func rect(_ value: CGRect) -> [String:CGFloat] { ["x":value.minX,"y":value.minY,"width":value.width,"height":value.height] }
        return ["active":started,"pending":anchor != nil,"enabled":configuration?.enabled ?? false,
            "frames":payload.mapValues { rect($0.rect) },"viewport":rect(viewport),
            "scrollFound":scroll != nil,"scrollOffset":offset.y,"cachedFrames":documentFrames.count,
            "draft":lastDraft.map { $0.sorted() } ?? []]
    }
    #endif
    override var isFlipped: Bool { true }
    override var isOpaque: Bool { false }
    override func hitTest(_ point: NSPoint) -> NSView? { nil }
    override init(frame frameRect: NSRect) {
        super.init(frame:frameRect)
        identifier = NSUserInterfaceItemIdentifier("queue-marquee-area")
        setAccessibilityElement(false); wantsLayer = true
    }
    required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }
    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow(); detach()
        guard let window = window else { return }
        monitor = NSEvent.addLocalMonitorForEvents(matching:[.leftMouseDown,.leftMouseDragged,.leftMouseUp,.keyDown,.scrollWheel]) { [weak self] event in
            guard let self = self else { return event }
            return self.handle(event)
        }
        keyObserver = NotificationCenter.default.addObserver(forName:NSWindow.didResignKeyNotification,object:window,queue:.main) { [weak self] _ in self?.cancel() }
        resizeObserver = NotificationCenter.default.addObserver(forName:NSWindow.didResizeNotification,object:window,queue:.main) { [weak self] _ in self?.cancel() }
    }
    override func setFrameSize(_ newSize: NSSize) {
        if frame.size != newSize { cancel() }
        super.setFrameSize(newSize)
    }
    deinit { detach() }
    func detach() {
        cancel()
        if let monitor = monitor { NSEvent.removeMonitor(monitor) }; monitor = nil
        if let keyObserver = keyObserver { NotificationCenter.default.removeObserver(keyObserver) }; keyObserver = nil
        if let resizeObserver = resizeObserver { NotificationCenter.default.removeObserver(resizeObserver) }; resizeObserver = nil
        if let scrollObserver = scrollObserver { NotificationCenter.default.removeObserver(scrollObserver) }; scrollObserver = nil
        scroll = nil
    }
    func configure(_ next: QueueMarqueeConfiguration, frames: [String:QueueMarqueeFrame]) {
        if configuration != next { cancel(); configuration = next }
        controlFrames = frames.filter { $0.key.hasPrefix("control:") && $0.value.layout == next.layout }.values.map {
            $0.rect.offsetBy(dx:-next.layout.origin.x,dy:-next.layout.origin.y)
        }
        let eligible = Set(next.layout.ids)
        let fresh = frames.filter { $0.value.layout == next.layout && eligible.contains($0.key) }.mapValues {
            QueueMarqueeFrame(layout:$0.layout,rect:$0.rect.offsetBy(dx:-next.layout.origin.x,dy:-next.layout.origin.y))
        }
        if payload != fresh {
            payload = fresh
            if anchor != nil { ingestFrames() }
        }
    }
    private func publish(_ ids: Set<String>?) {
        if lastDraft == ids { return }
        lastDraft = ids; publication += 1
        let revision = publication
        DispatchQueue.main.async { [weak self] in
            guard let self = self, self.publication == revision else { return }
            self.draft?(ids)
        }
    }
    func cancel() {
        let wasDragging = anchor != nil
        anchor = nil; started = false; drawn = nil; documentFrames = [:]
        timer?.invalidate(); timer = nil; allowedClip = nil
        if wasDragging { publish(nil); needsDisplay = true }
    }
    private func findScroll() -> NSScrollView? {
        guard window != nil else { return nil }
        func descendants(_ view: NSView) -> [NSScrollView] {
            (view as? NSScrollView).map { [$0] } ?? view.subviews.flatMap(descendants)
        }
        var ancestor = superview
        while let node = ancestor {
            let candidates = descendants(node).filter { $0.window === window && !$0.isHiddenOrHasHiddenAncestor }
            if let best = candidates.max(by:{ score($0) < score($1) }), score(best) > bounds.width*bounds.height*0.4 { return best }
            ancestor = node.superview
        }
        return nil
    }
    private func score(_ candidate: NSScrollView) -> CGFloat {
        let rect = convert(candidate.contentView.bounds,from:candidate.contentView).intersection(bounds)
        return rect.isNull ? 0 : rect.width*rect.height
    }
    private func bindScroll(_ next: NSScrollView) {
        guard scroll !== next else { return }
        if let scrollObserver = scrollObserver { NotificationCenter.default.removeObserver(scrollObserver) }
        scroll = next; next.contentView.postsBoundsChangedNotifications = true
        scrollObserver = NotificationCenter.default.addObserver(forName:NSView.boundsDidChangeNotification,object:next.contentView,queue:.main) { [weak self] _ in
            guard let self = self, self.anchor != nil, let allowed = self.allowedClip, let actual = self.scroll?.contentView.bounds else { return }
            if abs(actual.minX-allowed.minX) > 1 || abs(actual.minY-allowed.minY) > 1 || actual.size != allowed.size { self.cancel() }
        }
    }
    private var viewport: CGRect {
        guard let scroll = scroll, let window = window, scroll.window === window else { return .zero }
        return convert(scroll.contentView.bounds,from:scroll.contentView).intersection(bounds)
    }
    private var offset: CGPoint {
        guard let scroll = scroll, let window = window, scroll.window === window, let document = scroll.documentView else { return .zero }
        let visible = scroll.documentVisibleRect
        return CGPoint(x:visible.minX,y:document.isFlipped ? visible.minY : document.bounds.maxY-visible.maxY)
    }
    private func clamped(_ point: CGPoint) -> CGPoint {
        let area = viewport
        return CGPoint(x:max(area.minX,min(point.x,area.maxX)),y:max(area.minY,min(point.y,area.maxY)))
    }
    private func documentPoint(_ point: CGPoint) -> CGPoint {
        let p = clamped(point), scroll = offset
        return CGPoint(x:p.x+scroll.x,y:p.y+scroll.y)
    }
    private func ingestFrames() {
        let origin = offset
        for (id,value) in payload {
            // Virtualized offscreen cells can report provisional positions.
            // Cache them only once they actually intersect the clip viewport.
            guard value.rect.intersects(viewport) else { continue }
            let rect = value.rect.offsetBy(dx:origin.x,dy:origin.y)
            if let old = documentFrames[id], abs(old.minX-rect.minX) > 1 || abs(old.minY-rect.minY) > 1 || old.size != rect.size {
                cancel(); return
            }
            documentFrames[id] = rect
        }
    }
    private func control(at event: NSEvent) -> Bool {
        guard let root = window?.contentView else { return true }
        var view = root.hitTest(root.convert(event.locationInWindow,from:nil))
        while let candidate = view {
            if candidate is NSControl && !(candidate is NSTableView) { return true }
            view = candidate.superview
        }
        return false
    }
    private func handle(_ event: NSEvent) -> NSEvent? {
        if event.type == .leftMouseDown { ownsMouseSequence = false }
        if event.type == .leftMouseUp && ownsMouseSequence && anchor == nil {
            ownsMouseSequence = false; return nil
        }
        if event.type == .keyDown, anchor != nil {
            let wasActive = started; cancel()
            return event.keyCode == 53 || wasActive ? nil : event
        }
        if event.type == .scrollWheel { cancel(); return event }
        guard event.window === window else { cancel(); return event }
        let point = convert(event.locationInWindow,from:nil)
        switch event.type {
        case .leftMouseDown:
            cancel()
            guard let configuration = configuration, configuration.enabled,
                  window?.isKeyWindow == true, NSApp.isActive,
                  !event.modifierFlags.contains(.control), !event.modifierFlags.contains(.option),
                  let scroll = findScroll(), !payload.isEmpty else { return event }
            bindScroll(scroll)
            guard viewport.contains(point), !controlFrames.contains(where:{ $0.contains(point) }), !control(at:event), !payload.values.contains(where:{ $0.rect.contains(point) }) else { return event }
            down = point; pointer = point; mode = QueueMarqueeMode(event.modifierFlags)
            downWindowSize = window?.frame.size; ownsMouseSequence = true
            anchor = documentPoint(point); allowedClip = scroll.contentView.bounds
            documentFrames = [:]; ingestFrames()
            // Own blank-space sequences so NSWindow background dragging and
            // NSTableView selection cannot race the conversion check marks.
            return nil
        case .leftMouseDragged:
            guard anchor != nil else { return ownsMouseSequence ? nil : event }
            guard window?.frame.size == downWindowSize else { cancel(); return nil }
            pointer = point
            let dx = point.x-down.x, dy = point.y-down.y
            if !started && dx*dx+dy*dy < 25 { return event }
            if !started {
                started = true
                let timer = Timer(timeInterval:1.0/30.0,repeats:true) { [weak self] _ in self?.autoScroll() }
                self.timer = timer; RunLoop.main.add(timer,forMode:.common)
            }
            updateDraft(); return nil
        case .leftMouseUp:
            guard anchor != nil else { return event }
            ownsMouseSequence = false
            guard started, window?.frame.size == downWindowSize else { cancel(); return nil }
            pointer = point; updateDraft()
            let result = lastDraft
            let allowed = configuration?.enabled == true
            cancel()
            if allowed, let result = result { commit?(result) }
            return nil
        default: return event
        }
    }
    private func updateDraft() {
        guard let anchor = anchor, let configuration = configuration, configuration.enabled else { cancel(); return }
        let rect = QueueMarqueeMath.rectangle(anchor,documentPoint(pointer))
        let origin = offset
        drawn = rect.offsetBy(dx:-origin.x,dy:-origin.y); needsDisplay = true
        let ids = QueueMarqueeMath.selection(base:configuration.selected,eligible:Set(configuration.layout.ids),
            hits:QueueMarqueeMath.hits(rect,frames:documentFrames),mode:mode)
        publish(ids)
    }
    private func autoScroll() {
        guard started, let scroll = scroll, let document = scroll.documentView else { return }
        let step = QueueMarqueeMath.scrollStep(y:pointer.y,viewport:viewport)
        guard step != 0 else { return }
        let clip = scroll.contentView, current = offset.y
        let limit = max(0,document.bounds.height-clip.bounds.height)
        let target = max(0,min(current+step,limit))
        guard target != current else { return }
        let y = document.isFlipped ? target : document.bounds.maxY-clip.bounds.height-target
        let next = CGRect(origin:CGPoint(x:clip.bounds.minX,y:y),size:clip.bounds.size)
        allowedClip = next; clip.scroll(to:next.origin); scroll.reflectScrolledClipView(clip)
        if anchor != nil { updateDraft() }
    }
    override func draw(_ dirtyRect: NSRect) {
        guard let rect = drawn, !viewport.isEmpty else { return }
        NSGraphicsContext.saveGraphicsState(); defer { NSGraphicsContext.restoreGraphicsState() }
        NSBezierPath(rect:viewport).addClip()
        accent.withAlphaComponent(0.14).setFill(); NSBezierPath(rect:rect).fill()
        accent.setStroke(); let border = NSBezierPath(rect:rect); border.lineWidth = 1; border.stroke()
    }
}
// MARK: - End queue marquee selection


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

// Standalone imports may be prominent; the workbench keeps processing as its primary action.
struct SourceMenuButton: NSViewRepresentable {
    @Binding var requested: Bool
    let disabled: Bool
    var title = "添加照片"
    var details = ""
    var hasSources = false
    var prominentWhenEmpty = true
    var allowsLibrary = true
    let addFiles: () -> Void
    let addLibrary: () -> Void

    func makeCoordinator() -> Coordinator { Coordinator() }
    func makeNSView(context: Context) -> NSButton {
        let button = NSButton(title:AppText.text("添加照片"),target:context.coordinator,action:#selector(Coordinator.openMenu(_:)))
        button.bezelStyle = .rounded
        button.controlSize = .regular
        button.font = .systemFont(ofSize:NSFont.systemFontSize)
        button.bezelColor = InterfaceStyle.nativeAccent
        button.contentTintColor = InterfaceStyle.nativeAccentForeground
        button.attributedTitle = NSAttributedString(string: AppText.text("添加照片"), attributes: [
            .foregroundColor: InterfaceStyle.nativeAccentForeground,
            .font: NSFont.systemFont(ofSize: NSFont.systemFontSize)
        ])
        button.image = NSImage(systemSymbolName:"plus",accessibilityDescription:nil)?
            .withSymbolConfiguration(.init(paletteColors: [InterfaceStyle.nativeAccentForeground]))
        button.imagePosition = .imageLeading
        button.identifier = NSUserInterfaceItemIdentifier("import-button")
        button.setAccessibilityIdentifier("import-button")
        button.setAccessibilityLabel(AppText.text("添加照片"))
        button.toolTip = AppText.text("添加照片、文件夹或系统照片图库")
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
            let item = NSMenuItem(title:AppText.text(title),action:action,keyEquivalent:"")
            item.target = context.coordinator
            item.image = NSImage(systemSymbolName:symbol,accessibilityDescription:nil)
            context.coordinator.menu.addItem(item)
        }
        return button
    }
    func updateNSView(_ button: NSButton, context: Context) {
        button.isEnabled = !disabled
        button.title = AppText.text(title)
        let prominent = prominentWhenEmpty && !hasSources
        let foreground = prominent ? InterfaceStyle.nativeAccentForeground : NSColor.controlTextColor
        button.bezelColor = prominent ? InterfaceStyle.nativeAccent : nil
        button.contentTintColor = foreground
        button.image = NSImage(systemSymbolName:"plus",accessibilityDescription:nil)?
            .withSymbolConfiguration(.init(paletteColors:[foreground]))
        let paragraph = NSMutableParagraphStyle(); paragraph.lineBreakMode = .byTruncatingMiddle
        button.attributedTitle = NSAttributedString(string:AppText.text(title),attributes:[
            .foregroundColor:foreground,
            .font:NSFont.systemFont(ofSize:NSFont.systemFontSize),.paragraphStyle:paragraph
        ])
        button.toolTip = AppText.text(details.isEmpty ? "添加照片、文件夹或系统照片图库" : details + "\n点击继续添加照片、文件夹或图库照片")
        button.setAccessibilityLabel(AppText.text(hasSources ? title + "，继续添加照片" : "添加照片"))
        context.coordinator.menu.items[0].title = AppText.text(hasSources ? "继续添加照片或文件夹…" : "选择照片或文件夹…")
        context.coordinator.menu.items[1].isHidden = !allowsLibrary
        context.coordinator.menu.items[1].title = AppText.text(hasSources ? "继续从系统图库添加…" : "扫描系统照片图库…")
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
        arrow.imagePosition = .imageOnly; arrow.setAccessibilityLabel(AppText.text("更改保存方式"))
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
        coordinator.primary.title = AppText.text(title)
        coordinator.primary.image = NSImage(systemSymbolName:symbol,accessibilityDescription:nil)
        coordinator.primary.toolTip = AppText.text(details)
        coordinator.primary.setAccessibilityLabel(AppText.text(title + "，更改输出位置"))
        coordinator.primary.isEnabled = !disabled; coordinator.arrow.isEnabled = !disabled
        coordinator.arrow.toolTip = AppText.text("更改保存方式、文件夹或目标相册")
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
                let title = config.chooseDestination == nil ? destination.title : destination == .photos ? "相册" : destination == .folderAndPhotos ? "相册和文件夹" : "文件夹"
                let item = NSMenuItem(title:AppText.text(title),action:#selector(selectDestination(_:)),keyEquivalent:"")
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
            let item = NSMenuItem(title:AppText.text(title),action:action,keyEquivalent:""); item.target = self; menu.addItem(item)
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
    var prominentWhenEmpty = true
    var allowsLibrary = true
    let addFiles: () -> Void
    let addLibrary: () -> Void
    var body: some View {
        Menu {
            Button(AppText.text("选择照片或文件夹…"),action:addFiles)
            if allowsLibrary { Button(AppText.text("扫描系统照片图库…"),action:addLibrary) }
        } label: { Label(AppText.text(title),systemImage:"plus") }
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
            Picker(AppText.text("保存方式"),selection:$destination) { ForEach(destinations,id:\.self) { Text(AppText.text($0.title)).tag($0) } }
            Button(AppText.text("更改输出位置…"),action:changeLocation)
        } label: { Label(AppText.text(title),systemImage:symbol).lineLimit(1) }
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
    var chooseFolder: () -> Void = {}
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
    var exportResults: () -> Void = {}
    var chooseUnconvertedOutput: () -> Void = {}
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
        case .converted, .resumed, .copied: return .green
        default: return .secondary
        }
    }
}

struct PhotoStatusLabel: View {
    let state: PhotoState
    var body: some View {
        Label(AppText.text(state.title), systemImage: state.symbol)
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
    func subtleAnimation<Value: Equatable>(_ value: Value, duration: Double = 0.16) -> some View {
        modifier(SubtleAnimation(value:value,duration:duration))
    }
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

enum InterfaceMotion {
    static func reduced(_ system: Bool) -> Bool {
        #if UI_DIAGNOSTICS
        return system || CommandLine.arguments.contains("--ui-reduce-motion")
        #else
        return system
        #endif
    }
}

private struct SubtleAnimation<Value: Equatable>: ViewModifier {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    let value: Value
    let duration: Double
    func body(content: Content) -> some View {
        content.animation(InterfaceMotion.reduced(reduceMotion) ? nil : .easeInOut(duration:duration),value:value)
    }
}

// Width is measured inside the queue, after the inspector has taken its space.
struct QueueLayout {
    let width: CGFloat
    var twoLines: Bool { width < 560 }
    var showsCover: Bool { width >= 800 }
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
