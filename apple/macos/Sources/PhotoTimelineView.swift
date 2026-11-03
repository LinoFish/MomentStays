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
    @Published var coverRevision = 0
    @Published var candidateStatus = "正在读取封面…" {
        didSet {
            #if UI_DIAGNOSTICS
            MotionPreviewDiagnostics.candidateStatus = candidateStatus
            #endif
        }
    }
    private(set) var originalCoverTime: Double?
    private var defaultCandidateStatus = "正在读取封面…"
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
            duration = 3; coverTime = 0.4; originalCoverTime = 0.4; message = "示意时间线，没有实际视频"
            defaultCandidateStatus = "示意照片，不含高清封面"; candidateStatus = defaultCandidateStatus
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
                    self.defaultCandidateStatus = session.candidateIssue ?? (session.candidates.isEmpty ? "未找到其他高清封面" : "更换封面不影响动态视频")
                    self.candidateStatus = self.defaultCandidateStatus
                    #if UI_DIAGNOSTICS
                    MotionPreviewDiagnostics.duration = session.duration; MotionPreviewDiagnostics.cover = session.coverTime
                    MotionPreviewDiagnostics.candidateCount = session.candidates.count
                    #endif
                    self.message = session.coverIssue ?? "移动指针预览；橙色标记为封面"
                    self.selectCover(self.selectedCover)
                case .failure(let error): self.message = error.localizedDescription; self.candidateStatus = error.localizedDescription
                }
            }
        }
    }

    func selectCover(_ index: Int?) {
        hover(nil)
        selectedCover = index; candidateGeneration = UUID()
        #if UI_DIAGNOSTICS
        MotionPreviewDiagnostics.candidateImageLoaded = false
        #endif
        guard let index = index else {
            candidateImage = nil; coverRevision += 1
            coverTime = originalCoverTime; candidateStatus = defaultCandidateStatus
            #if UI_DIAGNOSTICS
            MotionPreviewDiagnostics.cover = coverTime
            #endif
            return
        }
        guard let session = session else { return }
        let choice = candidates.first { $0.index == index }
        coverTime = choice?.time
        #if UI_DIAGNOSTICS
        MotionPreviewDiagnostics.cover = coverTime
        MotionPreviewDiagnostics.candidateImageLoaded = false
        #endif
        guard let choice = choice, choice.issue == nil else {
            candidateImage = nil; coverRevision += 1
            candidateStatus = choice?.issue ?? "此封面不可用，请另选封面"
            return
        }
        candidateStatus = "正在读取高清封面…"
        let id = generation, candidateID = candidateGeneration
        Self.renderer.async { [weak self] in
            let result = Result { try session.candidateFrame(index) }
            DispatchQueue.main.async {
                guard let self = self, self.generation == id, self.candidateGeneration == candidateID else { return }
                switch result {
                case .success(let image):
                    self.candidateImage = image; self.coverRevision += 1; self.candidateStatus = "已更换封面，动态视频不变"
                    #if UI_DIAGNOSTICS
                    MotionPreviewDiagnostics.candidateImageLoaded = true
                    #endif
                case .failure(let error):
                    self.candidateImage = nil; self.coverRevision += 1; self.candidateStatus = error.localizedDescription
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
        candidateStatus = "正在读取封面…"
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
    private var key: String { item.id + "|" + direction + "|" + timeDomain + "|" + (item.row.version?.manualSignature ?? "") }
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
                ZStack {
                    if let cover = controller.candidateImage {
                        previewImage(cover).id(controller.coverRevision).transition(.opacity)
                    }
                }
                .subtleAnimation(controller.coverRevision,duration:0.12)
                if let frame = controller.frame { previewImage(frame).transaction { $0.animation = nil } }
                HStack {
                    Text(item.isMotion ? "动态照片" : item.row.confirmedStatic ? "静态照片" : "需检查")
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

    private func previewImage(_ image: CGImage) -> some View {
        GeometryReader { geometry in
            Image(decorative:image,scale:1).resizable().scaledToFill()
                .frame(width:geometry.size.width,height:geometry.size.height).clipped()
        }.accessibilityHidden(true)
    }

    private func chooseMarker(_ marker: CoverTimelineMarker) {
        guard allowsCoverSelection, marker.issue == nil else { return }
        controller.hover(nil)
        chooseCover(marker.index)
    }

    @ViewBuilder private func markerControl(_ group: CoverTimelineGroup) -> some View {
        let current = group.markers.first { $0.index == selectedCover }
        let title = (current ?? group.markers[0]).shortTitle + (group.markers.count > 1 ? " +\(group.markers.count - 1)" : "")
        let selected = current != nil
        Group {
            if group.markers.count == 1, let marker = group.markers.first {
                Button { chooseMarker(marker) } label: { markerLabel(title,selected:selected) }
                    .buttonStyle(.plain)
                    .disabled(!allowsCoverSelection || marker.issue != nil)
                    .help(marker.issue ?? marker.title)
                    .accessibilityLabel(marker.title)
            } else {
                Menu {
                    ForEach(group.markers) { marker in
                        Button { chooseMarker(marker) } label: {
                            Label(marker.title,systemImage:marker.index == selectedCover ? "checkmark" : "photo")
                        }.disabled(marker.issue != nil).help(marker.issue ?? marker.title)
                    }
                } label: { markerLabel(title,selected:selected) }
                    .menuStyle(.borderlessButton).menuIndicator(.hidden)
                    .disabled(!allowsCoverSelection)
                    .help(group.markers.map(\.title).joined(separator:"\n"))
                    .accessibilityLabel("相邻封面，" + group.markers.map(\.title).joined(separator:"，"))
            }
        }
        .accessibilityValue(selected ? "当前封面" : "未选择")
        .accessibilityIdentifier("cover-timeline-" + group.id)
    }

    private func markerLabel(_ title: String, selected: Bool) -> some View {
        Text(title).font(.system(size:11,weight:selected ? .semibold : .medium)).lineLimit(1)
            .padding(.horizontal,7).frame(height:24)
            .foregroundColor(selected ? .black : .primary)
            .background(selected ? InterfaceStyle.accent : InterfaceStyle.content)
            .clipShape(Capsule())
            .overlay(Capsule().stroke(selected ? InterfaceStyle.accent : InterfaceStyle.separator,lineWidth:1))
            .subtleAnimation(selected,duration:0.12)
            .frame(width:CoverTimelineGroup.controlWidth,height:28).contentShape(Rectangle())
    }

    private var timeline: some View {
        VStack(spacing: 0) {
            GeometryReader { geometry in
                ZStack(alignment: .leading) {
                    Capsule().fill(Color.secondary.opacity(0.2)).frame(height:3)
                        .padding(.horizontal,timelineInset(geometry.size.width)).offset(y:direction == "to-apple" ? 10 : 0)
                    if let duration = controller.duration, duration > 0 {
                        if let cover = controller.coverTime {
                            RoundedRectangle(cornerRadius: 1).fill(InterfaceStyle.accent)
                                .frame(width: 3, height: 13)
                                .offset(x:timelineX(cover / duration,width:geometry.size.width) - 1.5,y:direction == "to-apple" ? 10 : 0)
                        }
                        if let fraction = controller.hoverFraction {
                            Circle().fill(InterfaceStyle.accent).frame(width: 8, height: 8)
                                .offset(x:timelineX(fraction,width:geometry.size.width) - 4,y:direction == "to-apple" ? 10 : 0)
                        }
                        #if os(macOS)
                        TimelineHoverArea { controller.hover($0.map(Double.init)) }
                            .frame(height:18).padding(.horizontal,timelineInset(geometry.size.width))
                            .offset(y:direction == "to-apple" ? 10 : 0)
                        #else
                        Color.clear.contentShape(Rectangle())
                            .gesture(DragGesture(minimumDistance: 0).onChanged { value in
                                controller.hover(Double(value.location.x / max(1, geometry.size.width)))
                            }.onEnded { _ in controller.hover(nil) })
                        #endif
                        if direction == "to-apple" {
                            ForEach(CoverTimelineGroup.layout(original:controller.originalCoverTime,candidates:controller.candidates,
                                duration:duration,width:geometry.size.width)) { group in
                                markerControl(group).position(x:group.x,y:14)
                            }
                        }
                    }
                }.frame(maxHeight: .infinity)
            }.frame(height:direction == "to-apple" ? 44 : 20)
            HStack(spacing: 4) {
                if let cover = controller.coverTime { Text(String(format: "封面 %.2f s", cover)) }
                else { Text(controller.duration == nil ? "视频预览不可用" : "封面无法定位") }
                Spacer(minLength: 0)
                if let duration = controller.duration { Text(String(format: "%.2f s", duration)) }
            }
            .font(.system(size: 11)).foregroundColor(.secondary).monospacedDigit().lineLimit(1)
            .accessibilityElement(children:.ignore)
            .accessibilityLabel("原始动态照片时间线")
            .accessibilityValue(accessibilityValue)
            .accessibilityAdjustableAction { adjustment in adjustPreview(adjustment) }
            .focusable()
            .onMoveCommand { direction in
                if direction == .left { adjustPreview(.decrement) }
                else if direction == .right { adjustPreview(.increment) }
            }
        }
        .help(controller.message)
    }

    private func timelineInset(_ width: CGFloat) -> CGFloat {
        direction == "to-apple" ? min(CoverTimelineGroup.controlWidth / 2,width / 2) : 0
    }

    private func timelineX(_ fraction: Double, width: CGFloat) -> CGFloat {
        let inset = timelineInset(width)
        return inset + CGFloat(min(1,max(0,fraction))) * max(0,width - 2 * inset)
    }

    private func adjustPreview(_ adjustment: AccessibilityAdjustmentDirection) {
            guard let duration = controller.duration, duration > 0 else { return }
            let current = controller.hoverFraction ?? (controller.coverTime ?? 0) / duration
            switch adjustment {
            case .increment: controller.hover(current + 0.05)
            case .decrement: controller.hover(current - 0.05)
            @unknown default: break
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
        override func mouseDown(with event: NSEvent) { update(event) }
        override func mouseDragged(with event: NSEvent) { update(event) }
        override func mouseExited(with event: NSEvent) { move?(nil) }
    }
}
#endif

private struct ManualVideoFrame: Decodable {
    let frame_index: Int
    let presented_us: Int
    let media_us: Int
}
private struct ManualFrameManifest: Decodable {
    let source_sha256: String
    let source_version: String
    let track_index: Int
    let frames: [ManualVideoFrame]
}
private final class ManualFrameSession {
    let manifest: ManualFrameManifest
    let folder: URL
    let generator: AVAssetImageGenerator
    init(source: URL) throws {
        folder = FileManager.default.temporaryDirectory.appendingPathComponent("MomentStays-manual-" + UUID().uuidString)
        try FileManager.default.createDirectory(at:folder,withIntermediateDirectories:false)
        do {
            let movie = folder.appendingPathComponent("source.mp4")
            let value = try RustCore.engine(["mode":"manual-cover-frames","input":source.path,"output":movie.path])
            manifest = try JSONDecoder().decode(ManualFrameManifest.self,from:JSONSerialization.data(withJSONObject:value))
            guard !manifest.frames.isEmpty else { throw Failure("没有可选择的主视频帧") }
            generator = AVAssetImageGenerator(asset:AVURLAsset(url:movie))
            generator.appliesPreferredTrackTransform = true
            generator.maximumSize = CGSize(width:1000,height:700)
            generator.requestedTimeToleranceBefore = CMTime(value:2,timescale:1_000_000)
            generator.requestedTimeToleranceAfter = CMTime(value:2,timescale:1_000_000)
        } catch { try? FileManager.default.removeItem(at:folder); throw error }
    }
    deinit { try? FileManager.default.removeItem(at:folder) }
    func image(at index: Int) throws -> CGImage {
        let time = manifest.frames[index].presented_us
        var actual = CMTime.invalid
        // Seek just inside the explicitly selected sample: truncating fractional
        // PTS to microseconds can otherwise display the preceding frame in AVF.
        // This seek offset is never used for the output marker. Validate against
        // all real sample times so an adjacent frame can never be confirmed.
        let image = try generator.copyCGImage(at:CMTime(value:Int64(time) + 1,timescale:1_000_000),actualTime:&actual)
        let actualUS = CMTimeGetSeconds(actual) * 1e6
        let nearest = manifest.frames.indices.min { abs(Double(manifest.frames[$0].presented_us) - actualUS) < abs(Double(manifest.frames[$1].presented_us) - actualUS) }
        guard actual.isNumeric, nearest == index, abs(actualUS - Double(time)) <= 3 else {
            throw Failure("预览未落在所选真实帧，请重试；未确认其他画面")
        }
        return image
    }
}
private final class ManualFrameController: ObservableObject {
    @Published var image: CGImage?
    @Published private(set) var isLoading = false
    @Published var message = "正在读取主视频采样表…"
    @Published var position = 0
    @Published private(set) var count = 0
    @Published private(set) var renderedPosition: Int?
    private var session: ManualFrameSession?
    private var generation = UUID()
    private var renderGeneration = UUID()
    private let queue = DispatchQueue(label:"MomentStays.manual-frames",qos:.userInitiated)
    func load(_ item: PhotoQueueItem, confirmed: ManualCoverChoice?) {
        cancel()
        isLoading = true; message = "正在读取主视频采样表…"
        let id = generation
        queue.async { [weak self] in
            let result = Result { try ManualFrameSession(source:item.source) }
            DispatchQueue.main.async {
                guard let self = self, self.generation == id else { return }
                guard SourceVersion.read(item.source) == item.row.version else {
                    self.isLoading = false; self.message = "来源发生变化，请关闭此窗口并重新扫描"
                    return
                }
                switch result {
                case .success(let session):
                    self.session = session; self.count = session.manifest.frames.count
                    let matching = confirmed.flatMap { choice in
                        choice.source_sha256 == session.manifest.source_sha256 && choice.source_version == session.manifest.source_version ? session.manifest.frames.firstIndex(where:{ $0.frame_index == choice.frame_index }) : nil
                    }
                    self.move(to:matching ?? 0)
                case .failure(let error): self.isLoading = false; self.message = error.localizedDescription
                }
            }
        }
    }
    func move(to value: Int) {
        guard let session = session, session.manifest.frames.indices.contains(value) else { return }
        position = value; renderedPosition = nil; image = nil; isLoading = true
        let id = generation, renderID = UUID(); renderGeneration = renderID
        let frame = session.manifest.frames[value]
        message = String(format:"第 %d 帧 · 播放时间 %.6f s（真实 PTS 映射）",frame.frame_index + 1,Double(frame.presented_us)/1e6)
        queue.async { [weak self] in
            let result = Result { try session.image(at:value) }
            DispatchQueue.main.async {
                guard let self = self, self.generation == id, self.renderGeneration == renderID else { return }
                self.isLoading = false
                switch result {
                case .success(let image): self.image = image; self.renderedPosition = value
                case .failure(let error): self.message = error.localizedDescription
                }
            }
        }
    }
    var choice: ManualCoverChoice? {
        guard let session = session, renderedPosition == position else { return nil }
        return ManualCoverChoice(source_sha256:session.manifest.source_sha256,source_version:session.manifest.source_version,
            track_index:session.manifest.track_index,frame_index:session.manifest.frames[position].frame_index)
    }
    func cancel() { generation = UUID(); renderGeneration = UUID(); session = nil; count = 0; image = nil; renderedPosition = nil; isLoading = false }
}

struct ManualMainFrameEditor: View {
    let item: PhotoQueueItem
    let confirmed: ManualCoverChoice?
    let confirm: (ManualCoverChoice) -> Void
    let cancel: () -> Void
    @StateObject private var controller = ManualFrameController()
    var body: some View {
        VStack(alignment:.leading,spacing:12) {
            Text("人工定位动态封面").font(.headline)
            Text(item.name).lineLimit(1).truncationMode(.middle)
            Text("原片封面时间未确定。逐帧选择动态标记，原相机静态照片保持不变；取消不会确认任何帧。")
                .font(.callout).foregroundColor(.secondary)
            ZStack {
                Color.black
                if let image = controller.image { Image(decorative:image,scale:1).resizable().scaledToFit() }
                else if controller.isLoading { ProgressView() }
                else {
                    VStack(spacing:10) {
                        Label("无法显示所选画面",systemImage:"exclamationmark.triangle")
                        Button("重试") {
                            if controller.count > 0 { controller.move(to:controller.position) }
                            else { controller.load(item,confirmed:confirmed) }
                        }
                    }.foregroundColor(.white)
                }
            }.frame(height:280).accessibilityLabel("主视频所选画面")
            if controller.count > 0 {
                Slider(value:Binding(get:{ Double(controller.position) },set:{ controller.move(to:Int($0.rounded())) }),
                    in:0...Double(max(1,controller.count - 1)),step:1)
                    .accessibilityLabel("逐帧选择主视频").accessibilityIdentifier("manual-frame-slider")
                HStack {
                    Button("上一帧") { controller.move(to:controller.position - 1) }.disabled(controller.position == 0)
                    Button("下一帧") { controller.move(to:controller.position + 1) }.disabled(controller.position >= controller.count - 1)
                }
            }
            Text(controller.message).font(.callout).monospacedDigit().textSelection(.enabled)
            HStack {
                Button("取消",action:cancel).keyboardShortcut(.cancelAction)
                Spacer()
                Button("确认此帧（保留原静态照片）") { if let choice = controller.choice { confirm(choice) } }
                    .disabled(controller.choice == nil).keyboardShortcut(.defaultAction)
                    .accessibilityIdentifier("confirm-manual-frame")
            }
        }.padding(20).frame(width:560)
            .onAppear { controller.load(item,confirmed:confirmed) }
            .onDisappear { controller.cancel() }
    }
}
