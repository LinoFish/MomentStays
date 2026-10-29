import AppKit
import SwiftUI

final class WorkspaceWindow: NSWindow {
    override var canBecomeKey: Bool { true }
    override var canBecomeMain: Bool { true }
}

// macOS host: shared UI and task state live in SwiftUI/model files.
final class AppDelegate: NSObject, NSApplicationDelegate, NSWindowDelegate, NSMenuItemValidation, NSMenuDelegate {
    let model = WorkspaceModel()
    private(set) var window: NSWindow!
    private lazy var platform = MacPlatformServices(model: model)
    private var pendingOpen: [URL] = []
    #if UI_DIAGNOSTICS
    private var diagnostics: UISnapshotSupport?
    #endif

    func applicationDidFinishLaunching(_ notification: Notification) {
        if startupArgument("--direction") == "to-android" { model.updateSettings { $0.direction = .toAndroid } }
        if startupArgument("--direction") == "android-to-android" { model.updateSettings { $0.direction = .androidToAndroid } }
        buildMenu()
        AppIconAppearance.shared.start()
        window = WorkspaceWindow(contentRect: NSRect(x: 0, y: 0, width: 1120, height: 700),
            styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView], backing: .buffered, defer: false)
        for kind: NSWindow.ButtonType in [.closeButton,.miniaturizeButton,.zoomButton] {
            window.standardWindowButton(kind)?.isHidden = false
        }
        window.titleVisibility = .hidden
        window.titlebarAppearsTransparent = true
        let brand = NSTitlebarAccessoryViewController()
        brand.view = NSHostingView(rootView:MomentStaysBrandIcon().frame(width:20,height:20).accessibilityHidden(true))
        brand.view.frame = NSRect(x:0,y:0,width:28,height:24)
        brand.layoutAttribute = .right
        window.addTitlebarAccessoryViewController(brand)
        window.isMovableByWindowBackground = true
        window.title = AppName.display; window.isReleasedWhenClosed = false
        window.delegate = self; platform.window = window
        var root = WorkspaceView(model: model, actions: platform.actions)
        #if UI_DIAGNOSTICS
        let diagnostics = UISnapshotSupport(model: model, window: window)
        self.diagnostics = diagnostics
        root.layoutObserver = { [weak diagnostics] frames in diagnostics?.frames = frames }
        diagnostics.prepare()
        #endif
        window.contentView = NSHostingView(rootView:root.frame(minWidth:WindowSizing.workspace.width,minHeight:WindowSizing.workspace.height))
        window.contentMinSize = WindowSizing.workspace
        window.center(); window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        #if UI_DIAGNOSTICS
        diagnostics.start()
        #endif
        if !pendingOpen.isEmpty { platform.openSources(pendingOpen); pendingOpen = [] }
        else if let path = startupArgument("--open-source") { platform.openSources([URL(fileURLWithPath: path)]) }
    }

    private func startupArgument(_ flag: String) -> String? {
        guard let index = CommandLine.arguments.firstIndex(of: flag), index + 1 < CommandLine.arguments.count else { return nil }
        return CommandLine.arguments[index + 1]
    }

    private func buildMenu() {
        let menu = NSMenu()
        func section(_ title: String) -> NSMenu {
            let item = NSMenuItem(); menu.addItem(item)
            let child = NSMenu(title: title); item.submenu = child; return child
        }
        func command(_ menu: NSMenu, _ title: String, _ action: Selector, _ key: String = "") {
            menu.addItem(withTitle: title, action: action, keyEquivalent: key).target = self
        }
        let app = section(AppName.display)
        command(app, "关于 \(AppName.display)", #selector(about))
        app.addItem(.separator())
        app.addItem(withTitle: "隐藏 \(AppName.display)", action: #selector(NSApplication.hide(_:)), keyEquivalent: "h")
        app.addItem(.separator())
        app.addItem(withTitle: "退出 \(AppName.display)", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        let file = section("文件")
        file.delegate = self
        command(file, "选择照片或文件夹…", #selector(chooseSources), "o")
        command(file, "扫描系统照片图库…", #selector(importPhotos))
        command(file, "更改输出位置…", #selector(chooseOutput))
        command(file, "共享输出文件…", #selector(shareResults))
        command(file, "最近结果与交付…", #selector(recentResults))
        command(file, "停止图库自动监测", #selector(stopMonitoring))
        file.addItem(.separator())
        command(file, "打开输出文件夹", #selector(openOutput))
        command(file, "查看转换报告", #selector(showReport))
        file.addItem(.separator())
        file.addItem(withTitle: "关闭窗口", action: #selector(NSWindow.performClose(_:)), keyEquivalent: "w")
        let edit = section("编辑")
        edit.addItem(withTitle: "撤销", action: Selector(("undo:")), keyEquivalent: "z")
        edit.addItem(withTitle: "剪切", action: #selector(NSText.cut(_:)), keyEquivalent: "x")
        edit.addItem(withTitle: "拷贝", action: #selector(NSText.copy(_:)), keyEquivalent: "c")
        edit.addItem(withTitle: "粘贴", action: #selector(NSText.paste(_:)), keyEquivalent: "v")
        edit.addItem(withTitle: "全选", action: #selector(NSText.selectAll(_:)), keyEquivalent: "a")
        let task = section("任务")
        command(task, "开始转换", #selector(start), "\r")
        command(task, "停止任务", #selector(stop), ".")
        command(task, "重新扫描", #selector(rescan), "r")
        command(task, "导出设置…", #selector(showSettings), ",")
        let windowMenu = section("窗口")
        windowMenu.addItem(withTitle: "最小化", action: #selector(NSWindow.performMiniaturize(_:)), keyEquivalent: "m")
        windowMenu.addItem(withTitle: "缩放", action: #selector(NSWindow.performZoom(_:)), keyEquivalent: "")
        NSApp.windowsMenu = windowMenu
        let help = section("帮助")
        command(help, UserGuide.title(for: .current), #selector(showHelp), "?")
        NSApp.mainMenu = menu
    }

    func menuNeedsUpdate(_ menu: NSMenu) {
        for item in menu.items where item.action == #selector(importPhotos) || item.action == #selector(stopMonitoring) {
            item.isHidden = model.settings.direction == .androidToAndroid
        }
    }

    func validateMenuItem(_ item: NSMenuItem) -> Bool {
        switch item.action {
        case #selector(start): return model.canStart && !model.sharing && window?.attachedSheet == nil
        case #selector(stop): return model.canStop
        case #selector(rescan): return model.canRescan
        case #selector(importPhotos): return model.settings.direction != .androidToAndroid && !model.isBusy && !model.sharing && !model.isPreview && window?.attachedSheet == nil
        case #selector(chooseSources): return model.canAppendSources && window?.attachedSheet == nil
        case #selector(chooseOutput): return !model.isBusy && !model.sharing && !model.isPreview && window?.attachedSheet == nil
        case #selector(shareResults): return model.shareReady && !model.isBusy && !model.sharing
        case #selector(recentResults): return !model.isBusy && !model.sharing
        case #selector(stopMonitoring): return model.libraryMonitoring
        case #selector(showReport): return model.report != nil
        case #selector(openOutput): return model.canOpenOutput
        case #selector(showSettings): return window?.attachedSheet == nil
        default: return true
        }
    }

    @objc private func shareResults() { platform.shareResults() }
    @objc private func recentResults() { platform.showRecentTasks() }
    @objc private func stopMonitoring() { platform.stopMonitoring() }
    @objc private func chooseSources() { model.sourceMenuRequested = true }
    @objc private func chooseOutput() { platform.chooseOutput() }
    @objc private func importPhotos() { platform.importPhotos() }
    @objc private func start() { platform.start() }
    @objc private func stop() { model.stop() }
    @objc private func rescan() { model.rescan() }
    @objc private func openOutput() { platform.openOutput() }
    @objc private func showReport() { platform.showReport() }
    @objc private func showSettings() { model.settingsPresented = true }
    @objc private func showHelp() {
        NSWorkspace.shared.open(UserGuide.url(for: .current))
    }
    @objc private func about() {
        guard window.attachedSheet == nil else { return }
        let alert = NSAlert(); alert.messageText = AppName.display
        alert.informativeText = "Android 动态照片 ↔ Apple Live Photo\n\n原生 SwiftUI 界面，适用于 macOS 12 或更新版本。\n转换在本机完成，原片只读。\n图库中的 iCloud 原件可能需要下载。"
        alert.addButton(withTitle: "好"); alert.beginSheetModal(for: window)
    }

    func application(_ sender: NSApplication, openFiles filenames: [String]) {
        guard !filenames.isEmpty, model.canAppendSources, window?.attachedSheet == nil else { sender.reply(toOpenOrPrint: .failure); return }
        let urls = filenames.map { URL(fileURLWithPath: $0) }
        if window == nil { pendingOpen.append(contentsOf: urls) } else { platform.openSources(urls) }
        sender.reply(toOpenOrPrint: .success)
    }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        window?.makeKeyAndOrderFront(nil); return true
    }
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { !model.isBusy }
    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        #if UI_DIAGNOSTICS
        if diagnostics?.finished == true { return .terminateNow }
        #endif
        if model.isBusy { model.stop(); return .terminateCancel }
        if model.sharing { model.errorMessage = "共享应用仍在读取文件，请完成或取消共享后退出。"; return .terminateCancel }
        return .terminateNow
    }
    func windowShouldClose(_ sender: NSWindow) -> Bool {
        if model.isBusy { model.stop(); return false }
        if model.sharing { return false }
        return true
    }
    func applicationWillTerminate(_ notification: Notification) { platform.cleanup() }
}

@main
struct MotionLiveApplication {
    static func main() {
        let application = NSApplication.shared
        application.setActivationPolicy(.regular)
        let delegate = AppDelegate(); application.delegate = delegate
        withExtendedLifetime(delegate) { application.run() }
    }
}
