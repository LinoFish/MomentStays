import AppKit
import Photos

// macOS 12 picker. Original bytes are read only on user action; cloud downloads opt in.
final class PhotosLibraryPicker: NSObject, NSTableViewDataSource, NSTableViewDelegate, NSCollectionViewDataSource, NSCollectionViewDelegateFlowLayout {
    private var sheet: NSWindow!
    private let table = NSTableView()
    private let listScroll = NSScrollView()
    private let gridScroll = NSScrollView()
    private let collection = NSCollectionView()
    private let viewMode = NSSegmentedControl(labels:["缩略图","列表"],trackingMode:.selectOne,target:nil,action:nil)
    private var selectedIndexes = IndexSet()
    private var syncingSelection = false
    private var thumbnailMode: Bool { viewMode.selectedSegment == 0 }
    private var assetCount: Int { previewCount ?? assets?.count ?? 0 }
    private let message = NSTextField(wrappingLabelWithString: "选择相册和照片，原片保持不变。")
    private let libraryInfo = NSTextField(wrappingLabelWithString: "")
    private let importButton = NSButton(title: "读取所选", target: nil, action: nil)
    private let cancelButton = NSButton(title: "取消", target: nil, action: nil)
    private let allButton = NSButton(title: "读取全部", target: nil, action: nil)
    private let albumPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let versionPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let network = NSButton(checkboxWithTitle: "允许下载 iCloud 原件", target: nil, action: nil)
    private let monitor = NSButton(checkboxWithTitle: "自动转换新增或编辑照片（运行期间）", target: nil, action: nil)
    private var albums: [PHAssetCollection] = []
    private var assets: PHFetchResult<PHAsset>!
    private let images = PHCachingImageManager()
    private let readProgress = NSProgressIndicator()
    private let photoBrowser = NSView()
    private let emptyMessage = NSTextField(wrappingLabelWithString:"此相册没有可读取的照片\n试试其他相册或转换方向")
    private var previewCount: Int? // Set only by the diagnostics entry point; never authorizes PhotoKit.
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
    #if UI_DIAGNOSTICS
    func presentPreview(_ parent: NSWindow, count: Int, selectFirst: Bool, switchVersion: Bool = false, listMode: Bool = false, switchView: Bool = false, resize: Bool = false) {
        self.parent = parent; previewCount = count
        build(parent)
        sheet.setContentSize(WindowSizing.library)
        if selectFirst && count > 0 { setSelection(IndexSet(integersIn:0..<min(3,count))) }
        if listMode { viewMode.selectedSegment = 1; changeViewMode() }
        if switchView {
            viewMode.selectedSegment = 1; changeViewMode()
            viewMode.selectedSegment = 0; changeViewMode()
        }
        if switchVersion { versionPopup.selectItem(at:1); changeScope() }
        if resize {
            sheet.setContentSize(NSSize(width:760,height:620)); sheet.contentView?.layoutSubtreeIfNeeded()
            sheet.setContentSize(WindowSizing.library); sheet.contentView?.layoutSubtreeIfNeeded()
        }
    }

    var diagnosticMetrics: [String: Any] {
        guard let root = sheet?.contentView else { return [:] }
        let visible = root.bounds.insetBy(dx:-1,dy:-1)
        return ["width":root.bounds.width,"height":root.bounds.height,"rows":previewCount ?? 0,
                "mode":thumbnailMode ? "thumbnails" : "list", "browser_height":photoBrowser.bounds.height,
                "grid_overflow":collection.collectionViewLayout?.collectionViewContentSize.width ?? 0 > gridScroll.contentSize.width + 1,
                "visible_thumbnails":collection.visibleItems().count, "selected":selectedIndexes.count,
                "table_selected":table.selectedRowIndexes.count,"grid_selected":collection.selectionIndexPaths.count,"primary_title":importButton.title,
                "primary_enabled":importButton.isEnabled,"empty_visible":!emptyMessage.isHidden,
                "empty_centered":abs(emptyMessage.frame.midX - photoBrowser.bounds.midX) < 1 && abs(emptyMessage.frame.midY - photoBrowser.bounds.midY) < 1,
                "first_row_visible":assetCount == 0 || (thumbnailMode
                    ? collection.collectionViewLayout?.layoutAttributesForItem(at:IndexPath(item:0,section:0)).map { collection.visibleRect.contains($0.frame) } ?? false
                    : table.visibleRect.contains(table.rect(ofRow:0))),
                "controls_visible":[albumPopup,versionPopup,viewMode,network,monitor,allButton,importButton,cancelButton].allSatisfy {
                    visible.contains($0.convert($0.bounds,to:root))
                }]
    }
    #endif

