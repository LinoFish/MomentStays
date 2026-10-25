#if UI_DIAGNOSTICS
import AppKit
import SwiftUI

// Only compiled for --diagnostics builds. Synthetic data never accesses photos.
final class UISnapshotSupport {
    let model: WorkspaceModel
    let window: NSWindow
    var frames: [String: CGRect] = [:]
    private(set) var finished = false
    private var checks: [[String: Any]] = []
    private var reportWindow: NSWindow?
    init(model: WorkspaceModel, window: NSWindow) { self.model = model; self.window = window }
    private func argument(_ flag: String) -> String? {
        guard let index = CommandLine.arguments.firstIndex(of: flag), index + 1 < CommandLine.arguments.count else { return nil }
        return CommandLine.arguments[index + 1]
    }
    func prepare() {
        if let appearance = argument("--ui-appearance") { NSApp.appearance = NSAppearance(named: appearance == "dark" ? .darkAqua : .aqua) }
        if argument("--direction") == "to-android" { model.updateSettings { $0.direction = .toAndroid } }
        guard argument("--ui-snapshot") != nil else { return }
        model.loadPreview(count: Int(argument("--ui-fixtures") ?? "24") ?? 24,
            progressPreview: CommandLine.arguments.contains("--ui-progress-preview"))
        if let source = argument("--ui-motion-source") {
            MotionPreviewDiagnostics.source = URL(fileURLWithPath: source)
            model.loadPreview(count: 1, progressPreview: false)
        }
        if let destination = argument("--ui-destination"), let value = OutputDestination(rawValue: destination) {
            model.updateSettings { $0.destination = value }
            model.deliverySummary = "转换结果已保留，可在最近结果中重试交付"
            model.recentTasksAvailable = true; model.shareReady = model.settings.direction == .toAndroid
            model.libraryMonitoring = true; model.librarySummary = "监测所选相册（运行期间）"
        }
        if CommandLine.arguments.contains("--ui-select-first") { model.selection = model.items.first?.id }
        if CommandLine.arguments.contains("--ui-detail") { model.detailExpanded = true }
    }
    func start() {
        guard let path = argument("--ui-snapshot") else { return }
        let width = Double(argument("--ui-width") ?? "1120") ?? 1120
        let height = Double(argument("--ui-height") ?? "700") ?? 700
        resize(width: width, height: height)
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.7) {
            if CommandLine.arguments.contains("--ui-error-preview") { self.model.errorMessage = "示例错误：照片缺少有效封面时间。原片保持不变，可在高级参数中检查时间策略。" }
            if CommandLine.arguments.contains("--ui-report-preview") { self.showReportPreview() }
            if CommandLine.arguments.contains("--ui-expand-settings") { self.model.settingsPresented = true }
            if CommandLine.arguments.contains("--ui-resize-check") {
                self.model.selection = self.model.items.first?.id
                self.model.updateSettings { $0.exportDataFiles = false }
                self.resizeSequence([(390, 540), (640, 600), (959, 640), (960, 640), (1280, 540), (480, 600)], path: path)
            } else if CommandLine.arguments.contains("--ui-hover-preview") { self.hoverPreview(path: path) }
            else { self.finishSnapshot(path) }
        }
    }
    private func hoverPreview(path: String, remaining: Int = 60, sent: Bool = false) {
        if MotionPreviewDiagnostics.duration != nil, let content = window.contentView,
           let target = descendants(content).first(where: { $0.identifier?.rawValue == "motion-timeline-hover" }) {
            if !sent {
                let point = target.convert(NSPoint(x: target.bounds.width * 0.8, y: target.bounds.midY), to: nil)
                if let event = NSEvent.mouseEvent(with: .mouseMoved, location: point, modifierFlags: [], timestamp: 0,
                    windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: 0, pressure: 0) {
                    target.mouseMoved(with: event)
                }
            }
            if MotionPreviewDiagnostics.frame != nil {
                if CommandLine.arguments.contains("--ui-hover-exit") {
                    target.mouseExited(with: NSEvent())
                }
                finishSnapshot(path); return
            }
            if remaining > 0 {
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) { self.hoverPreview(path: path, remaining: remaining - 1, sent: true) }
                return
            }
        } else if remaining > 0 {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) { self.hoverPreview(path: path, remaining: remaining - 1) }
            return
        }
        finishSnapshot(path)
    }

    private func resize(width: Double, height: Double) {
        var frame = window.frame
        frame.size = NSSize(width: max(390, width), height: max(540, height))
        window.setFrame(frame, display: true)
    }
    private func resizeSequence(_ sizes: [(Double, Double)], path: String) {
        guard let next = sizes.first else { finishSnapshot(path); return }
        resize(width: next.0, height: next.1)
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) {
            var check = self.metrics()
            check["selection_preserved"] = self.model.selection == self.model.items.first?.id
            check["settings_preserved"] = !self.model.settings.exportDataFiles
            self.checks.append(check)
            self.resizeSequence(Array(sizes.dropFirst()), path: path)
        }
    }
    private func metrics() -> [String: Any] {
        let bounds = window.contentView?.bounds ?? .zero
        let visible = CGRect(origin: .zero, size: bounds.size)
        let queue = frames["queue-list"] ?? .zero
        let primary = frames["primary-action"] ?? .zero
        let tables = window.contentView.map { descendants($0).compactMap { $0 as? NSTableView } } ?? []
        return ["frame_width": window.frame.width, "frame_height": window.frame.height,
            "content_height": bounds.height, "queue_height": queue.height,
            "queue_width": queue.width, "primary_visible": !primary.isEmpty && visible.contains(primary),
            "queue_visible": !queue.isEmpty && visible.contains(queue),
            "preview_duration": MotionPreviewDiagnostics.duration.map { $0 as Any } ?? NSNull(),
            "preview_cover": MotionPreviewDiagnostics.cover.map { $0 as Any } ?? NSNull(),
            "preview_frame": MotionPreviewDiagnostics.frame.map { $0 as Any } ?? NSNull(),
            "preview_hovering": MotionPreviewDiagnostics.hovering,
            "row_count": model.items.count, "inspector_visible": frames["inspector"] != nil,
            "settings_sheet_visible": window.attachedSheet != nil,
            "native_tables": tables.map { ["rows": $0.numberOfRows, "selected_row": $0.selectedRow,
                "visible_rows": $0.rows(in: $0.visibleRect).length,
                "horizontal_overflow": $0.bounds.width > ($0.enclosingScrollView?.contentSize.width ?? $0.bounds.width) + 1] as [String: Any] },
            "frames": frames.filter { !$0.key.isEmpty }.mapValues { ["x": $0.minX, "y": $0.minY, "width": $0.width, "height": $0.height] }]
    }
    private func descendants(_ view: NSView) -> [NSView] {
        [view] + view.subviews.flatMap { descendants($0) }
    }
    private func showReportPreview() {
        let files: [[String: Any]] = model.items.prefix(12).enumerated().map { index, item in
            ["source": item.source.path, "status": index % 3 == 0 ? "failed" : "converted",
             "detail": index % 3 == 0 ? "原片缺少有效封面时间；未发布输出，原始文件保持不变。" : "转换完成，图像与视频已通过校验。"]
        }
        let report = ConversionReport(BatchOutcome(counts: ["converted": 8, "failed": 4], report: nil,
            reportData: ["files": files], cancelled: false))
        let panel = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 390, height: 500), styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
        panel.title = "转换报告"; panel.isReleasedWhenClosed = false
        panel.contentView = NSHostingView(rootView: ReportView(report: report, actions: WorkspaceActions()))
        panel.center(); panel.makeKeyAndOrderFront(nil); reportWindow = panel
    }
    private func finishSnapshot(_ path: String) {
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
            let target = self.reportWindow ?? self.window.attachedSheet ?? self.window
            target.contentView?.layoutSubtreeIfNeeded()
            if let capture = CGWindowListCreateImage(.null, .optionIncludingWindow, CGWindowID(target.windowNumber), [.boundsIgnoreFraming, .bestResolution]),
               let png = NSBitmapImageRep(cgImage: capture).representation(using: .png, properties: [:]) {
                try? png.write(to: URL(fileURLWithPath: path))
            }
            var result = self.metrics(); result["resize_checks"] = self.checks
            if let data = try? JSONSerialization.data(withJSONObject: result, options: [.prettyPrinted, .sortedKeys]) {
                try? data.write(to: URL(fileURLWithPath: path + ".json"))
            }
            self.finished = true
            self.model.settingsPresented = false
            if let sheet = self.window.attachedSheet {
                self.window.endSheet(sheet); sheet.orderOut(nil)
            }
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) { NSApp.terminate(nil) }
            // A diagnostic run has only synthetic data. A lingering modal animation
            // must not leave an unattended preview process running after capture.
            DispatchQueue.main.asyncAfter(deadline: .now() + 1) { exit(0) }
        }
    }
}
#endif
