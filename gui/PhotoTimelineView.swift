import SwiftUI
import AVFoundation
#if os(macOS)
import AppKit
#endif

private final class MotionPreviewController: ObservableObject {
    @Published var duration: Double?
    @Published var coverTime: Double?
    @Published var message = "正在读取视频…"
    @Published var frame: CGImage?
    @Published var frameTime: Double?
    @Published var hoverFraction: Double?
    @Published var candidates: [PreviewCoverCandidate] = []
    @Published var candidateImage: CGImage?
    @Published var candidateStatus = "正在读取高清候选…"
    private var originalCoverTime: Double?
    private var selectedCover: Int?
    private var candidateGeneration = UUID()
    private var session: MotionPreviewSession?
    private var token = StopToken()
    private var generation = UUID()
    private var scheduled = false
    private var rendering = false
    private static let loader = DispatchQueue(label: "MomentStays.preview-prepare", qos: .utility)
    private static let renderer = DispatchQueue(label: "MomentStays.preview-frames", qos: .userInitiated)

    func prepare(source: URL, direction: String, domain: String, demo: Bool, selectedCover: Int?) {
        cancel()
        self.selectedCover = selectedCover
        duration = nil; coverTime = nil; message = "正在读取视频…"
        if demo {
            duration = 3; coverTime = 0.4; message = "示意时间线，没有实际视频"
            return
        }
        let id = generation, workToken = token
        Self.loader.async { [weak self] in
            let result = Result { try MotionPreviewSession.prepare(source: source, direction: direction, timeDomain: domain, token: workToken) }
            DispatchQueue.main.async {
                guard let self = self, self.generation == id, !workToken.isCancelled else { return }
                switch result {
                case .success(let session):
                    self.session = session; self.duration = session.duration; self.coverTime = session.coverTime
                    self.originalCoverTime = session.coverTime; self.candidates = session.candidates
                    self.candidateStatus = session.candidateIssue ?? (session.candidates.isEmpty ? "原片没有辅助高清候选" : "封面使用所选照片，动态保持主运动轨")
                    #if UI_DIAGNOSTICS
                    MotionPreviewDiagnostics.duration = session.duration; MotionPreviewDiagnostics.cover = session.coverTime
                    MotionPreviewDiagnostics.candidateCount = session.candidates.count
                    #endif
                    self.message = session.coverIssue ?? "悬浮预览原始动态主轨；琥珀标记为封面位置"
                    self.selectCover(self.selectedCover)
                case .failure(let error): self.message = error.localizedDescription; self.candidateStatus = error.localizedDescription
                }
            }
        }
    }

    func selectCover(_ index: Int?) {
        selectedCover = index; candidateImage = nil; candidateGeneration = UUID()
        guard let index = index else { coverTime = originalCoverTime; return }
        guard let session = session else { return }
        let choice = candidates.first { $0.index == index }
        coverTime = choice?.time
        #if UI_DIAGNOSTICS
        MotionPreviewDiagnostics.cover = coverTime
        MotionPreviewDiagnostics.candidateImageLoaded = false
        #endif
        guard let choice = choice, choice.issue == nil else {
            candidateStatus = choice?.issue ?? "所选高清候选不存在；请选择原始照片或其他候选"
            return
        }
        let id = generation, candidateID = candidateGeneration
        Self.renderer.async { [weak self] in
            let result = Result { try session.candidateFrame(index) }
            DispatchQueue.main.async {
                guard let self = self, self.generation == id, self.candidateGeneration == candidateID else { return }
                switch result {
                case .success(let image):
                    self.candidateImage = image; self.candidateStatus = "已选择高清候选；动态仍使用主运动轨"
                    #if UI_DIAGNOSTICS
                    MotionPreviewDiagnostics.candidateImageLoaded = true
                    #endif
                case .failure(let error): self.candidateStatus = error.localizedDescription
                }
            }
        }
    }

    func hover(_ fraction: Double?) {
        hoverFraction = fraction.map { min(1, max(0, $0)) }
        #if UI_DIAGNOSTICS
        MotionPreviewDiagnostics.hovering = fraction != nil
        if fraction == nil { MotionPreviewDiagnostics.frame = nil }
        #endif
        if fraction == nil { frame = nil; frameTime = nil; return }
        requestFrame()
    }

