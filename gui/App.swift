import AppKit
import UniformTypeIdentifiers

final class CanvasView: NSView {
    override var isFlipped: Bool { true }
    override var isOpaque: Bool { true }
    override func draw(_ dirtyRect: NSRect) {
        NSColor.windowBackgroundColor.setFill()
        dirtyRect.fill()
    }
    override func viewDidChangeEffectiveAppearance() { super.viewDidChangeEffectiveAppearance(); needsDisplay = true }
}

final class DropCard: NSView {
    override var wantsUpdateLayer: Bool { true }
    var onDrop: (([URL])->Void)?
    var acceptsDrop = true
    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layer?.cornerRadius = 8
        layer?.borderWidth = 0.5
        registerForDraggedTypes([.fileURL])
    }
    required init?(coder: NSCoder) { fatalError("init(coder:)") }
    override func updateLayer() {
        layer?.backgroundColor = NSColor.controlBackgroundColor.cgColor
        layer?.borderColor = NSColor.separatorColor.withAlphaComponent(0.5).cgColor
    }
    override func draggingEntered(_ sender: NSDraggingInfo) -> NSDragOperation {
        guard acceptsDrop, sender.draggingPasteboard.canReadObject(forClasses:[NSURL.self],options:[.urlReadingFileURLsOnly:true]) else { return [] }
        layer?.borderColor = NSColor.controlAccentColor.cgColor
        return .copy
    }
    override func draggingExited(_ sender: NSDraggingInfo?) { needsDisplay = true }
    override func performDragOperation(_ sender: NSDraggingInfo) -> Bool {
        guard acceptsDrop, let urls = sender.draggingPasteboard.readObjects(forClasses:[NSURL.self],options:[.urlReadingFileURLsOnly:true]) as? [URL],
              !urls.isEmpty else { return false }
        onDrop?(urls); needsDisplay = true
        return true
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate, NSWindowDelegate, NSTableViewDataSource, NSTableViewDelegate {
    var window: NSWindow!
    private let sourcePath = NSTextField(labelWithString:"拖入多张照片或文件夹，也可点击多选")
    private let outputPath = NSTextField(labelWithString:"选择原片文件夹之外的位置")
    private let directionPopup = NSPopUpButton(frame:.zero,pullsDown:false)
    private let photosButton = NSButton(title:"照片图库…",target:nil,action:nil)
    private let vendorPopup = NSPopUpButton(frame:.zero,pullsDown:false)
    private let containerPopup = NSPopUpButton(frame:.zero,pullsDown:false)
    private let imageQuality = NSSlider(value:0.95,minValue:0.01,maxValue:1,target:nil,action:nil)
    private let gainQuality = NSSlider(value:0.95,minValue:0.01,maxValue:1,target:nil,action:nil)
    private let imageQualityText = NSTextField(labelWithString:"95%")
    private let gainQualityText = NSTextField(labelWithString:"95%")
    private let preserveJPEG = NSButton(checkboxWithTitle:"JPEG 原图保留编码",target:nil,action:nil)
    private let hdrTarget = NSTextField(labelWithString:"HDR：Ultra HDR（默认）")
    private var libraryPicker: PhotosLibraryPicker?
    private let settingsToggle = NSButton(title:"导出设置…",target:nil,action:nil)
    private var settingsExpanded = false
    private var compact = false
    private var cardsRow: NSStackView!
    private var headerHeight: NSLayoutConstraint!
    private var ownedPhotoCopies: [URL] = []
    private var cardsHeight: NSLayoutConstraint!
    private var mobileCardWidths: [NSLayoutConstraint] = []
    private var platformLabel: NSView!
    private var secondaryMetrics: [NSView] = []
    private var metricsRow: NSStackView!
    private var metricsHeight: NSLayoutConstraint!
    private var settingsPanel: NSBox!
    private var forwardOptions: NSStackView!
    private var reverseOptions: NSStackView!
    private var commonOptions: NSStackView!
    private var reverseFormatRow: NSStackView!
    private var reverseQualityRow: NSStackView!
    private var footerRow: NSStackView!
    private var footerHeight: NSLayoutConstraint!
    private var footerActions: NSStackView!
    private var desktopActionsWidth: NSLayoutConstraint!
    private var mobileActionsWidth: NSLayoutConstraint!
    private var mobileActivityWidth: NSLayoutConstraint!
    private var optionSpacers: [NSView] = []
    private var buttonHeights: [NSLayoutConstraint] = []
    private var primaryHeight: NSLayoutConstraint!
    private var originalTableHeader: NSTableHeaderView?
    private var isToAndroid: Bool { directionPopup.indexOfSelectedItem == 1 }
    private let sourceButton = NSButton(title:"选择原片…",target:nil,action:nil)
    private let outputButton = NSButton(title:"选择位置…",target:nil,action:nil)
    private let scanButton = NSButton(title:"重新扫描",target:nil,action:nil)
    private let convertButton = NSButton(title:"开始转换",target:nil,action:nil)
    private let stopButton = NSButton(title:"停止",target:nil,action:nil)
    private let openButton = NSButton(title:"打开输出",target:nil,action:nil)
    private let reportButton = NSButton(title:"查看报告",target:nil,action:nil)
    private let advancedButton = NSButton(title:"高级设置…",target:nil,action:nil)
    private let legacy = NSButton(checkboxWithTitle:"尝试兼容非 Ultra HDR",target:nil,action:nil)
    private let exportDataFiles = NSButton(checkboxWithTitle:"导出 JSON 清单和报告",target:nil,action:nil)
    private let missingPopup = NSPopUpButton(frame:.zero,pullsDown:false)
    private let domainPopup = NSPopUpButton(frame:.zero,pullsDown:false)
    private let repairAuxiliary = NSButton(checkboxWithTitle:"独立导出时允许修复辅助轨的零时长",target:nil,action:nil)
    private let trackPopup = NSPopUpButton(frame:.zero,pullsDown:false)
    private let exportPopup = NSPopUpButton(frame:.zero,pullsDown:false)
    private let status = NSTextField(labelWithString:"选择原片后自动扫描")
    private let detail = NSTextField(wrappingLabelWithString:"选择 JPEG/HEIC 或文件夹，批量生成 Live Photo 配对文件。")
    private let progress = NSProgressIndicator()
    private let table = NSTableView()
    private var emptyState: NSView?
    private let emptyLabel = NSTextField(labelWithString:"照片会显示在这里")
    private let totalMetric = NSTextField(labelWithString:"—")
    private let motionMetric = NSTextField(labelWithString:"—")
    private let hdrMetric = NSTextField(labelWithString:"—")
    private let issueMetric = NSTextField(labelWithString:"—")
    private let sourceCard = DropCard(frame:.zero)
    private let outputCard = DropCard(frame:.zero)
    private var sources: [URL] = []
    private var output: URL?
    private var report: URL?
    private var reportData: [String:Any]?
    private var rows: [ScanRow] = []
    private var results: [String:(String,String)] = [:]
    private var token = StopToken()
    private var busy = false
    private var converting = false
    private var advancedPopover: NSPopover?
    private var pendingOpen: [URL] = []
    private var reportWindow: NSWindow?
    private var existingChoiceForBatch: ExistingOutputDecision?
    private let pageScroll = NSScrollView()

    private func label(_ title: String, size: CGFloat = 13, weight: NSFont.Weight = .regular, color: NSColor = .labelColor) -> NSTextField {
        let label = NSTextField(labelWithString:title)
        label.font = .systemFont(ofSize:size,weight:weight); label.textColor = color
        return label
    }
    private func stack(_ views: [NSView], vertical: Bool = true, spacing: CGFloat = 12) -> NSStackView {
        let s = NSStackView(views:views); s.orientation = vertical ? .vertical : .horizontal
        s.alignment = vertical ? .leading : .centerY; s.spacing = spacing
        return s
    }
    private func fill(_ child: NSView, in parent: NSView, inset: CGFloat = 0) {
        child.translatesAutoresizingMaskIntoConstraints = false; parent.addSubview(child)
        NSLayoutConstraint.activate([child.leadingAnchor.constraint(equalTo:parent.leadingAnchor,constant:inset),
            child.trailingAnchor.constraint(equalTo:parent.trailingAnchor,constant:-inset),
            child.topAnchor.constraint(equalTo:parent.topAnchor,constant:inset),child.bottomAnchor.constraint(equalTo:parent.bottomAnchor,constant:-inset)])
    }
    private func flex() -> NSView {
        let v = NSView(); v.setContentHuggingPriority(.init(1),for:.horizontal); return v
    }
    private func bind(_ button: NSButton, _ selector: Selector) { button.target = self; button.action = selector; button.bezelStyle = .rounded }

    func applicationDidFinishLaunching(_ notification: Notification) {
        if let mode = argument("--ui-appearance") { NSApp.appearance = NSAppearance(named:mode == "dark" ? .darkAqua : .aqua) }
        buildMenu()
        window = NSWindow(contentRect:NSRect(x:0,y:0,width:1040,height:640),styleMask:[.titled,.closable,.miniaturizable,.resizable],backing:.buffered,defer:false)
        window.title = "Motion-Live"
        window.titlebarAppearsTransparent = true
        window.minSize = NSSize(width:390,height:540)
        window.delegate = self; window.isReleasedWhenClosed = false
        window.contentView = CanvasView(frame:window.contentView!.bounds)
        window.center()
        buildUI()
        if argument("--direction") == "to-android" { directionPopup.selectItem(at:1); directionChanged() }
        updateAdaptiveLayout()
        if let value = argument("--ui-height"), let height = Double(value), height.isFinite {
            var frame = window.frame; frame.size.height = CGFloat(max(540,min(height,1200)))
            window.setFrame(frame,display:true)
        }
        if let value = argument("--ui-width"), let width = Double(value), width.isFinite {
            var frame = window.frame; frame.size.width = CGFloat(max(390,min(width,1600)))
            window.setFrame(frame,display:true)
        }
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps:true)
        if CommandLine.arguments.contains("--ui-expand-settings") { settingsExpanded = true; updateAdaptiveLayout() }
        if let path = argument("--ui-snapshot") {
            if let source = argument("--open-source") { choose([URL(fileURLWithPath:source)]) }
            waitForSnapshot(path)
        } else if !pendingOpen.isEmpty { choose(pendingOpen); pendingOpen = [] }
        else if let path = argument("--open-source") { choose([URL(fileURLWithPath:path)]) }
    }
    func argument(_ flag: String) -> String? {
        let args = CommandLine.arguments
        guard let i = args.firstIndex(of:flag), i + 1 < args.count else { return nil }
        return args[i+1]
    }
    private func buildMenu() {
        let menu = NSMenu()
        let appItem = NSMenuItem(); menu.addItem(appItem)
        let app = NSMenu(); appItem.submenu = app
        app.addItem(withTitle:"关于 Motion-Live",action:#selector(about),keyEquivalent:"").target = self
        app.addItem(.separator())
        app.addItem(withTitle:"隐藏 Motion-Live",action:#selector(NSApplication.hide(_:)),keyEquivalent:"h")
        app.addItem(.separator())
        app.addItem(withTitle:"退出 Motion-Live",action:#selector(NSApplication.terminate(_:)),keyEquivalent:"q")
        let fileItem = NSMenuItem(); menu.addItem(fileItem)
        let file = NSMenu(title:"文件"); fileItem.submenu = file
        file.addItem(withTitle:"选择原片…",action:#selector(selectSource),keyEquivalent:"o").target = self
        file.addItem(withTitle:"打开输出",action:#selector(openOutput),keyEquivalent:"").target = self
        let editItem = NSMenuItem(); menu.addItem(editItem)
        let edit = NSMenu(title:"编辑"); editItem.submenu = edit
        edit.addItem(withTitle:"拷贝",action:#selector(NSText.copy(_:)),keyEquivalent:"c")
        edit.addItem(withTitle:"全选",action:#selector(NSText.selectAll(_:)),keyEquivalent:"a")
        let helpItem = NSMenuItem(); menu.addItem(helpItem)
        let help = NSMenu(title:"帮助"); helpItem.submenu = help
        help.addItem(withTitle:"使用说明",action:#selector(showHelp),keyEquivalent:"?").target = self
        NSApp.mainMenu = menu
    }
    private func buildUI() {
        guard let root = window.contentView else { return }
        pageScroll.hasVerticalScroller = true; pageScroll.autohidesScrollers = true
        pageScroll.drawsBackground = false
        root.addSubview(pageScroll)
        let content = CanvasView()
        content.translatesAutoresizingMaskIntoConstraints = false
        pageScroll.documentView = content
        let fillHeight = content.heightAnchor.constraint(equalTo:pageScroll.contentView.heightAnchor)
        fillHeight.priority = .init(249)
        NSLayoutConstraint.activate([content.widthAnchor.constraint(equalTo:pageScroll.contentView.widthAnchor),
            content.heightAnchor.constraint(greaterThanOrEqualTo:pageScroll.contentView.heightAnchor),fillHeight])
        let brandIcon = NSImageView(image:NSImage(systemSymbolName:"livephoto",accessibilityDescription:"Motion-Live")!)
        brandIcon.symbolConfiguration = NSImage.SymbolConfiguration(pointSize:22,weight:.medium)
        brandIcon.contentTintColor = .controlAccentColor
        directionPopup.addItems(withTitles:["安卓 → 苹果实况","苹果实况 → 安卓"])
        directionPopup.font = .systemFont(ofSize:12); directionPopup.target = self; directionPopup.action = #selector(directionChanged)
        platformLabel = label("本地处理 · macOS 12+",size:11,color:.secondaryLabelColor)
        let header = stack([brandIcon,label("Motion-Live",size:17,weight:.semibold),directionPopup,flex(),platformLabel],vertical:false,spacing:10)
        headerHeight = header.heightAnchor.constraint(equalToConstant:28); headerHeight.isActive = true

        bind(sourceButton,#selector(selectSource)); bind(outputButton,#selector(selectOutput))
        bind(photosButton,#selector(importPhotos)); bind(settingsToggle,#selector(toggleSettings));
        bind(scanButton,#selector(rescan)); bind(convertButton,#selector(startConversion)); bind(stopButton,#selector(stop))
        bind(openButton,#selector(openOutput)); bind(reportButton,#selector(openReport)); bind(advancedButton,#selector(showAdvanced))
        sourceCard.onDrop = { [weak self] in self?.choose($0) }
        outputCard.onDrop = { [weak self] in if let first = $0.first { self?.setOutput(first) } }
        for button in [sourceButton,outputButton,photosButton,scanButton,openButton,reportButton,advancedButton,stopButton] {
            button.controlSize = .small; button.font = .systemFont(ofSize:12)
        }
        func card(_ view: DropCard, title: String, symbol: String, path: NSTextField, button: NSButton, hint: String) {
            path.font = .systemFont(ofSize:12); path.textColor = .secondaryLabelColor
            path.lineBreakMode = .byTruncatingMiddle
            path.setContentCompressionResistancePriority(.init(100),for:.horizontal)
            let icon = NSImageView(image:NSImage(systemSymbolName:symbol,accessibilityDescription:nil)!)
            icon.contentTintColor = .controlAccentColor
            icon.symbolConfiguration = NSImage.SymbolConfiguration(pointSize:12,weight:.medium)
            let items: [NSView] = view === sourceCard ? [icon,label(title,size:12,weight:.semibold),flex(),photosButton,button] : [icon,label(title,size:12,weight:.semibold),flex(),button]
            let top = stack(items,vertical:false,spacing:6)
            let body = stack([top,path],spacing:3)
            for child in [top,path] { child.translatesAutoresizingMaskIntoConstraints = false; child.widthAnchor.constraint(equalTo:body.widthAnchor).isActive = true }
            view.toolTip = hint
            fill(body,in:view,inset:9)
        }
        card(sourceCard,title:"原始照片",symbol:"photo.on.rectangle",path:sourcePath,button:sourceButton,hint:"支持多选 / 拖入多个 JPEG、HEIC 或文件夹，包含子文件夹。原片只读。")
        card(outputCard,title:"保存位置",symbol:"folder",path:outputPath,button:outputButton,hint:"生成独立配对文件；请选择原片文件夹之外的位置。")
        let cards = stack([sourceCard,outputCard],vertical:false,spacing:10); cards.distribution = .fillEqually
        cardsRow = cards
        cardsHeight = cards.heightAnchor.constraint(equalToConstant:60); cardsHeight.isActive = true
        mobileCardWidths = [sourceCard.widthAnchor.constraint(equalTo:cards.widthAnchor),outputCard.widthAnchor.constraint(equalTo:cards.widthAnchor)]

        func metric(_ value: NSTextField, _ name: String) -> NSView {
            value.font = .monospacedDigitSystemFont(ofSize:12,weight:.semibold)
            return stack([label(name,size:11,color:.secondaryLabelColor),value],vertical:false,spacing:5)
        }
        scanButton.image = NSImage(systemSymbolName:"arrow.clockwise",accessibilityDescription:nil)
        scanButton.imagePosition = .imageLeading
        let hdrMetricView = metric(hdrMetric,"Ultra HDR"), issueMetricView = metric(issueMetric,"缺封面")
        secondaryMetrics = [hdrMetricView,issueMetricView]
        let metrics = stack([label("照片队列",size:12,weight:.semibold),metric(totalMetric,"共"),metric(motionMetric,"动态"),hdrMetricView,issueMetricView,flex(),scanButton],vertical:false,spacing:18)
        metricsRow = metrics; metricsHeight = metrics.heightAnchor.constraint(equalToConstant:24); metricsHeight.isActive = true

        let scroll = NSScrollView()
        scroll.hasVerticalScroller = true; scroll.autohidesScrollers = true
        scroll.borderType = .noBorder; scroll.wantsLayer = true; scroll.layer?.cornerRadius = 8; scroll.layer?.masksToBounds = true
        table.usesAlternatingRowBackgroundColors = true; table.rowHeight = 28
        table.style = .fullWidth; table.allowsMultipleSelection = false
        table.delegate = self; table.dataSource = self
        for (id,title,width) in [("name","照片",320.0),("manufacturer","厂商",100.0),("hdr","图像",150.0),("cover","封面位置",135.0),("status","状态",100.0)] {
            let column = NSTableColumn(identifier:NSUserInterfaceItemIdentifier(id))
            column.title = title; column.width = width; column.minWidth = id == "name" ? 220 : 85
            table.addTableColumn(column)
        }
        table.columnAutoresizingStyle = .noColumnAutoresizing
        originalTableHeader = table.headerView
        scroll.documentView = table
        let tableArea = NSView(); fill(scroll,in:tableArea)
        let emptyIcon = NSImageView(image:NSImage(systemSymbolName:"photo.on.rectangle.angled",accessibilityDescription:nil)!)
        emptyIcon.symbolConfiguration = NSImage.SymbolConfiguration(pointSize:25,weight:.light)
        emptyIcon.contentTintColor = .tertiaryLabelColor
        emptyLabel.font = .systemFont(ofSize:13,weight:.medium); emptyLabel.textColor = .secondaryLabelColor
        let empty = stack([emptyIcon,emptyLabel,label("选择原片，或拖入多张照片与文件夹",size:11,color:.tertiaryLabelColor)],spacing:7)
        empty.alignment = .centerX; empty.translatesAutoresizingMaskIntoConstraints = false
        tableArea.addSubview(empty); emptyState = empty
        NSLayoutConstraint.activate([empty.centerXAnchor.constraint(equalTo:tableArea.centerXAnchor),empty.centerYAnchor.constraint(equalTo:tableArea.centerYAnchor,constant:10)])
        tableArea.heightAnchor.constraint(greaterThanOrEqualToConstant:100).isActive = true
        tableArea.setContentHuggingPriority(.init(1),for:.vertical)
        tableArea.setContentCompressionResistancePriority(.init(1),for:.vertical)
        detail.font = .systemFont(ofSize:11); detail.textColor = .secondaryLabelColor
        detail.maximumNumberOfLines = 1; detail.lineBreakMode = .byTruncatingMiddle; detail.isSelectable = true
        detail.heightAnchor.constraint(equalToConstant:18).isActive = true

        legacy.state = .on
        exportDataFiles.state = .on; exportDataFiles.font = .systemFont(ofSize:12)
        exportDataFiles.toolTip = "JSON 不是导入 Live Photo 的必需文件，用于校验、续转和排错。关闭后本次只导出媒体文件，仍可在应用中查看本次报告；不删除已有 JSON。"
        legacy.font = .systemFont(ofSize:12)
        legacy.toolTip = "只转换有明确参数的已有旧增益图（Apple/ISO）；普通 SDR 不生成 HDR，Ultra HDR 原样保留。"
        missingPopup.addItems(withTitles:["缺少时间时跳过并报错（推荐）","缺少时间时使用视频中点"])
        domainPopup.addItems(withTitles:["自动检查，有歧义时报错（推荐）","使用视频样本时间（media）","使用编辑后的播放时间（presentation）"])
        trackPopup.addItems(withTitles:["主运动轨（默认）","辅助影像轨"])
        exportPopup.addItems(withTitles:["仅 Live Photo","Live Photo ＋ 辅助轨","仅导出辅助轨"])
        trackPopup.toolTip = "选择作为 Live Photo 视频的轨道。辅助轨可能只有一帧，不能代替连续动态视频。"
        exportPopup.toolTip = "辅助轨单独保存为原名.auxiliary.mov，不重新编码，不添加音频或封面标记。无辅助轨会报告原因。"
        exportPopup.target = self; exportPopup.action = #selector(exportModeChanged)
        for popup in [trackPopup,exportPopup] { popup.controlSize = .small; popup.font = .systemFont(ofSize:12) }
        func field(_ name:String,_ views:[NSView]) -> NSStackView {
            stack([label(name,size:11,color:.secondaryLabelColor)] + views,vertical:false,spacing:8)
        }
        let trackOptions = stack([field("视频轨",[trackPopup]),field("导出",[exportPopup])],vertical:false,spacing:16)
        preserveJPEG.state = .on; preserveJPEG.font = .systemFont(ofSize:12)
        let optionSpacer = flex(); optionSpacers = [optionSpacer]
        let options = stack([legacy,preserveJPEG,exportDataFiles,optionSpacer,advancedButton],vertical:false,spacing:16)
        forwardOptions = trackOptions; commonOptions = options
        vendorPopup.addItems(withTitles:["Google / 通用","小米","三星","华为","荣耀","OPPO","vivo V2"])
        containerPopup.addItems(withTitles:["JPEG · Ultra HDR","HEIC · 保留原编码"])
        for popup in [vendorPopup,containerPopup] { popup.controlSize = .small; popup.font = .systemFont(ofSize:12); popup.target = self; popup.action = #selector(androidSettingsChanged) }
        for slider in [imageQuality,gainQuality] { slider.target = self; slider.action = #selector(qualityChanged); slider.widthAnchor.constraint(equalToConstant:110).isActive = true }
        reverseFormatRow = stack([field("目标厂商",[vendorPopup]),field("照片格式",[containerPopup])],vertical:false,spacing:16)
        reverseQualityRow = stack([field("主图质量",[imageQuality,imageQualityText]),field("增益图",[gainQuality,gainQualityText])],vertical:false,spacing:16)
        hdrTarget.font = .systemFont(ofSize:11); hdrTarget.textColor = .secondaryLabelColor
        preserveJPEG.toolTip = "JPEG 输入默认不重新编码；关闭后主图使用指定质量。HEIC 转 JPEG 始终使用主图质量。已有 Ultra HDR 增益图原样保留，增益图质量用于 Apple HDR 转换。"
        reverseOptions = stack([reverseFormatRow,reverseQualityRow,hdrTarget],spacing:6)
        let settingsBody = stack([trackOptions,reverseOptions,options],spacing:4)
        for child in settingsBody.arrangedSubviews {
            child.translatesAutoresizingMaskIntoConstraints = false; child.widthAnchor.constraint(equalTo:settingsBody.widthAnchor).isActive = true
        }
        let settings = NSBox(); settings.boxType = .custom
        settingsPanel = settings
        settings.cornerRadius = 8; settings.borderWidth = 0.5
        settings.borderColor = .separatorColor; settings.fillColor = .controlBackgroundColor
        settings.contentViewMargins = .zero
        fill(settingsBody,in:settings.contentView!,inset:8)
        status.font = .systemFont(ofSize:11,weight:.medium)
        status.lineBreakMode = .byTruncatingMiddle
        status.setContentCompressionResistancePriority(.init(1),for:.horizontal)
        progress.style = .bar; progress.isIndeterminate = false; progress.minValue = 0; progress.maxValue = 1
        progress.controlSize = .small
        progress.heightAnchor.constraint(equalToConstant:12).isActive = true
        let activity = stack([status,progress],spacing:2)
        for child in activity.arrangedSubviews {
            child.translatesAutoresizingMaskIntoConstraints = false; child.widthAnchor.constraint(equalTo:activity.widthAnchor).isActive = true
        }
        activity.setContentHuggingPriority(.init(1),for:.horizontal)
        activity.setContentCompressionResistancePriority(.init(1),for:.horizontal)
        activity.widthAnchor.constraint(greaterThanOrEqualToConstant:180).isActive = true
        footerActions = stack([openButton,reportButton,flex(),stopButton,convertButton],vertical:false,spacing:8)
        let footer = stack([activity,footerActions],vertical:false,spacing:8)
        footerRow = footer; footer.distribution = .fill
        desktopActionsWidth = footerActions.widthAnchor.constraint(equalToConstant:350)
        mobileActionsWidth = footerActions.widthAnchor.constraint(equalTo:footer.widthAnchor)
        mobileActivityWidth = activity.widthAnchor.constraint(equalTo:footer.widthAnchor)
        desktopActionsWidth.isActive = true
        openButton.widthAnchor.constraint(equalToConstant:76).isActive = true
        reportButton.widthAnchor.constraint(equalToConstant:76).isActive = true
        stopButton.widthAnchor.constraint(equalToConstant:50).isActive = true
        convertButton.keyEquivalent = "\r"; convertButton.controlSize = .regular
        convertButton.font = .systemFont(ofSize:13,weight:.medium)
        convertButton.widthAnchor.constraint(equalToConstant:106).isActive = true
        primaryHeight = convertButton.heightAnchor.constraint(equalToConstant:28); primaryHeight.isActive = true
        stopButton.keyEquivalent = "\u{1b}"
        footerHeight = footer.heightAnchor.constraint(equalToConstant:30); footerHeight.isActive = true
        buttonHeights = [settingsToggle,sourceButton,outputButton,photosButton,openButton,reportButton,stopButton,scanButton,directionPopup,vendorPopup,containerPopup,trackPopup,exportPopup,legacy,exportDataFiles,preserveJPEG,advancedButton,imageQuality,gainQuality].map { $0.heightAnchor.constraint(equalToConstant:44) }
        settingsToggle.isHidden = true
        let container = stack([header,cards,metrics,tableArea,detail,settingsToggle,settings],spacing:8)
        fill(container,in:content,inset:14)
        footer.translatesAutoresizingMaskIntoConstraints = false
        root.addSubview(footer)
        pageScroll.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([pageScroll.topAnchor.constraint(equalTo:root.topAnchor),pageScroll.leadingAnchor.constraint(equalTo:root.leadingAnchor),pageScroll.trailingAnchor.constraint(equalTo:root.trailingAnchor),pageScroll.bottomAnchor.constraint(equalTo:footer.topAnchor,constant:-4),footer.leadingAnchor.constraint(equalTo:root.leadingAnchor,constant:14),footer.trailingAnchor.constraint(equalTo:root.trailingAnchor,constant:-14),footer.bottomAnchor.constraint(equalTo:root.bottomAnchor,constant:-14)])
        for child in container.arrangedSubviews {
            child.translatesAutoresizingMaskIntoConstraints = false
            child.widthAnchor.constraint(equalTo:container.widthAnchor).isActive = true
        }
        refreshControls()
    }
    @objc private func directionChanged() {
        guard !busy else { return }
        refreshControls(); androidSettingsChanged(); updateAdaptiveLayout()
        if !sources.isEmpty { rescan() }
    }
    @objc private func androidSettingsChanged() {
        let supportsHEIC = vendorPopup.indexOfSelectedItem <= 2
        containerPopup.item(at:1)?.isEnabled = supportsHEIC
        if !supportsHEIC { containerPopup.selectItem(at:0) }
        hdrTarget.stringValue = containerPopup.indexOfSelectedItem == 1 ? "HDR：保留 HEIC 原始编码；安卓显示需真机验证" : "HDR：Ultra HDR（默认）；SDR 不生成增益图"
        hdrTarget.lineBreakMode = .byTruncatingTail
        hdrTarget.toolTip = hdrTarget.stringValue
        refreshControls()
    }
    @objc private func qualityChanged() {
        imageQualityText.stringValue = "\(Int((imageQuality.doubleValue * 100).rounded()))%"
        gainQualityText.stringValue = "\(Int((gainQuality.doubleValue * 100).rounded()))%"
    }
    @objc private func toggleSettings() {
        settingsExpanded.toggle(); updateAdaptiveLayout()
    }
    func windowDidResize(_ notification:Notification) { updateAdaptiveLayout() }
    private func updateAdaptiveLayout() {
        guard cardsRow != nil, let width = window?.contentView?.bounds.width else { return }
        let narrow = width < 700, changed = narrow != compact; compact = narrow
        platformLabel.isHidden = narrow
        for spacer in optionSpacers { spacer.isHidden = narrow }
        commonOptions.spacing = narrow ? 4 : 16
        for view in secondaryMetrics { view.isHidden = narrow }
        metricsRow.spacing = narrow ? 8 : 18; metricsHeight.constant = narrow ? 44 : 24
        headerHeight.constant = narrow ? 44 : 28
        cardsRow.orientation = narrow ? .vertical : .horizontal
        cardsRow.alignment = narrow ? .leading : .centerY
        cardsHeight.constant = narrow ? 176 : 60
        for constraint in mobileCardWidths { constraint.isActive = narrow }
        for constraint in buttonHeights { constraint.isActive = narrow }
        primaryHeight.constant = narrow ? 44 : 28
        for stack in [forwardOptions!,commonOptions!,reverseFormatRow!,reverseQualityRow!] {
            stack.orientation = narrow ? .vertical : .horizontal
            stack.alignment = narrow ? .leading : .centerY
        }
        footerRow.orientation = narrow ? .vertical : .horizontal
        footerRow.alignment = narrow ? .leading : .centerY
        desktopActionsWidth.isActive = !narrow; mobileActionsWidth.isActive = narrow; mobileActivityWidth.isActive = narrow
        footerHeight.constant = narrow ? 82 : 30
        settingsToggle.isHidden = !narrow
        settingsToggle.title = settingsExpanded ? "收起导出设置" : "导出设置…"
        settingsPanel.isHidden = narrow && !settingsExpanded
        table.rowHeight = narrow ? 76 : 28
        table.headerView = narrow ? nil : originalTableHeader
        for column in table.tableColumns where column.identifier.rawValue != "name" { column.isHidden = narrow }
        let otherWidth = table.tableColumns.dropFirst().filter { !$0.isHidden }.reduce(CGFloat(0)) { $0 + $1.width + table.intercellSpacing.width }
        table.tableColumns.first?.width = max(220,width - 28 - otherWidth - 32)
        if changed { table.reloadData() }
        window.contentView?.needsLayout = true
    }
    @objc private func importPhotos() {
        guard !busy else { return }
        if !isToAndroid { directionPopup.selectItem(at:1); refreshControls() }
        let picker = PhotosLibraryPicker { [weak self] urls,error in
            guard let self = self else { return }
            self.libraryPicker = nil
            if let error = error { self.showError(error); return }
            guard !urls.isEmpty else { return }
            let root = urls[0].deletingLastPathComponent().deletingLastPathComponent()
            if root.lastPathComponent.hasPrefix("motionlive-photos-") { self.ownedPhotoCopies.append(root) }
            self.choose(urls)
            self.sourcePath.stringValue = "\(urls.count) 张实况 · 来自照片图库"
            self.output = URL(fileURLWithPath:NSHomeDirectory()).appendingPathComponent("Pictures/Motion-Live Exports")
            self.outputPath.stringValue = self.output!.path; self.refreshControls()
        }
        libraryPicker = picker; picker.present(window)
    }
    func applicationWillTerminate(_ notification:Notification) {
        for root in ownedPhotoCopies { try? FileManager.default.removeItem(at:root) }
    }
    func numberOfRows(in tableView: NSTableView) -> Int { rows.count }
    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        guard row < rows.count, let column = tableColumn else { return nil }
        let item = rows[row], id = column.identifier.rawValue
        if compact && id == "name" {
            let state = results[item.relative]?.0 ?? (item.isMotion ? "ready" : "failed")
            let title = label(item.source.lastPathComponent,size:13,weight:.medium)
            title.lineBreakMode = .byTruncatingMiddle; title.toolTip = item.source.path
            let meta = label(item.manufacturer + " · " + item.hdr,size:11,color:.secondaryLabelColor)
            let stateText = ["ready":"待转换","converted":"已完成","failed":"需检查","skipped":"已跳过","resumed":"已有结果"][state] ?? state
            let timing = label(item.cover + " · " + stateText,size:11,color:state == "failed" ? .systemRed : .secondaryLabelColor)
            let body = stack([title,meta,timing],spacing:3), cell = NSTableCellView()
            fill(body,in:cell,inset:8)
            for child in body.arrangedSubviews { child.translatesAutoresizingMaskIntoConstraints = false; child.widthAnchor.constraint(equalTo:body.widthAnchor).isActive = true }
            return cell
        }
        let cell = NSTableCellView()
        let field = NSTextField(labelWithString:"")
        field.font = id == "cover" ? .monospacedDigitSystemFont(ofSize:11,weight:.regular) : .systemFont(ofSize:12)
        field.lineBreakMode = id == "name" ? .byTruncatingMiddle : .byTruncatingTail
        field.translatesAutoresizingMaskIntoConstraints = false; cell.addSubview(field); cell.textField = field
        NSLayoutConstraint.activate([field.leadingAnchor.constraint(equalTo:cell.leadingAnchor,constant:8),field.trailingAnchor.constraint(equalTo:cell.trailingAnchor,constant:-6),field.centerYAnchor.constraint(equalTo:cell.centerYAnchor)])
        switch id {
        case "name": field.stringValue = item.relative; field.toolTip = item.source.path
        case "manufacturer": field.stringValue = item.manufacturer; field.toolTip = item.manufacturerDetail; field.textColor = .secondaryLabelColor
        case "hdr": field.stringValue = item.hdr; field.toolTip = item.hdr == "荣耀私有 HDR" ? item.detail : item.hdr; field.textColor = item.hdr == "Ultra HDR" ? .systemPurple : .secondaryLabelColor
        case "cover": field.stringValue = item.cover; field.textColor = .secondaryLabelColor
        default:
            let result = results[item.relative]
            let state = result?.0 ?? (item.isMotion ? "ready" : item.needsInspection ? "inspect" : "skipped")
            field.stringValue = ["inspect":"需检查","ready":"等待转换","converted":"已转换","resumed":"已验证，跳过","failed":"转换失败","skipped":"跳过"][state] ?? state
            field.textColor = ["failed","inspect"].contains(state) ? .systemRed : ["converted","resumed"].contains(state) ? .systemGreen : .secondaryLabelColor
            field.toolTip = result?.1 ?? item.detail
        }
        return cell
    }
    func tableViewSelectionDidChange(_ notification: Notification) {
        let i = table.selectedRow
        guard i >= 0 && i < rows.count else { return }
        let row = rows[i]
        detail.stringValue = row.relative + " — " + (results[row.relative]?.1 ?? row.detail)
        detail.toolTip = detail.stringValue
    }
    private func refreshControls() {
        directionPopup.isEnabled = !busy; photosButton.isEnabled = !busy
        photosButton.isHidden = !isToAndroid
        forwardOptions?.isHidden = isToAndroid; reverseOptions?.isHidden = !isToAndroid
        legacy.isHidden = isToAndroid; preserveJPEG.isHidden = !isToAndroid
        for control in [vendorPopup,containerPopup] { control.isEnabled = !busy }
        imageQuality.isEnabled = !busy && containerPopup.indexOfSelectedItem == 0
        gainQuality.isEnabled = !busy && containerPopup.indexOfSelectedItem == 0
        preserveJPEG.isEnabled = !busy && containerPopup.indexOfSelectedItem == 0
        sourceButton.isEnabled = !busy; outputButton.isEnabled = !busy
        sourceCard.acceptsDrop = !busy; outputCard.acceptsDrop = !busy
        scanButton.isEnabled = !busy && !sources.isEmpty
        convertButton.isEnabled = !busy && !sources.isEmpty && output != nil && rows.contains(where: { $0.isMotion })
        legacy.isEnabled = !busy && exportPopup.indexOfSelectedItem != 2; advancedButton.isEnabled = !busy
        trackPopup.isEnabled = !busy && exportPopup.indexOfSelectedItem != 2; exportPopup.isEnabled = !busy
        convertButton.title = isToAndroid ? "导出到安卓" : exportPopup.indexOfSelectedItem == 2 ? "导出辅助轨" : "开始转换"
        stopButton.isHidden = !busy; stopButton.isEnabled = busy && !token.isCancelled
        openButton.isEnabled = output.map { FileManager.default.fileExists(atPath:$0.path) } ?? false
        reportButton.isEnabled = reportData != nil
        exportDataFiles.isEnabled = !busy
        emptyState?.isHidden = !rows.isEmpty
        table.usesAlternatingRowBackgroundColors = !rows.isEmpty
        progress.isHidden = !busy
        status.toolTip = status.stringValue
    }
    @objc private func selectSource() {
        guard !busy else { return }
        let panel = NSOpenPanel(); panel.title = isToAndroid ? "选择完整实况配对（图片与 MOV）或文件夹" : "选择 Android 原始动态照片"
        panel.canChooseDirectories = true; panel.canChooseFiles = true; panel.allowsMultipleSelection = true
        panel.prompt = "选择并扫描"
        panel.beginSheetModal(for:window) { if $0 == .OK { self.choose(panel.urls) } }
    }
    private func choose(_ urls: [URL]) {
        guard !busy, let first = urls.first else { return }
        sources = Array(Set(urls.map { NativeBatch.normalized($0) })).sorted { $0.path < $1.path }
        sourcePath.stringValue = sources.count == 1 ? sources[0].path : "已选择 \(sources.count) 个文件 / 文件夹"
        sourcePath.toolTip = sources.map { $0.path }.joined(separator:"\n")
        let base = NativeBatch.root(NativeBatch.normalized(first))
        output = base.deletingLastPathComponent().appendingPathComponent(base.lastPathComponent + (isToAndroid ? " Motion Photos" : " Live Photos"))
        outputPath.stringValue = output!.path; outputPath.toolTip = output!.path
        report = nil; reportData = nil; rows = []; results = [:]; table.reloadData()
        rescan()
    }
    @objc private func selectOutput() {
        guard !busy else { return }
        let panel = NSOpenPanel(); panel.title = "选择保存位置"; panel.prompt = "保存到这里"
        panel.canChooseDirectories = true; panel.canChooseFiles = false; panel.canCreateDirectories = true
        panel.directoryURL = output?.deletingLastPathComponent()
        panel.beginSheetModal(for:window) { if $0 == .OK, let url = panel.url { self.setOutput(url) } }
    }
    private func setOutput(_ url: URL) {
        guard !busy else { return }
        let destination = NativeBatch.normalized(url)
        do {
            try NativeBatch.validateOutput(sources:sources,output:destination)
            guard (try? destination.resourceValues(forKeys:[.isDirectoryKey]).isDirectory) == true else { throw Failure("请选择一个保存文件夹") }
            output = destination; outputPath.stringValue = destination.path; outputPath.toolTip = destination.path
            report = nil; reportData = nil; refreshControls()
        } catch { showError(error) }
    }
    @objc private func rescan() {
        guard !busy, !sources.isEmpty else { return }
        let selectedSources = sources, direction = isToAndroid ? "to-android" : "to-apple"
        busy = true; converting = false; token = StopToken(); results = [:]; report = nil; reportData = nil
        status.stringValue = "正在扫描照片…"; progress.isIndeterminate = true; progress.startAnimation(nil); refreshControls()
        let currentToken = token
        DispatchQueue.global(qos:.userInitiated).async {
            do {
                let scanned = try NativeBatch.scan(sources:selectedSources,direction:direction,token:currentToken) { index,total in
                    if index % 20 == 0 || index == total {
                        DispatchQueue.main.async { self.status.stringValue = "正在扫描 \(index) / \(total)…" }
                    }
                }
                DispatchQueue.main.async {
                    self.rows = scanned; self.busy = false
                    self.progress.stopAnimation(nil); self.progress.isIndeterminate = false; self.progress.doubleValue = 0
                    self.totalMetric.stringValue = "\(scanned.count)"
                    self.motionMetric.stringValue = "\(scanned.filter { $0.isMotion }.count)"
                    self.hdrMetric.stringValue = "\(scanned.filter { $0.hdr == "Ultra HDR" }.count)"
                    self.issueMetric.stringValue = "\(scanned.filter { $0.isMotion && $0.cover == "未记录" }.count)"
                    self.status.stringValue = currentToken.isCancelled ? "扫描已停止" : scanned.contains(where: { $0.isMotion }) ? "扫描完成，可以开始转换" : "未找到支持的动态照片 JPEG/HEIC"
                    self.detail.stringValue = "Ultra HDR 保留原样；SDR 不生成增益图。选中照片可查看详情。"
                    self.table.reloadData(); self.refreshControls()
                }
            } catch { DispatchQueue.main.async { self.finishWithError(error) } }
        }
    }
    @objc private func exportModeChanged() { refreshControls() }
    private var options: ConversionOptions {
        var value = ConversionOptions(legacyHDR:legacy.state == .on,missingTime:missingPopup.indexOfSelectedItem == 1 ? "midpoint" : "error",
            timeDomain:["auto","media","presentation"][max(0,domainPopup.indexOfSelectedItem)],videoTrack:trackPopup.indexOfSelectedItem == 1 ? "secondary" : "primary",
            outputMode:["livephoto","livephoto+auxiliary","auxiliary-only"][max(0,exportPopup.indexOfSelectedItem)],repairAuxiliaryDuration:repairAuxiliary.state == .on,exportDataFiles:exportDataFiles.state == .on)
        value.direction = isToAndroid ? "to-android" : "to-apple"
        value.android = AndroidOptions(vendor:["google","xiaomi","samsung","huawei","honor","oppo","vivo"][max(0,vendorPopup.indexOfSelectedItem)],container:containerPopup.indexOfSelectedItem == 1 ? "heic" : "jpeg",hdr:containerPopup.indexOfSelectedItem == 1 ? "preserve" : "ultra",imageQuality:imageQuality.doubleValue,gainQuality:gainQuality.doubleValue,preserveJPEG:preserveJPEG.state == .on)
        return value
    }
    @objc private func startConversion() {
        guard !busy, !sources.isEmpty, let output = output else { return }
        let selectedSources = sources
        do { try NativeBatch.validateOutput(sources:selectedSources,output:output) } catch { showError(error); return }
        advancedPopover?.close()
        existingChoiceForBatch = nil
        busy = true; converting = true; token = StopToken(); results = [:]; report = nil; reportData = nil
        status.stringValue = "正在准备转换…"; progress.doubleValue = 0; refreshControls(); table.reloadData()
        let selectedOptions = options, currentToken = token
        DispatchQueue.global(qos:.userInitiated).async {
            do {
                let outcome = try NativeBatch.run(sources:selectedSources,output:output,options:selectedOptions,token:currentToken,
                    existingOutput: { source,target in self.chooseExistingOutput(source:source,target:target,token:currentToken) }) { update in
                    DispatchQueue.main.async {
                        self.progress.maxValue = Double(max(update.total,1)); self.progress.doubleValue = Double(update.index)
                        self.status.stringValue = "\(update.index) / \(update.total)  ·  \(update.relative)"
                        self.status.toolTip = self.status.stringValue
                        self.results[update.relative] = (update.status,update.detail)
                        if let i = self.rows.firstIndex(where: { $0.relative == update.relative }) {
                            let column = self.table.column(withIdentifier:NSUserInterfaceItemIdentifier(self.compact ? "name" : "status"))
                            if column >= 0 { self.table.reloadData(forRowIndexes:IndexSet(integer:i),columnIndexes:IndexSet(integer:column)) }
                        }
                    }
                }
                DispatchQueue.main.async {
                    self.busy = false; self.converting = false; self.report = outcome.report; self.reportData = outcome.reportData
                    let c = outcome.counts
                    self.status.stringValue = (outcome.cancelled ? "已停止" : "转换完成") + "  ·  成功 \(c["converted"] ?? 0)  ·  已有 \(c["resumed"] ?? 0)  ·  跳过 \(c["skipped"] ?? 0)  ·  失败 \(c["failed"] ?? 0)"
                    self.detail.stringValue = selectedOptions.direction == "to-android" ? "已生成单文件安卓动态照片；厂商识别和 HDR 显示请在目标手机验证。" : selectedOptions.outputMode == "auxiliary-only" ? "辅助轨已导出为独立 MOV，不含音频；若修复零时长，会记入清单。查看报告可了解详情。" : "将每对同名 JPG/JPEG/HEIC 与 MOV 一起导入「照片」；.auxiliary.mov 是独立视频，请分开保存。"
                    self.refreshControls()
                }
            } catch { DispatchQueue.main.async { self.finishWithError(error) } }
        }
    }
    private func chooseExistingOutput(source: URL, target: URL, token: StopToken) -> ExistingOutputDecision {
        // Conversion waits on its worker queue; the main queue stays available
        // for the sheet, window updates and application events.
        let answered = DispatchSemaphore(value:0)
        var choice = ExistingOutputDecision.cancel
        DispatchQueue.main.async {
            if token.isCancelled { answered.signal(); return }
            if let previous = self.existingChoiceForBatch { choice = previous; answered.signal(); return }
            let alert = NSAlert()
            alert.messageText = "已有转换结果"
            alert.informativeText = "照片：\(source.lastPathComponent)\n已有输出：\(target.path)\n\n跳过会保留已有文件。覆盖会替换这张照片的整个输出文件夹；新结果完成并通过校验后才替换，失败时保留旧结果。"
            alert.alertStyle = .warning
            alert.addButton(withTitle:"跳过")
            alert.addButton(withTitle:"覆盖")
            alert.addButton(withTitle:"停止本次任务").keyEquivalent = "\u{1b}"
            alert.showsSuppressionButton = true
            alert.suppressionButton?.title = "将此选择应用于本次所有已有输出"
            alert.beginSheetModal(for:self.window) { response in
                switch response {
                case .alertFirstButtonReturn: choice = .skip
                case .alertSecondButtonReturn: choice = .overwrite
                default: choice = .cancel
                }
                if choice != .cancel && alert.suppressionButton?.state == .on { self.existingChoiceForBatch = choice }
                answered.signal()
            }
        }
        answered.wait()
        return choice
    }
    @objc private func stop() {
        token.cancel(); stopButton.isEnabled = false
        status.stringValue = converting ? "正在停止，等待当前文件清理完成…" : "正在停止扫描…"
    }
    private func finishWithError(_ error: Error) {
        busy = false; converting = false; progress.stopAnimation(nil); progress.isIndeterminate = false
        status.stringValue = "任务未完成：" + error.localizedDescription; refreshControls(); showError(error)
    }
    private func showError(_ error: Error) {
        let alert = NSAlert(); alert.messageText = "无法完成操作"; alert.informativeText = error.localizedDescription
        alert.alertStyle = .warning; alert.addButton(withTitle:"好"); alert.beginSheetModal(for:window)
    }
    @objc private func openOutput() { if let output = output { NSWorkspace.shared.open(output) } }
    @objc private func openReport() {
        guard let data = reportData else { return }
        do {
            guard let counts = data["counts"] as? [String:Int], let files = data["files"] as? [[String:Any]] else { throw Failure("报告格式无法读取") }
            let panel = NSWindow(contentRect:NSRect(x:0,y:0,width:760,height:500),styleMask:[.titled,.closable,.resizable],backing:.buffered,defer:false)
            panel.title = "转换报告"; panel.isReleasedWhenClosed = false; panel.minSize = NSSize(width:560,height:350)
            let summary = label("成功 \(counts["converted"] ?? 0)   已有 \(counts["resumed"] ?? 0)   跳过 \(counts["skipped"] ?? 0)   失败 \(counts["failed"] ?? 0)",size:15,weight:.semibold)
            let scroll = NSScrollView(); scroll.hasVerticalScroller = true; scroll.autohidesScrollers = true; scroll.borderType = .bezelBorder
            let text = NSTextView(frame:NSRect(x:0,y:0,width:700,height:380))
            text.isEditable = false; text.isSelectable = true; text.font = .systemFont(ofSize:12)
            text.textContainerInset = NSSize(width:12,height:12); text.isHorizontallyResizable = false
            text.autoresizingMask = [.width]; text.textContainer?.widthTracksTextView = true
            let names = ["converted":"成功","resumed":"已验证，跳过","skipped":"跳过","failed":"失败"]
            text.string = files.map { file in
                let status = file["status"] as? String ?? "failed"
                let source = file["source"] as? String ?? ""
                let message = file["detail"] as? String ?? file["error"] as? String ?? ""
                return "【\(names[status] ?? status)】\(source)\n\(message)"
            }.joined(separator:"\n\n")
            scroll.documentView = text
            let reveal = NSButton(title:"在访达中显示原始报告",target:self,action:#selector(revealReport)); reveal.bezelStyle = .rounded
            reveal.isHidden = report == nil
            let content = stack([summary,scroll,reveal],spacing:16)
            fill(content,in:panel.contentView!,inset:20)
            scroll.translatesAutoresizingMaskIntoConstraints = false; scroll.widthAnchor.constraint(equalTo:content.widthAnchor).isActive = true
            scroll.setContentHuggingPriority(.init(1),for:.vertical)
            panel.center(); panel.makeKeyAndOrderFront(nil); reportWindow = panel
        } catch { showError(error) }
    }
    @objc private func revealReport() { if let report = report { NSWorkspace.shared.activateFileViewerSelecting([report]) } }
    @objc private func showAdvanced() {
        if let popover = advancedPopover, popover.isShown { popover.close(); return }
        let controller = NSViewController()
        let content = NSView(frame:NSRect(x:0,y:0,width:440,height:290)); controller.view = content
        let note = NSTextField(wrappingLabelWithString:"仅在了解原片时间轴时修改。自动模式会检查封面位置，遇到歧义时报告错误。")
        note.font = .systemFont(ofSize:11); note.textColor = .secondaryLabelColor
        repairAuxiliary.toolTip = "仅对有一个编码样本、时长为零的辅助轨，使用主轨最短有效帧长修复时长。压缩画面与样本起始时间不变，清单记录修复。默认关闭。"
        repairAuxiliary.font = .systemFont(ofSize:12)
        let controls = stack([label("封面时间未记录时",size:12,weight:.medium),missingPopup,label("原片封面时间的含义",size:12,weight:.medium),domainPopup,repairAuxiliary,note],spacing:10)
        fill(controls,in:content,inset:18)
        for child in [missingPopup,domainPopup,note] { child.widthAnchor.constraint(equalTo:controls.widthAnchor).isActive = true }
        let popover = NSPopover(); popover.contentViewController = controller; popover.behavior = .transient
        popover.contentSize = content.frame.size; advancedPopover = popover
        popover.show(relativeTo:advancedButton.bounds,of:advancedButton,preferredEdge:.maxY)
    }
    @objc private func showHelp() {
        if let url = Bundle.main.url(forResource:"使用说明",withExtension:"txt") { NSWorkspace.shared.open(url) }
    }
    @objc private func about() {
        let alert = NSAlert(); alert.messageText = "Motion-Live 1.2.0"
        alert.informativeText = "Android 动态照片 → Apple Live Photo\n\n适用于 macOS 12 或更新版本。\n照片在本机处理，原片只读。\n\n支持 Google/小米兼容 JPEG、华为/荣耀 LIVE_、三星 SEF、OPPO/vivo 单文件 JPEG。\n新增三星 mpvd/SEF HEIC 样片无损配对。其他 HEIC/AVIF、vivo 旧双文件及未知私有 HDR 暂未完整支持。"
        alert.addButton(withTitle:"好"); alert.beginSheetModal(for:window)
    }
    func application(_ sender: NSApplication, openFiles filenames: [String]) {
        guard !filenames.isEmpty, !busy else { sender.reply(toOpenOrPrint:.failure); return }
        let urls = filenames.map { URL(fileURLWithPath:$0) }
        if window == nil { pendingOpen.append(contentsOf:urls) } else { choose(urls) }
        sender.reply(toOpenOrPrint:.success)
    }
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { !busy }
    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        if busy { stop(); return .terminateCancel }
        return .terminateNow
    }
    func windowShouldClose(_ sender: NSWindow) -> Bool {
        if busy { stop(); return false }
        return true
    }
    private func waitForSnapshot(_ path: String, attempt: Int = 0) {
        DispatchQueue.main.asyncAfter(deadline:.now() + 0.25) {
            if self.busy && attempt < 120 { self.waitForSnapshot(path,attempt:attempt + 1); return }
            if CommandLine.arguments.contains("--ui-progress-preview") {
                self.busy = true; self.converting = true
                self.status.stringValue = "正在转换 12 / 32 · 保留原始画质"
                self.progress.doubleValue = 0.375; self.refreshControls()
            }
            if CommandLine.arguments.contains("--ui-scroll-bottom"), let document = self.pageScroll.documentView {
                document.scroll(NSPoint(x:0,y:max(0,document.bounds.height - self.pageScroll.contentView.bounds.height)))
            }
            DispatchQueue.main.asyncAfter(deadline:.now() + 0.2) {
                self.snapshot(path)
                // Preview-only busy state must not trigger the normal quit guard.
                self.busy = false; self.converting = false; NSApp.terminate(nil)
            }
        }
    }
    private func snapshot(_ path: String) {
        guard let view = window.contentView else { return }
        view.layoutSubtreeIfNeeded(); view.displayIfNeeded()
        guard let bitmap = view.bitmapImageRepForCachingDisplay(in:view.bounds) else { return }
        view.cacheDisplay(in:view.bounds,to:bitmap)
        // Window capture includes animated native control layers, which
        // cacheDisplay omits (notably the progress bar's colored fill).
        let capture = CGWindowListCreateImage(.null,.optionIncludingWindow,CGWindowID(window.windowNumber),[.boundsIgnoreFraming,.bestResolution])
        let rendered = capture.map { NSBitmapImageRep(cgImage:$0) } ?? bitmap
        if let png = rendered.representation(using:.png,properties:[:]) { try? png.write(to:URL(fileURLWithPath:path)) }
        let metrics: [String:Any] = ["frame_width":window.frame.width,"compact":compact,"settings_expanded":settingsExpanded,"frame_height":window.frame.height,"content_height":view.bounds.height,
            "document_height":pageScroll.documentView?.bounds.height ?? 0,"scroll_y":pageScroll.contentView.bounds.minY,
            "minimum_height":window.minSize.height,"table_height":table.enclosingScrollView?.bounds.height ?? 0,
            "row_height":table.rowHeight,"queue_rows":rows.count,
            "footer_visible":view.bounds.contains(convertButton.convert(convertButton.bounds,to:view)),
            "primary_trailing_margin":view.bounds.maxX - convertButton.convert(convertButton.bounds,to:view).maxX,
            "status_width":status.bounds.width,"progress_visible":!progress.isHidden,
            "progress_height":progress.bounds.height,"progress_value":progress.doubleValue,"progress_max":progress.maxValue]
        if let data = try? JSONSerialization.data(withJSONObject:metrics,options:[.prettyPrinted,.sortedKeys]) {
            try? data.write(to:URL(fileURLWithPath:path + ".json"))
        }
    }
}

@main
struct MotionLiveApplication {
    static func main() {
        let application = NSApplication.shared
        application.setActivationPolicy(.regular)
        let delegate = AppDelegate()
        application.delegate = delegate
        withExtendedLifetime(delegate) { application.run() }
    }
}
