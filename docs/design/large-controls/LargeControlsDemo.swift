import AppKit
import SwiftUI

private final class DemoState: ObservableObject {
    @Published var comfortable = true
    @Published var reverse = false
    @Published var destination = "系统照片图库"
    @Published var format = "自动 · 保留兼容格式"
    @Published var scope = "全部照片"
    @Published var count = 6
    @Published var selected = 0
    @Published var running = false
    @Published var progress = 0.0
    @Published var report = false
    @Published var advanced = false
    @Published var keepReport = true
    @Published var message = "6 张示意照片已就绪"
    private var run = UUID()

    func convert() {
        if running { run = UUID(); running = false; message = "演示已停止"; return }
        run = UUID(); let id = run
        running = true; progress = 0
        tick(id)
    }
    private func tick(_ id: UUID) {
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) { [weak self] in
            guard let self = self, self.run == id, self.running else { return }
            self.progress = min(1, self.progress + 0.1)
            self.message = "正在演示转换 · \(Int(self.progress * 100))%"
            if self.progress >= 0.99 { self.running = false; self.message = "演示完成 · 没有转换实际文件" }
            else { self.tick(id) }
        }
    }
}

private struct FrameKey: PreferenceKey {
    static var defaultValue: [String: CGRect] = [:]
    static func reduce(value: inout [String: CGRect], nextValue: () -> [String: CGRect]) { value.merge(nextValue(), uniquingKeysWith: { _, new in new }) }
}
private extension View {
    func measured(_ name: String) -> some View {
        background(GeometryReader { geometry in Color.clear.preference(key: FrameKey.self, value: [name: geometry.frame(in: .named("demo"))]) })
    }
}
private struct Brand: View {
    @Environment(\.colorScheme) private var scheme
    var body: some View {
        if let image = Bundle.main.image(forResource: NSImage.Name(scheme == .dark ? "AppIconDark" : "AppIcon")) {
            Image(nsImage: image).renderingMode(.original).resizable().scaledToFit()
        }
    }
}