    private func requestFrame() {
        guard !scheduled, !rendering, session != nil, hoverFraction != nil else { return }
        scheduled = true
        let id = generation
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) { [weak self] in
            guard let self = self, self.generation == id else { return }
            self.scheduled = false
            guard let session = self.session, let fraction = self.hoverFraction else { return }
            self.rendering = true
            Self.renderer.async { [weak self] in
                let result = Result { try session.frame(at: fraction) }
                DispatchQueue.main.async {
                    guard let self = self, self.generation == id else { return }
                    self.rendering = false
                    guard let current = self.hoverFraction else { return }
                    switch result {
                    case .success(let value):
                        self.frame = value.image; self.frameTime = value.time
                        #if UI_DIAGNOSTICS
                        MotionPreviewDiagnostics.frame = value.time
                        #endif
                    case .failure: self.frame = nil; self.frameTime = nil; self.message = "此位置无法提取预览画面"
                    }
                    let previousIndex = MotionPreviewSession.frameIndex(fraction: fraction, duration: session.duration, times: session.frameTimes)
                    let currentIndex = MotionPreviewSession.frameIndex(fraction: current, duration: session.duration, times: session.frameTimes)
                    if previousIndex != currentIndex { self.requestFrame() }
                }
            }
        }
    }

    func cancel() {
        token.cancel(); token = StopToken(); generation = UUID()
        session = nil; frame = nil; frameTime = nil; hoverFraction = nil
        candidates = []; candidateImage = nil; originalCoverTime = nil; candidateGeneration = UUID()
        candidateStatus = "正在读取高清候选…"
        scheduled = false; rendering = false
    }

    deinit { token.cancel() }
}

struct PhotoMotionPreview: View {
    let item: PhotoQueueItem
    let direction: String
    let timeDomain: String
    let isDemo: Bool
    let height: CGFloat
    let select: () -> Void
    var selectedCover: Int? = nil
    var chooseCover: (Int?) -> Void = { _ in }
    var allowsCoverSelection = false
    var showsCandidateStatus = false
    @StateObject private var controller = MotionPreviewController()
    private var key: String { item.id + "|" + direction + "|" + timeDomain }
    private var previewSource: URL {
        #if UI_DIAGNOSTICS
        return MotionPreviewDiagnostics.source ?? item.source
        #else
        return item.source
        #endif
    }
    private var usesDemo: Bool {
        #if UI_DIAGNOSTICS
        return isDemo && MotionPreviewDiagnostics.source == nil
        #else
        return isDemo
        #endif
    }

    var body: some View {
        VStack(spacing: 0) {
            ZStack(alignment: .topLeading) {
                PhotoThumbnail(source: previewSource, isPreview: usesDemo)
                if let frame = controller.frame ?? controller.candidateImage {
                    GeometryReader { geometry in
                        Image(decorative: frame, scale: 1).resizable().scaledToFill()
                            .frame(width: geometry.size.width, height: geometry.size.height).clipped()
                    }.accessibilityHidden(true)
                }
                HStack {
                    Text(item.isMotion ? "动态照片" : "静态照片")
                    Spacer(minLength: 0)
                    if controller.frame != nil, let time = controller.frameTime { Text(String(format: "预览 %.2f s", time)).monospacedDigit() }
                }
                .font(.system(size: 11, weight: .medium)).foregroundColor(.white)
                .padding(.horizontal, 7).padding(.vertical, 4)
                .background(Color.black.opacity(0.65)).cornerRadius(5).padding(8)
            }
            .frame(height: height).clipped().contentShape(Rectangle()).onTapGesture(perform: select)
            if item.isMotion { timeline.padding(.horizontal, 10).padding(.vertical, 6) }
            if item.isMotion && direction == "to-apple" {
                if !controller.candidates.isEmpty || selectedCover != nil { coverMenu.padding(.horizontal,10).padding(.bottom,8) }
                if showsCandidateStatus { Text(controller.candidateStatus).font(.callout).foregroundColor(.secondary).padding(10) }
            }
        }
        .onAppear { prepare() }
        .onChange(of: key) { _ in prepare() }
        .onChange(of: selectedCover) { controller.selectCover($0) }
        .onDisappear { controller.cancel() }
    }

    private func prepare() {
        if item.isMotion { controller.prepare(source: previewSource, direction: direction, domain: timeDomain, demo: usesDemo, selectedCover:selectedCover) }
    }

    private var coverMenu: some View {
        Menu {
            Button { chooseCover(nil) } label: {
                Label("原始照片",systemImage:selectedCover == nil ? "checkmark":"photo")
            }
            ForEach(controller.candidates) { choice in
                Button { chooseCover(choice.index) } label: {
                    Label(choice.title,systemImage:selectedCover == choice.index ? "checkmark":"photo")
                }.disabled(choice.issue != nil).help(choice.issue ?? "使用该高清画面作为导出封面")
            }
        } label: {
            Text(selectedCover.map { "封面：高清候选 \($0 + 1)" } ?? "封面：原始照片").font(.caption)
        }
        .disabled(!allowsCoverSelection)
        .accessibilityIdentifier("cover-candidate-picker")
        .help(controller.candidateStatus)
    }

