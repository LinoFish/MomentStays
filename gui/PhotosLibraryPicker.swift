import AppKit
import Photos

// macOS 12 picker. Original bytes are read only on user action; cloud downloads opt in.
final class PhotosLibraryPicker: NSObject, NSTableViewDataSource, NSTableViewDelegate {
    private var sheet: NSWindow!
    private let table = NSTableView()
    private let message = NSTextField(labelWithString: "仅访问系统图库；选择范围后扫描。原片不会被修改。")
    private let libraryInfo = NSTextField(wrappingLabelWithString: "")
    private let importButton = NSButton(title: "读取所选", target: nil, action: nil)
    private let cancelButton = NSButton(title: "取消", target: nil, action: nil)
    private let allButton = NSButton(title: "扫描范围内全部", target: nil, action: nil)
    private let albumPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let versionPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let network = NSButton(checkboxWithTitle: "允许下载 iCloud 原件", target: nil, action: nil)
    private let monitor = NSButton(checkboxWithTitle: "监测此范围新增/编辑照片并自动转换（运行期间）", target: nil, action: nil)
    private var albums: [PHAssetCollection] = []
    private var assets: PHFetchResult<PHAsset>!
    private let images = PHCachingImageManager()
    private let service = PhotoLibraryService()
    private let token = StopToken()
    private let store: DeliveryStore
    private let direction: ConversionDirection
    private var importing = false, finished = false
    var onProgress: (String) -> Void = { _ in }
    private(set) var origins: [String: LibraryOrigin] = [:]
    private(set) var selectedScope: LibraryScope
    private(set) var wantsMonitoring = false
    var wasCancelled: Bool { token.isCancelled }
    private(set) var summary = ""
    private let completion: ([URL], Error?) -> Void
    private weak var parent: NSWindow?
    init(direction: ConversionDirection = .toAndroid, version: LibraryVersion = .original, store: DeliveryStore = DeliveryStore(), completion: @escaping ([URL], Error?) -> Void) {
        self.direction = direction; self.store = store; self.completion = completion
        selectedScope = LibraryScope(direction: direction, version: version, networkAllowed: false)
    }
    func present(_ parent: NSWindow) {
        self.parent = parent
        PhotoLibraryService.authorize { [weak self] result in
            guard let self = self, !self.finished else { return }
            switch result {
            case .success: self.build(parent)
            case .failure(let error): self.finish([], error)
            }
        }
    }
    private func build(_ parent: NSWindow) {
        let width = min(720, max(366, (parent.contentView?.bounds.width ?? 720) - 24))
        let height = min(580, max(430, (parent.contentView?.bounds.height ?? 600) - 40))
        sheet = NSWindow(contentRect: NSRect(x: 0, y: 0, width: width, height: height), styleMask: [.titled, .resizable], backing: .buffered, defer: false)
        sheet.title = direction == .toAndroid ? "系统图库 · Apple 实况" : "系统图库 · 识别安卓动态照片"
        sheet.minSize = NSSize(width: 366, height: 430)
        albums = PhotoLibraryService.albums(writableOnly: false); albumPopup.addItem(withTitle: "整个系统照片图库")
        albumPopup.addItems(withTitles: albums.map { $0.localizedTitle ?? "未命名相册" })
        albumPopup.target = self; albumPopup.action = #selector(changeScope)
        versionPopup.addItems(withTitles: LibraryVersion.allCases.map(\.title)); versionPopup.selectItem(at: selectedScope.version == .original ? 0 : 1)
        versionPopup.target = self; versionPopup.action = #selector(changeScope)
        table.delegate = self; table.dataSource = self; table.rowHeight = 54; table.allowsMultipleSelection = true; table.style = .fullWidth
        for (id, title, size) in [("image", "照片", 60.0), ("name", "文件 / 拍摄时间", width - 100)] {
            let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier(id)); column.title = title; column.width = size
            column.minWidth = id == "image" ? 60 : 180; if id == "image" { column.maxWidth = 60 }; table.addTableColumn(column)
        }
        table.columnAutoresizingStyle = .lastColumnOnlyAutoresizingStyle
        let scroll = NSScrollView(); scroll.hasVerticalScroller = true; scroll.documentView = table
        for (button, action) in [(importButton, #selector(importSelected)), (allButton, #selector(readAll)), (cancelButton, #selector(cancel))] {
            button.target = self; button.action = action; button.bezelStyle = .rounded
        }
        importButton.keyEquivalent = "\r"; importButton.isEnabled = false; cancelButton.keyEquivalent = "\u{1b}"
        network.font = .systemFont(ofSize: 11); monitor.font = .systemFont(ofSize: 11)
        monitor.toolTip = "仅监测当前相册/方向。应用退出后停止，不在后台常驻。"
        let gap = NSView(); gap.setContentHuggingPriority(.init(1), for: .horizontal)
        let buttons = NSStackView(views: [cancelButton, gap, allButton, importButton]); buttons.orientation = .horizontal
        message.font = .systemFont(ofSize: 11); message.maximumNumberOfLines = 2; message.lineBreakMode = .byTruncatingMiddle
        libraryInfo.font = .systemFont(ofSize: 11); libraryInfo.isSelectable = true
        libraryInfo.setContentCompressionResistancePriority(.required, for: .vertical)
        let content = NSStackView(views: [albumPopup, versionPopup, libraryInfo, message, scroll, network, monitor, buttons])
        content.orientation = .vertical; content.spacing = 8; content.alignment = .leading; content.translatesAutoresizingMaskIntoConstraints = false
        sheet.contentView!.addSubview(content)
        NSLayoutConstraint.activate([content.topAnchor.constraint(equalTo: sheet.contentView!.topAnchor, constant: 12), content.bottomAnchor.constraint(equalTo: sheet.contentView!.bottomAnchor, constant: -12), content.leadingAnchor.constraint(equalTo: sheet.contentView!.leadingAnchor, constant: 12), content.trailingAnchor.constraint(equalTo: sheet.contentView!.trailingAnchor, constant: -12)])
        for view in [albumPopup, versionPopup, libraryInfo, message, scroll, buttons] { view.translatesAutoresizingMaskIntoConstraints = false; view.widthAnchor.constraint(equalTo: content.widthAnchor).isActive = true }
        scroll.setContentHuggingPriority(.init(1), for: .vertical)
        changeScope(); parent.beginSheet(sheet)
    }
    @objc private func changeScope() {
        guard !importing else { return }
        selectedScope.albumID = albumPopup.indexOfSelectedItem > 0 ? albums[albumPopup.indexOfSelectedItem - 1].localIdentifier : nil
        selectedScope.version = versionPopup.indexOfSelectedItem == 1 ? .edited : .original
        assets = PhotoLibraryService.fetch(selectedScope); table.reloadData(); importButton.isEnabled = false
        allButton.isEnabled = assets.count > 0
        var allImagesScope = selectedScope; allImagesScope.albumID = nil; allImagesScope.direction = .toApple
        let imageCount = selectedScope.albumID == nil && direction == .toApple ? assets.count : PhotoLibraryService.fetch(allImagesScope).count
        let permission = PHPhotoLibrary.authorizationStatus(for: .readWrite) == .limited ? "部分访问" : "完全访问"
        libraryInfo.stringValue = "系统图库可见图片 \(imageCount) 张 · 本方向/范围候选 \(assets.count) 张 · \(permission)\n含个人图库、共享相簿与同步来源。数量不符请核对「照片 → 设置 → 通用」当前是否为系统照片图库。"
        libraryInfo.toolTip = libraryInfo.stringValue
        if direction == .toApple {
            message.stringValue = "\(assets.count) 张待检查照片（含 Live Photo）· 读取原片后识别安卓动态，非安卓动态原片跳过。云端原件需允许下载。"
        } else {
            message.stringValue = "\(assets.count) 张 Apple 实况候选 · 当前仅列 Live Photo，扫描安卓动态请切换为「安卓 → 苹果实况」。云端原件需允许下载。"
        }
    }
    func numberOfRows(in tableView: NSTableView) -> Int { assets?.count ?? 0 }
    func tableView(_ tableView: NSTableView, viewFor column: NSTableColumn?, row: Int) -> NSView? {
        guard row < assets.count, let column = column else { return nil }; let asset = assets.object(at: row)
        if column.identifier.rawValue == "image" {
            let view = NSImageView(); view.imageScaling = .scaleProportionallyUpOrDown
            let options = PHImageRequestOptions(); options.isNetworkAccessAllowed = false
            images.requestImage(for: asset, targetSize: NSSize(width: 88, height: 88), contentMode: .aspectFit, options: options) { [weak view] image, _ in DispatchQueue.main.async { view?.image = image } }
            return view
        }
        let resources = PHAssetResource.assetResources(for: asset)
        let name = resources.first(where: { $0.type == .photo })?.originalFilename ?? "照片"
        let date = asset.creationDate.map { DateFormatter.localizedString(from: $0, dateStyle: .medium, timeStyle: .short) } ?? "未知时间"
        let field = NSTextField(labelWithString: name + "\n" + date + " · \(asset.pixelWidth) × \(asset.pixelHeight)")
        field.maximumNumberOfLines = 2; field.font = .systemFont(ofSize: 12); field.lineBreakMode = .byTruncatingMiddle; field.toolTip = field.stringValue
        return field
    }
    func tableViewSelectionDidChange(_ notification: Notification) {
        if !importing { importButton.isEnabled = !table.selectedRowIndexes.isEmpty }
    }
    @objc private func readAll() { table.selectAll(nil); importSelected() }
    @objc func cancel() {
        guard !finished else { return }; token.cancel(); service.cancel()
        if importing { message.stringValue = "正在停止原件读取…"; cancelButton.isEnabled = false }
        else { finish([], nil) }
    }
    private func finish(_ urls: [URL], _ error: Error?) {
        guard !finished else { return }; finished = true
        if let sheet = sheet { parent?.endSheet(sheet); sheet.orderOut(nil) }
        completion(urls, error)
    }
    @objc private func importSelected() {
        guard !importing, !table.selectedRowIndexes.isEmpty else { return }
        importing = true; importButton.isEnabled = false; allButton.isEnabled = false; table.isEnabled = false
        albumPopup.isEnabled = false; versionPopup.isEnabled = false; network.isEnabled = false; monitor.isEnabled = false
        selectedScope.networkAllowed = network.state == .on; wantsMonitoring = monitor.state == .on
        let selected = table.selectedRowIndexes.map { assets.object(at: $0) }, scope = selectedScope
        DispatchQueue.global(qos: .userInitiated).async {
            do {
                let result = try self.service.export(selected, scope: scope, store: self.store, token: self.token) { _, _, text in
                    DispatchQueue.main.async { self.message.stringValue = text; self.onProgress(text) }
                }
                DispatchQueue.main.async {
                    self.origins = result.origins
                    self.summary = "动态候选 \(result.urls.count) · 静态跳过 \(result.skipped) · 待读取 \(result.deferred)"
                    if !result.warnings.isEmpty { self.summary += "\n" + result.warnings.prefix(5).joined(separator: "\n") }
                    self.onProgress(self.summary)
                    self.finish(result.urls, self.token.isCancelled ? nil : result.urls.isEmpty && result.deferred > 0 ? Failure(self.summary) : nil)
                }
            } catch { DispatchQueue.main.async { self.finish([], error) } }
        }
    }
}