private struct DemoView: View {
    @ObservedObject var state: DemoState
    var frames: ([String: CGRect]) -> Void
    private var gap: CGFloat { state.comfortable ? 20 : 12 }
    private var labelHeight: CGFloat { state.comfortable ? 30 : 16 }

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 12) {
                Brand().frame(width: 34, height: 34)
                Text("MomentStays").font(.system(size: 17, weight: .semibold))
                Text("大控件预览").foregroundColor(.secondary).font(.system(size: 13))
                Spacer()
                Text("控件大小").foregroundColor(.secondary).font(.system(size: 13))
                Picker("控件大小", selection: $state.comfortable) {
                    Text("常规").tag(false)
                    Text("大控件").tag(true)
                }.pickerStyle(.segmented).labelsHidden().frame(width: 190).controlSize(.regular)
                    .accessibilityIdentifier("density-toggle")
            }.padding(.horizontal, 22).padding(.vertical, 12)
            Divider()
            HStack(spacing: 0) {
                sidebar.frame(width: 184)
                Divider()
                photos.frame(maxWidth: .infinity, maxHeight: .infinity)
                Divider()
                inspector.frame(width: 310)
            }.frame(maxWidth: .infinity, maxHeight: .infinity)
            Divider()
            footer
        }
        .font(.system(size: state.comfortable ? 15 : 13))
        .controlSize(state.comfortable ? .large : .regular)
        .background(Color(nsColor: .windowBackgroundColor))
        .coordinateSpace(name: "demo")
        .onPreferenceChange(FrameKey.self, perform: frames)
        .sheet(isPresented: $state.report) {
            VStack(alignment: .leading, spacing: 20) {
                Text("交互预览 Demo").font(.title2.weight(.semibold))
                Text("本窗口使用原生 SwiftUI 控件和代码绘制的示意照片。可以切换控件大小、转换方向、交付位置，调整示意时间条并演示进度。")
                Text("没有读取照片、修改图库或执行实际转换。").foregroundColor(.secondary)
                HStack { Spacer(); Button("完成") { state.report = false }.keyboardShortcut(.defaultAction) }
            }.padding(28).frame(width: 460)
        }
    }

    private var sidebar: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("工作空间").font(.system(size: 12)).foregroundColor(.secondary).padding(.horizontal, 12).padding(.bottom, 12)
            ForEach(["全部照片", "已完成", "需处理"], id: \.self) { scope in
                Button { state.scope = scope } label: {
                    HStack(spacing: 9) {
                        Image(systemName: scope == "全部照片" ? "photo.on.rectangle" : scope == "已完成" ? "checkmark.circle" : "exclamationmark.circle")
                        Text(scope)
                        Spacer(minLength: 0)
                        Text(scope == "全部照片" ? "\(state.count)" : "0").foregroundColor(.secondary)
                    }
                    .font(.system(size: state.comfortable ? 15 : 13, weight: state.scope == scope ? .semibold : .regular))
                    .padding(.horizontal, 12).frame(minHeight: state.comfortable ? 48 : 36)
                    .background(state.scope == scope ? Color.accentColor.opacity(0.12) : .clear).cornerRadius(8)
                }.buttonStyle(.plain)
            }
            Spacer()
            Button { state.report = true } label: {
                Label("最近结果", systemImage: "clock.arrow.circlepath").frame(minHeight: labelHeight)
            }.buttonStyle(.bordered).frame(maxWidth: .infinity)
            Label("本机处理", systemImage: "lock.shield").font(.system(size: 12)).foregroundColor(.secondary).padding(.top, 12)
            Text("示意数据，仅用于预览").font(.system(size: 12)).foregroundColor(.secondary)
        }.padding(.horizontal, 12).padding(.vertical, 25)
    }

    private var photos: some View {
        VStack(alignment: .leading, spacing: gap) {
            HStack {
                VStack(alignment: .leading, spacing: 5) {
                    Text("让瞬间继续。").font(.system(size: 27, weight: .semibold))
                    Text("把动态照片带进新的图库。").foregroundColor(.secondary).font(.system(size: 14))
                }
                Spacer(minLength: 8)
                NativeDemoButton(title: "添加照片", symbol: "plus", large: state.comfortable, height: 40) {
                    state.count += 3; state.scope = "全部照片"; state.message = "已添加 3 张示意照片"
                }
                .frame(width: state.comfortable ? 144 : 112, height: state.comfortable ? 40 : 24)
                .disabled(state.running).measured("import")
            }
            HStack(spacing: 8) {
                NativeDemoButton(title: "安卓转苹果实况", symbol: nil, large: state.comfortable, height: 36, prominent: !state.reverse) { state.reverse = false }
                NativeDemoButton(title: "苹果实况转安卓", symbol: nil, large: state.comfortable, height: 36, prominent: state.reverse) { state.reverse = true }
            }
            .frame(height: state.comfortable ? 36 : 24).disabled(state.running).measured("direction")
            HStack(spacing: 8) {
                Image(systemName: "photo.on.rectangle")
                Text("山湖旅行")
                Text("\(state.count) 张示意照片").foregroundColor(.secondary)
                Spacer()
            }.font(.system(size: 13)).foregroundColor(.secondary)
            Divider()
            HStack {
                Text(state.scope).fontWeight(.semibold)
                Spacer()
                Button { state.message = "示意队列已刷新" } label: {
                    Label("重新扫描", systemImage: "arrow.clockwise").frame(minHeight: state.comfortable ? 22 : 14)
                }.buttonStyle(.bordered).disabled(state.running)
            }
            ScrollView {
                if state.scope == "全部照片" {
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 174), spacing: 16)], spacing: 18) {
                        ForEach(0..<state.count, id: \.self) { index in
                            DemoPhoto(index: index, selected: state.selected == index) { state.selected = index }
                        }
                    }.padding(3)
                } else {
                    VStack(spacing: 16) {
                        Brand().frame(width: 58, height: 58)
                        Text("此范围暂无示意照片").foregroundColor(.secondary)
                        Button("显示全部照片") { state.scope = "全部照片" }.buttonStyle(.bordered)
                    }.frame(maxWidth: .infinity).padding(.top, 60)
                }
            }
        }
        .padding(24).background(Color(nsColor: .controlBackgroundColor))
    }

    private var inspector: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: gap) {
                VStack(alignment: .leading, spacing: 5) {
                    Text("交付设置").font(.system(size: 15, weight: .semibold))
                    Text(state.reverse ? "Motion Photo" : "Live Photo").font(.system(size: 22, weight: .semibold))
                }
                Divider()
                field("交付位置") {
                    NativeDemoChoice(selection: $state.destination, options: ["系统照片图库", "文件夹", "系统共享"], large: state.comfortable)
                        .frame(height: state.comfortable ? 36 : 24).measured("destination")
                }
                NativeDemoButton(title: "选择交付位置…", symbol: "folder", large: state.comfortable, height: 36) {
                    state.message = "输出位置为演示内容，不会打开文件或图库"
                }.frame(height: state.comfortable ? 36 : 24)
                field("图片格式") {
                    NativeDemoChoice(selection: $state.format, options: ["自动 · 保留兼容格式", "JPEG"], large: state.comfortable)
                        .frame(height: state.comfortable ? 36 : 24)
                }
                Divider()
                VStack(alignment: .leading, spacing: state.comfortable ? 18 : 10) {
                    Label("保留封面瞬间", systemImage: "checkmark.circle")
                    Label("复用原始音视频", systemImage: "checkmark.circle")
                    Label("本机处理", systemImage: "lock.shield")
                }.foregroundColor(.secondary)
                Text("HDR 按源格式及增益模型处理，兼容性以转换报告为准。")
                    .font(.system(size: 12)).foregroundColor(.secondary).fixedSize(horizontal: false, vertical: true)
                Divider()
                Group {
                Toggle("导出校验报告", isOn: $state.keepReport).toggleStyle(.switch)
                DisclosureGroup("高级参数", isExpanded: $state.advanced) {
                    VStack(alignment: .leading, spacing: 12) {
                        Text("封面时间缺失时报告错误。")
                        Text("当前为布局演示，参数不连接转换引擎。")
                    }.font(.system(size: 13)).foregroundColor(.secondary).padding(.top, 12)
                }
                }
            }.padding(24)
        }.disabled(state.running)
    }

    private func field<Content: View>(_ title: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: state.comfortable ? 10 : 6) {
            Text(title).font(.system(size: state.comfortable ? 14 : 12)).foregroundColor(.secondary)
            content()
        }
    }

    private var footer: some View {
        VStack(spacing: 8) {
            if state.running { ProgressView(value: state.progress).progressViewStyle(.linear) }
            HStack(spacing: 16) {
                Label(state.message, systemImage: state.running ? "arrow.triangle.2.circlepath" : "info.circle")
                    .font(.system(size: 13)).foregroundColor(.secondary).lineLimit(1)
                Spacer(minLength: 0)
                NativeDemoButton(title: "查看报告", symbol: nil, large: state.comfortable, height: 36) { state.report = true }
                    .frame(width: state.comfortable ? 108 : 90, height: state.comfortable ? 36 : 24)
                NativeDemoButton(title: state.running ? "停止演示" : "开始转换", symbol: state.running ? "stop.fill" : "arrow.triangle.2.circlepath",
                    large: state.comfortable, height: 44, prominent: true, defaultAction: true, action: state.convert)
                    .frame(width: state.comfortable ? 176 : 130, height: state.comfortable ? 44 : 24).measured("primary")
            }
        }.padding(.horizontal, 22).padding(.vertical, state.comfortable ? 16 : 10)
    }
}