    private var timeline: some View {
        VStack(spacing: 0) {
            GeometryReader { geometry in
                ZStack(alignment: .leading) {
                    Capsule().fill(Color.secondary.opacity(0.2)).frame(height: 3)
                    if let duration = controller.duration, duration > 0 {
                        if let cover = controller.coverTime {
                            RoundedRectangle(cornerRadius: 1).fill(Color(red: 0.94, green: 0.64, blue: 0.24))
                                .frame(width: 3, height: 13)
                                .offset(x: CGFloat(min(1, max(0, cover / duration))) * max(0, geometry.size.width - 3))
                        }
                        if let fraction = controller.hoverFraction {
                            Circle().fill(Color.accentColor).frame(width: 8, height: 8)
                                .offset(x: CGFloat(fraction) * max(0, geometry.size.width - 8))
                        }
                        #if os(macOS)
                        TimelineHoverArea { controller.hover($0.map(Double.init)) }
                            .frame(maxWidth: .infinity, maxHeight: .infinity)
                        #else
                        Color.clear.contentShape(Rectangle())
                            .gesture(DragGesture(minimumDistance: 0).onChanged { value in
                                controller.hover(Double(value.location.x / max(1, geometry.size.width)))
                            }.onEnded { _ in controller.hover(nil) })
                        #endif
                    }
                }.frame(maxHeight: .infinity)
            }.frame(height: 20)
            HStack(spacing: 4) {
                if let cover = controller.coverTime { Text(String(format: "封面 %.2f s", cover)) }
                else { Text(controller.duration == nil ? "视频预览不可用" : "封面无法定位") }
                Spacer(minLength: 0)
                if let duration = controller.duration { Text(String(format: "%.2f s", duration)) }
            }
            .font(.system(size: 11)).foregroundColor(.secondary).monospacedDigit().lineLimit(1)
        }
        .help(controller.message)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("原始动态照片时间线")
        .accessibilityValue(accessibilityValue)
        .accessibilityAdjustableAction { adjustment in
            guard let duration = controller.duration, duration > 0 else { return }
            let current = controller.hoverFraction ?? (controller.coverTime ?? 0) / duration
            switch adjustment {
            case .increment: controller.hover(current + 0.05)
            case .decrement: controller.hover(current - 0.05)
            @unknown default: break
            }
        }
    }

    private var accessibilityValue: String {
        guard let duration = controller.duration else { return controller.message }
        let cover = controller.coverTime.map { String(format: "封面 %.2f 秒，", $0) } ?? "封面无法定位，"
        let hover = controller.frameTime.map { String(format: "正在预览 %.2f 秒，", $0) } ?? ""
        return hover + cover + String(format: "总时长 %.2f 秒", duration)
    }
}

#if os(macOS)
// onContinuousHover requires macOS 13; an NSTrackingArea keeps scrubbing available on 12.
private struct TimelineHoverArea: NSViewRepresentable {
    let move: (CGFloat?) -> Void
    func makeNSView(context: Context) -> TrackingView {
        let view = TrackingView()
        view.identifier = NSUserInterfaceItemIdentifier("motion-timeline-hover")
        return view
    }
    func updateNSView(_ view: TrackingView, context: Context) { view.move = move }
    static func dismantleNSView(_ view: TrackingView, coordinator: ()) { view.move?(nil); view.move = nil }

    final class TrackingView: NSView {
        var move: ((CGFloat?) -> Void)?
        private var tracking: NSTrackingArea?
        override func viewDidMoveToWindow() { super.viewDidMoveToWindow(); window?.acceptsMouseMovedEvents = true }
        override func updateTrackingAreas() {
            super.updateTrackingAreas()
            if let tracking = tracking { removeTrackingArea(tracking) }
            let area = NSTrackingArea(rect: .zero, options: [.activeInKeyWindow, .inVisibleRect, .mouseEnteredAndExited, .mouseMoved], owner: self)
            addTrackingArea(area); tracking = area
        }
        private func update(_ event: NSEvent) {
            let x = convert(event.locationInWindow, from: nil).x
            move?(min(1, max(0, x / max(1, bounds.width))))
        }
        override func mouseEntered(with event: NSEvent) { update(event) }
        override func mouseMoved(with event: NSEvent) { update(event) }
        override func mouseExited(with event: NSEvent) { move?(nil) }
    }
}
#endif