    private func build(_ parent: NSWindow) {
        let width = min(760,max(WindowSizing.library.width,(parent.contentView?.bounds.width ?? 800) - 32))
        let height = min(620,max(WindowSizing.library.height,(parent.contentView?.bounds.height ?? 640) - 48))
        sheet = NSWindow(contentRect:NSRect(x:0,y:0,width:width,height:height),styleMask:[.titled,.resizable],backing:.buffered,defer:false)
        sheet.title = "照片图库"; sheet.contentMinSize = WindowSizing.library
        if previewCount == nil { albums = PhotoLibraryService.albums(writableOnly:false) }
        albumPopup.addItem(withTitle:"所有照片")
        albumPopup.addItems(withTitles:albums.map { $0.localizedTitle ?? "未命名相册" })
        albumPopup.target = self; albumPopup.action = #selector(changeScope)
        albumPopup.setAccessibilityLabel("相册范围")
        versionPopup.addItems(withTitles:LibraryVersion.allCases.map(\.title))
        versionPopup.selectItem(at:selectedScope.version == .original ? 0 : 1)
        versionPopup.target = self; versionPopup.action = #selector(changeScope)
        versionPopup.setAccessibilityLabel("照片版本")
        table.delegate = self; table.dataSource = self; table.rowHeight = 58
        table.allowsMultipleSelection = true; table.style = .fullWidth
        table.setAccessibilityLabel("图库照片，可多选")
        for (id,title,size) in [("image","照片",60.0),("name","文件与拍摄时间",width - 120)] {
            let column = NSTableColumn(identifier:NSUserInterfaceItemIdentifier(id))
            column.title = title; column.width = size; column.minWidth = id == "image" ? 60 : 180
            if id == "image" { column.maxWidth = 60 }
            table.addTableColumn(column)
        }
        table.columnAutoresizingStyle = .lastColumnOnlyAutoresizingStyle
        listScroll.hasVerticalScroller = true; listScroll.documentView = table
        listScroll.borderType = .bezelBorder
        let flow = NSCollectionViewFlowLayout()
        flow.minimumInteritemSpacing = 8; flow.minimumLineSpacing = 8
        flow.sectionInset = NSEdgeInsets(top:8,left:8,bottom:8,right:8)
        collection.frame = NSRect(x:0,y:0,width:width - 32,height:200)
        collection.autoresizingMask = [.width]
        collection.collectionViewLayout = flow
        collection.isSelectable = true; collection.allowsMultipleSelection = true
        collection.dataSource = self; collection.delegate = self
        collection.backgroundColors = [.controlBackgroundColor]
        collection.setAccessibilityLabel("图库缩略图，可按住 Command 多选")
        collection.register(LibraryThumbnailItem.self,forItemWithIdentifier:NSUserInterfaceItemIdentifier("thumbnail"))
        gridScroll.hasVerticalScroller = true; gridScroll.documentView = collection
        gridScroll.borderType = .bezelBorder
        for scroll in [listScroll,gridScroll] {
            scroll.translatesAutoresizingMaskIntoConstraints = false
            photoBrowser.addSubview(scroll)
            NSLayoutConstraint.activate([
                scroll.topAnchor.constraint(equalTo:photoBrowser.topAnchor),
                scroll.bottomAnchor.constraint(equalTo:photoBrowser.bottomAnchor),
                scroll.leadingAnchor.constraint(equalTo:photoBrowser.leadingAnchor),
                scroll.trailingAnchor.constraint(equalTo:photoBrowser.trailingAnchor)
            ])
        }
        viewMode.selectedSegment = 0; viewMode.target = self; viewMode.action = #selector(changeViewMode)
        viewMode.setAccessibilityLabel("图库显示方式")
        listScroll.isHidden = true
        emptyMessage.alignment = .center; emptyMessage.textColor = .secondaryLabelColor
        emptyMessage.font = .systemFont(ofSize:13); emptyMessage.translatesAutoresizingMaskIntoConstraints = false
        photoBrowser.addSubview(emptyMessage)
        NSLayoutConstraint.activate([
            emptyMessage.centerXAnchor.constraint(equalTo:photoBrowser.centerXAnchor),
            emptyMessage.centerYAnchor.constraint(equalTo:photoBrowser.centerYAnchor),
            emptyMessage.widthAnchor.constraint(equalTo:photoBrowser.widthAnchor,constant:-48)
        ])
        for (button,action) in [(importButton,#selector(importSelected)),(allButton,#selector(readAll)),(cancelButton,#selector(cancel))] {
            button.target = self; button.action = action; button.bezelStyle = .rounded
        }
        importButton.keyEquivalent = "\r"; importButton.isEnabled = false; cancelButton.keyEquivalent = "\u{1b}"
        allButton.toolTip = "读取当前相册中列出的全部照片"
        network.font = .systemFont(ofSize:12); monitor.font = .systemFont(ofSize:12)
        network.toolTip = "关闭时不下载云端原件；无法读取的照片会跳过并说明原因。"
        monitor.toolTip = "仅在应用运行期间监测所选相册与转换方向，退出后停止。"
        let gap = NSView(); gap.setContentHuggingPriority(.init(1),for:.horizontal)
        let buttons = NSStackView(views:[cancelButton,gap,allButton,importButton]); buttons.orientation = .horizontal
        let title = NSTextField(labelWithString:"从图库添加照片"); title.font = .systemFont(ofSize:18,weight:.semibold)
        let albumLabel = NSTextField(labelWithString:"相册"), versionLabel = NSTextField(labelWithString:"版本")
        for label in [albumLabel,versionLabel] { label.textColor = .secondaryLabelColor; label.font = .systemFont(ofSize:13) }
        let choices = NSStackView(views:[albumLabel,albumPopup,versionLabel,versionPopup])
        choices.orientation = .horizontal; choices.spacing = 8
        albumPopup.setContentHuggingPriority(.init(1),for:.horizontal)
        versionPopup.widthAnchor.constraint(equalToConstant:230).isActive = true
        let titleGap = NSView(); titleGap.setContentHuggingPriority(.init(1),for:.horizontal)
        let heading = NSStackView(views:[title,titleGap,viewMode]); heading.orientation = .horizontal
        let options = NSStackView(views:[network,monitor]); options.orientation = .horizontal; options.spacing = 16
        message.font = .systemFont(ofSize:12); message.maximumNumberOfLines = 0
        message.textColor = .secondaryLabelColor
        libraryInfo.font = .systemFont(ofSize:12); libraryInfo.isSelectable = true
        readProgress.style = .bar; readProgress.isIndeterminate = false
        readProgress.minValue = 0; readProgress.maxValue = 100; readProgress.isHidden = true
        let content = NSStackView(views:[heading,choices,libraryInfo,message,photoBrowser,readProgress,options,buttons])
        content.orientation = .vertical; content.spacing = 8; content.alignment = .leading
        content.translatesAutoresizingMaskIntoConstraints = false
        sheet.contentView!.addSubview(content)
        NSLayoutConstraint.activate([
            content.topAnchor.constraint(equalTo:sheet.contentView!.topAnchor,constant:16),
            content.bottomAnchor.constraint(equalTo:sheet.contentView!.bottomAnchor,constant:-16),
            content.leadingAnchor.constraint(equalTo:sheet.contentView!.leadingAnchor,constant:16),
            content.trailingAnchor.constraint(equalTo:sheet.contentView!.trailingAnchor,constant:-16),
            photoBrowser.heightAnchor.constraint(greaterThanOrEqualToConstant:120)
        ])
        for view in [heading,choices,libraryInfo,message,photoBrowser,readProgress,buttons] {
            view.translatesAutoresizingMaskIntoConstraints = false
            view.widthAnchor.constraint(equalTo:content.widthAnchor).isActive = true
        }
        photoBrowser.setContentHuggingPriority(.init(1),for:.vertical)
        changeScope(); parent.beginSheet(sheet)
        DispatchQueue.main.async {
            self.sheet.contentView?.layoutSubtreeIfNeeded()
            self.scrollToFirstPhoto()
        }
    }

    private func updateSummary(imageCount: Int, count: Int, permission: String) {
        libraryInfo.stringValue = "当前范围 \(count) 张 · 图库共 \(imageCount) 张 · \(permission)"
        libraryInfo.toolTip = "图库包含个人图库、共享相簿和同步来源。数量不符时，请在「照片 → 设置 → 通用」确认系统照片图库。"
        message.stringValue = direction == .toApple ? "选择动态照片，按住 ⌘ 可多选；静态照片会跳过。" : "仅显示 Live Photo，按住 ⌘ 可多选。"
        message.toolTip = "原片保持不变。云端原件需勾选允许下载；缩略图不会触发下载。"
        emptyMessage.isHidden = count > 0
    }
    @objc private func changeScope() {
        guard !importing else { return }
        setSelection([])
        if let count = previewCount {
            reloadBrowsers(); allButton.isEnabled = count > 0
            scrollToFirstPhoto()
            updateSummary(imageCount:count,count:count,permission:"示例数据")
            return
        }
        selectedScope.albumID = albumPopup.indexOfSelectedItem > 0 ? albums[albumPopup.indexOfSelectedItem - 1].localIdentifier : nil
        selectedScope.version = versionPopup.indexOfSelectedItem == 1 ? .edited : .original
        assets = PhotoLibraryService.fetch(selectedScope); reloadBrowsers()
        scrollToFirstPhoto()
        allButton.isEnabled = assets.count > 0
        var allImagesScope = selectedScope; allImagesScope.albumID = nil; allImagesScope.direction = .toApple
        let imageCount = selectedScope.albumID == nil && direction == .toApple ? assets.count : PhotoLibraryService.fetch(allImagesScope).count
        let permission = PHPhotoLibrary.authorizationStatus(for: .readWrite) == .limited ? "部分访问" : "完全访问"
        updateSummary(imageCount:imageCount,count:assets.count,permission:permission)
    }
    func numberOfRows(in tableView: NSTableView) -> Int { previewCount ?? assets?.count ?? 0 }
    func tableView(_ tableView: NSTableView, viewFor column: NSTableColumn?, row: Int) -> NSView? {
        guard let column = column else { return nil }
        if let count = previewCount {
            guard row < count else { return nil }
            if column.identifier.rawValue == "image" {
                let view = NSImageView(); view.image = NSImage(systemSymbolName:"photo",accessibilityDescription:nil)
                return view
            }
            let field = NSTextField(labelWithString:"图库照片_\(row + 1).heic\n2026年10月27日 · 4032 × 3024")
            field.font = .systemFont(ofSize:12); field.maximumNumberOfLines = 2
            return field
        }
        guard row < assets.count else { return nil }; let asset = assets.object(at:row)
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
    private func reloadBrowsers() {
        syncingSelection = true
        table.reloadData(); collection.reloadData()
        syncingSelection = false
    }
    private func scrollToFirstPhoto() {
        guard assetCount > 0 else { return }
        if thumbnailMode {
            collection.scrollToItems(at:[IndexPath(item:0,section:0)],scrollPosition:.top)
        } else { table.scrollRowToVisible(0) }
    }
    @objc private func changeViewMode() {
        guard !importing else { return }
        listScroll.isHidden = thumbnailMode; gridScroll.isHidden = !thumbnailMode
        sheet.contentView?.layoutSubtreeIfNeeded()
        collection.collectionViewLayout?.invalidateLayout()
        setSelection(selectedIndexes)
        if let first = selectedIndexes.first {
            if thumbnailMode { collection.scrollToItems(at:[IndexPath(item:first,section:0)],scrollPosition:.top) }
            else { table.scrollRowToVisible(first) }
        } else { scrollToFirstPhoto() }
        sheet.makeFirstResponder(thumbnailMode ? collection : table)
    }
    private func setSelection(_ indexes: IndexSet) {
        guard !syncingSelection else { return }
        syncingSelection = true
        selectedIndexes = indexes
        table.selectRowIndexes(indexes,byExtendingSelection:false)
        collection.selectionIndexPaths = Set(indexes.map { IndexPath(item:$0,section:0) })
        syncingSelection = false
        importButton.isEnabled = !importing && !indexes.isEmpty
        importButton.title = indexes.isEmpty ? "读取所选" : "读取所选（\(indexes.count)）"
    }
    func tableViewSelectionDidChange(_ notification: Notification) {
        if !importing && !syncingSelection { setSelection(table.selectedRowIndexes) }
    }
    func collectionView(_ collectionView: NSCollectionView, didSelectItemsAt indexPaths: Set<IndexPath>) {
        if !syncingSelection { setSelection(IndexSet(collectionView.selectionIndexPaths.map(\.item))) }
    }
    func collectionView(_ collectionView: NSCollectionView, didDeselectItemsAt indexPaths: Set<IndexPath>) {
        if !syncingSelection { setSelection(IndexSet(collectionView.selectionIndexPaths.map(\.item))) }
    }
    func collectionView(_ collectionView: NSCollectionView, shouldSelectItemsAt indexPaths: Set<IndexPath>) -> Set<IndexPath> {
        importing ? [] : indexPaths
    }
    func collectionView(_ collectionView: NSCollectionView, shouldDeselectItemsAt indexPaths: Set<IndexPath>) -> Set<IndexPath> {
        importing ? [] : indexPaths
    }
    func collectionView(_ collectionView: NSCollectionView, numberOfItemsInSection section: Int) -> Int { assetCount }
    func collectionView(_ collectionView: NSCollectionView, layout collectionViewLayout: NSCollectionViewLayout, sizeForItemAt indexPath: IndexPath) -> NSSize {
        let available = max(120,gridScroll.contentSize.width - 16)
        let columns = max(1,Int((available + 8) / 144))
        return NSSize(width:floor((available - CGFloat(columns - 1) * 8) / CGFloat(columns)),height:132)
    }
    func collectionView(_ collectionView: NSCollectionView, itemForRepresentedObjectAt indexPath: IndexPath) -> NSCollectionViewItem {
        let item = collectionView.makeItem(withIdentifier:NSUserInterfaceItemIdentifier("thumbnail"),for:indexPath) as! LibraryThumbnailItem
        if previewCount != nil {
            item.configurePreview(indexPath.item)
        } else {
            item.configure(assets.object(at:indexPath.item),manager:images)
        }
        return item
    }
    @objc private func readAll() { setSelection(IndexSet(integersIn:0..<assetCount)); importSelected() }
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
        guard previewCount == nil, !importing, !selectedIndexes.isEmpty else { return }
        readProgress.isHidden = false; readProgress.doubleValue = 0
        importing = true; importButton.isEnabled = false; allButton.isEnabled = false; table.isEnabled = false; viewMode.isEnabled = false
        albumPopup.isEnabled = false; versionPopup.isEnabled = false; network.isEnabled = false; monitor.isEnabled = false
        selectedScope.networkAllowed = network.state == .on; wantsMonitoring = monitor.state == .on
        let selected = selectedIndexes.map { assets.object(at: $0) }, scope = selectedScope
        DispatchQueue.global(qos: .userInitiated).async {
            do {
                let result = try self.service.export(selected, scope: scope, store: self.store, token: self.token) { current, total, text in
                    DispatchQueue.main.async {
                        self.message.stringValue = text; self.onProgress(text)
                        self.readProgress.doubleValue = total > 0 ? Double(current) / Double(total) * 100 : 0
                    }
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

// Reusable, bounded previews. Never download iCloud images merely to browse.
private final class LibraryThumbnailItem: NSCollectionViewItem {
    private let photo = NSImageView()
    private let caption = NSTextField(labelWithString:"")
    private let check = NSImageView()
    private var manager: PHImageManager?
    private var request: PHImageRequestID = PHInvalidImageRequestID
    private var assetID: String?
    override var isSelected: Bool { didSet { updateSelection() } }
    override var highlightState: NSCollectionViewItem.HighlightState { didSet { updateSelection() } }

    override func loadView() {
        view = NSView(); view.wantsLayer = true
        view.layer?.cornerRadius = 6
        photo.imageScaling = .scaleProportionallyUpOrDown
        caption.font = .systemFont(ofSize:12); caption.lineBreakMode = .byTruncatingMiddle
        caption.maximumNumberOfLines = 1
        check.image = NSImage(systemSymbolName:"checkmark.circle.fill",accessibilityDescription:"已选择")
        check.contentTintColor = InterfaceStyle.nativeAccent
        for child in [photo,caption,check] { child.translatesAutoresizingMaskIntoConstraints = false; view.addSubview(child) }
        NSLayoutConstraint.activate([
            photo.topAnchor.constraint(equalTo:view.topAnchor,constant:4),
            photo.leadingAnchor.constraint(equalTo:view.leadingAnchor,constant:4),
            photo.trailingAnchor.constraint(equalTo:view.trailingAnchor,constant:-4),
            photo.bottomAnchor.constraint(equalTo:caption.topAnchor,constant:-4),
            caption.leadingAnchor.constraint(equalTo:view.leadingAnchor,constant:6),
            caption.trailingAnchor.constraint(equalTo:view.trailingAnchor,constant:-6),
            caption.bottomAnchor.constraint(equalTo:view.bottomAnchor,constant:-6),
            caption.heightAnchor.constraint(equalToConstant:16),
            check.topAnchor.constraint(equalTo:view.topAnchor,constant:8),
            check.trailingAnchor.constraint(equalTo:view.trailingAnchor,constant:-8),
            check.widthAnchor.constraint(equalToConstant:18),check.heightAnchor.constraint(equalToConstant:18)
        ])
        updateSelection()
    }
    private func reset() {
        _ = view
        if request != PHInvalidImageRequestID { manager?.cancelImageRequest(request) }
        request = PHInvalidImageRequestID; assetID = nil
        photo.image = NSImage(systemSymbolName:"photo",accessibilityDescription:nil)
    }
    func configurePreview(_ index: Int) {
        reset(); caption.stringValue = "图库照片_\(index + 1).heic"
        view.toolTip = caption.stringValue + "（示意照片）"
        photo.setAccessibilityLabel("示意照片")
    }
    func configure(_ asset: PHAsset, manager: PHImageManager) {
        reset(); self.manager = manager; assetID = asset.localIdentifier
        caption.stringValue = PHAssetResource.assetResources(for:asset).first(where: { $0.type == .photo })?.originalFilename ?? "照片"
        view.toolTip = caption.stringValue
        photo.setAccessibilityLabel(caption.stringValue)
        let options = PHImageRequestOptions(); options.isNetworkAccessAllowed = false
        options.resizeMode = .fast; options.deliveryMode = .opportunistic
        request = manager.requestImage(for:asset,targetSize:NSSize(width:320,height:240),contentMode:.aspectFit,options:options) { [weak self] image, _ in
            DispatchQueue.main.async {
                guard let self = self, self.assetID == asset.localIdentifier else { return }
                if let image = image { self.photo.image = image }
            }
        }
    }
    override func prepareForReuse() { super.prepareForReuse(); reset() }
    private func updateSelection() {
        guard isViewLoaded else { return }
        let selected = isSelected || highlightState == .forSelection
        view.layer?.borderWidth = selected ? 2 : 0
        view.layer?.borderColor = InterfaceStyle.nativeAccent.cgColor
        view.layer?.backgroundColor = (selected ? InterfaceStyle.nativeAccent.withAlphaComponent(0.12) : NSColor.clear).cgColor
        check.isHidden = !isSelected
    }
}