// Native flexible-height bezels avoid the fixed-height push-button bezel on macOS 12.
private struct NativeDemoButton: NSViewRepresentable {
    let title: String
    let symbol: String?
    let large: Bool
    let height: CGFloat
    var prominent = false
    var defaultAction = false
    let action: () -> Void
    @Environment(\.isEnabled) private var enabled

    func makeCoordinator() -> Coordinator { Coordinator() }
    func makeNSView(context: Context) -> NSButton {
        let button = NSButton(title: title, target: context.coordinator, action: #selector(Coordinator.invokeAction))
        button.setButtonType(.momentaryPushIn)
        button.imagePosition = .imageLeading; button.imageHugsTitle = true
        button.setContentHuggingPriority(.defaultLow, for: .horizontal)
        return button
    }
    func updateNSView(_ button: NSButton, context: Context) {
        context.coordinator.action = action
        button.title = title; button.isEnabled = enabled
        button.bezelStyle = large ? .regularSquare : .rounded
        button.controlSize = large ? .large : .regular
        button.font = .systemFont(ofSize: large ? (height >= 44 ? 16 : 15) : 13, weight: prominent ? .semibold : .medium)
        button.image = symbol.flatMap { NSImage(systemSymbolName: $0, accessibilityDescription: nil) }
        button.symbolConfiguration = NSImage.SymbolConfiguration(pointSize: large ? 15 : 13, weight: .medium)
        button.bezelColor = prominent ? .controlAccentColor : nil
        button.keyEquivalent = defaultAction ? "\r" : ""
        button.invalidateIntrinsicContentSize()
    }
    final class Coordinator: NSObject {
        var action: (() -> Void)?
        @objc func invokeAction() { action?() }
    }
}

private struct NativeDemoChoice: NSViewRepresentable {
    @Binding var selection: String
    let options: [String]
    let large: Bool
    @Environment(\.isEnabled) private var enabled
    func makeCoordinator() -> Coordinator { Coordinator() }
    func makeNSView(context: Context) -> NSPopUpButton {
        let button = NSPopUpButton(frame: .zero, pullsDown: false)
        button.target = context.coordinator; button.action = #selector(Coordinator.select(_:))
        button.setContentHuggingPriority(.defaultLow, for: .horizontal)
        return button
    }
    func updateNSView(_ button: NSPopUpButton, context: Context) {
        context.coordinator.change = { selection = $0 }
        if button.itemTitles != options { button.removeAllItems(); button.addItems(withTitles: options) }
        button.selectItem(withTitle: selection)
        button.controlSize = large ? .large : .regular
        button.bezelStyle = large ? .regularSquare : .rounded
        button.font = .systemFont(ofSize: large ? 15 : 13)
        button.isEnabled = enabled
    }
    final class Coordinator: NSObject {
        var change: ((String) -> Void)?
        @objc func select(_ sender: NSPopUpButton) { if let title = sender.titleOfSelectedItem { change?(title) } }
    }
}

private struct DemoPhoto: View {
    let index: Int
    let selected: Bool
    let select: () -> Void
    @State private var position = 0.4
    @State private var hovering = false
    var body: some View {
        VStack(spacing: 0) {
            Landscape(index: index, position: hovering ? position : 0.4)
                .frame(height: 126).clipped()
                .overlay(alignment: .topLeading) {
                    Text("示意插画").font(.system(size: 11)).padding(5).foregroundColor(.white)
                        .background(Color.black.opacity(0.6)).cornerRadius(4).padding(8)
                }
                .contentShape(Rectangle()).onTapGesture(perform: select)
            VStack(alignment: .leading, spacing: 5) {
                ZStack {
                    Slider(value: $position, in: 0...1).controlSize(.regular)
                    GeometryReader { geometry in
                        Rectangle().fill(Color(red: 240/255, green: 164/255, blue: 60/255)).frame(width: 3, height: 12)
                            .position(x: geometry.size.width * 0.4, y: geometry.size.height / 2)
                    }.allowsHitTesting(false)
                }.onHover { hovering = $0 }
                HStack { Text("封面 0.40 s"); Spacer(); Text("1.00 s") }.font(.system(size: 11)).foregroundColor(.secondary)
                Button(action: select) {
                    HStack { Text("山湖_\(index + 1).jpg").fontWeight(.medium); Spacer(); Text("待转换").foregroundColor(.secondary) }
                        .font(.system(size: 12)).padding(.top, 8).frame(maxWidth: .infinity, alignment: .leading)
                }.buttonStyle(.plain)
            }.padding(10)
        }
        .background(Color(nsColor: .controlBackgroundColor)).cornerRadius(9)
        .overlay(RoundedRectangle(cornerRadius: 9).stroke(selected ? Color.accentColor : Color.secondary.opacity(0.25), lineWidth: selected ? 2 : 1))
    }
}

private struct Landscape: View {
    let index: Int
    let position: Double
    var body: some View {
        Canvas { context, size in
            let skies: [Color] = [Color(red: 0.68, green: 0.81, blue: 0.85), Color(red: 0.84, green: 0.72, blue: 0.62), Color(red: 0.71, green: 0.78, blue: 0.71)]
            context.fill(Path(CGRect(origin: .zero, size: size)), with: .color(skies[index % skies.count]))
            let sun = CGRect(x: size.width * (0.65 + position * 0.12), y: size.height * 0.14, width: 26, height: 26)
            context.fill(Path(ellipseIn: sun), with: .color(Color(red: 1, green: 0.94, blue: 0.76)))
            var mountain = Path()
            mountain.move(to: CGPoint(x: 0, y: size.height * 0.7))
            mountain.addLines([CGPoint(x: size.width * 0.25, y: size.height * 0.2), CGPoint(x: size.width * 0.5, y: size.height * 0.65), CGPoint(x: size.width * 0.72, y: size.height * 0.32), CGPoint(x: size.width, y: size.height * 0.7), CGPoint(x: size.width, y: size.height), CGPoint(x: 0, y: size.height)])
            mountain.closeSubpath()
            context.fill(mountain, with: .color(Color(red: 0.30, green: 0.44, blue: 0.46)))
            let lake = CGRect(x: 0, y: size.height * 0.74, width: size.width, height: size.height * 0.26)
            context.fill(Path(lake), with: .color(Color(red: 0.56, green: 0.72, blue: 0.73)))
        }
    }
}

private final class DemoDelegate: NSObject, NSApplicationDelegate {
    let state = DemoState()
    var window: NSWindow!
    var frames: [String: CGRect] = [:]
    private func argument(_ name: String) -> String? {
        guard let index = CommandLine.arguments.firstIndex(of: name), index + 1 < CommandLine.arguments.count else { return nil }
        return CommandLine.arguments[index + 1]
    }
    func applicationDidFinishLaunching(_ notification: Notification) {
        if CommandLine.arguments.contains("--regular") { state.comfortable = false }
        if CommandLine.arguments.contains("--dark") { NSApp.appearance = NSAppearance(named: .darkAqua) }
        NSApp.applicationIconImage = Bundle.main.image(forResource: "AppIcon")
        let menu = NSMenu(); let item = NSMenuItem(); let appMenu = NSMenu()
        appMenu.addItem(withTitle: "退出预览", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        item.submenu = appMenu; menu.addItem(item); NSApp.mainMenu = menu
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1220, height: 800), styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
        window.title = "MomentStays · 大控件 Demo"; window.isReleasedWhenClosed = false
        window.minSize = NSSize(width: 1100, height: 720)
        window.contentView = NSHostingView(rootView: DemoView(state: state, frames: { [weak self] value in self?.frames = value }))
        window.center(); window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
        if let output = argument("--snapshot") {
            DispatchQueue.main.asyncAfter(deadline: .now() + 1) { self.capture(output) }
        }
    }
    private func capture(_ output: String) {
        if let capture = CGWindowListCreateImage(.null, .optionIncludingWindow, CGWindowID(window.windowNumber), [.boundsIgnoreFraming, .bestResolution]),
           let data = NSBitmapImageRep(cgImage: capture).representation(using: .png, properties: [:]) {
            try? data.write(to: URL(fileURLWithPath: output))
        }
        let metrics: [String: Any] = ["comfortable": state.comfortable, "controls": frames.mapValues { ["width": $0.width, "height": $0.height] }]
        if let data = try? JSONSerialization.data(withJSONObject: metrics, options: [.prettyPrinted, .sortedKeys]) { try? data.write(to: URL(fileURLWithPath: output + ".json")) }
        NSApp.terminate(nil)
    }
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
}

@main
private struct LargeControlsDemo {
    static func main() {
        let app = NSApplication.shared; app.setActivationPolicy(.regular)
        let delegate = DemoDelegate(); app.delegate = delegate
        withExtendedLifetime(delegate) { app.run() }
    }
}
