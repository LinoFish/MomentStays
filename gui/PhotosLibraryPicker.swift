import AppKit
import Photos

// PHPicker needs macOS 13. This macOS 12 picker reads only selected asset
// resources, never writes to the user's library or substitutes a preview JPEG.
final class PhotosLibraryPicker: NSObject, NSTableViewDataSource, NSTableViewDelegate {
    private var sheet: NSWindow!
    private let table = NSTableView()
    private let message = NSTextField(labelWithString:"选择实况照片；iCloud 原件可能需要下载。图库不会被修改。")
    private let importButton = NSButton(title:"导入选中实况",target:nil,action:nil)
    private let cancelButton = NSButton(title:"取消",target:nil,action:nil)
    private var assets: PHFetchResult<PHAsset>!
    private let images = PHCachingImageManager()
    private let token = StopToken()
    private var importing = false
    private let lock = NSLock()
    private var requests: [PHAssetResourceDataRequestID] = []
    private let completion: ([URL],Error?)->Void
    private weak var parent: NSWindow?
    init(completion:@escaping ([URL],Error?)->Void) { self.completion = completion }
    func present(_ parent:NSWindow) {
        self.parent = parent
        func proceed(_ status:PHAuthorizationStatus) {
            DispatchQueue.main.async {
                guard status == .authorized || status == .limited else {
                    self.completion([],Failure("没有照片图库读取权限。可在系统设置授权，或使用导出的完整 HEIC/JPEG + MOV 配对文件。")); return
                }
                self.build(parent)
            }
        }
        let status = PHPhotoLibrary.authorizationStatus(for:.readWrite)
        if status == .notDetermined { PHPhotoLibrary.requestAuthorization(for:.readWrite,handler:proceed) }
        else { proceed(status) }
    }
    private func build(_ parent:NSWindow) {
        let options = PHFetchOptions()
        options.predicate = NSPredicate(format:"(mediaSubtypes & %d) != 0",PHAssetMediaSubtype.photoLive.rawValue)
        options.sortDescriptors = [NSSortDescriptor(key:"creationDate",ascending:false)]
        assets = PHAsset.fetchAssets(with:.image,options:options)
        sheet = NSWindow(contentRect:NSRect(x:0,y:0,width:680,height:480),styleMask:[.titled,.resizable],backing:.buffered,defer:false)
        sheet.title = "从「照片」选择实况"; sheet.minSize = NSSize(width:480,height:360)
        table.delegate = self; table.dataSource = self; table.rowHeight = 54; table.allowsMultipleSelection = true; table.style = .fullWidth
        for (id,title,width) in [("image","照片",70.0),("name","文件 / 拍摄时间",440.0),("size","尺寸",120.0)] {
            let column = NSTableColumn(identifier:NSUserInterfaceItemIdentifier(id)); column.title = title; column.width = width; table.addTableColumn(column)
        }
        let scroll = NSScrollView(); scroll.hasVerticalScroller = true; scroll.documentView = table
        importButton.target = self; importButton.action = #selector(importSelected); importButton.bezelStyle = .rounded; importButton.keyEquivalent = "\r"; importButton.isEnabled = false
        cancelButton.target = self; cancelButton.action = #selector(cancel); cancelButton.bezelStyle = .rounded; cancelButton.keyEquivalent = "\u{1b}"
        let gap = NSView(); gap.setContentHuggingPriority(.init(1),for:.horizontal)
        let buttons = NSStackView(views:[cancelButton,gap,importButton]); buttons.orientation = .horizontal
        message.font = .systemFont(ofSize:12); message.lineBreakMode = .byTruncatingMiddle
        let content = NSStackView(views:[message,scroll,buttons]); content.orientation = .vertical; content.spacing = 12; content.alignment = .leading
        content.translatesAutoresizingMaskIntoConstraints = false; sheet.contentView!.addSubview(content)
        NSLayoutConstraint.activate([content.topAnchor.constraint(equalTo:sheet.contentView!.topAnchor,constant:14),content.bottomAnchor.constraint(equalTo:sheet.contentView!.bottomAnchor,constant:-14),content.leadingAnchor.constraint(equalTo:sheet.contentView!.leadingAnchor,constant:14),content.trailingAnchor.constraint(equalTo:sheet.contentView!.trailingAnchor,constant:-14)])
        for child in content.arrangedSubviews { child.translatesAutoresizingMaskIntoConstraints = false; child.widthAnchor.constraint(equalTo:content.widthAnchor).isActive = true }
        scroll.setContentHuggingPriority(.init(1),for:.vertical)
        if assets.count == 0 { message.stringValue = "图库中未找到可读取的实况照片。可取消后选择导出的完整原片配对。" }
        parent.beginSheet(sheet)
    }
    func numberOfRows(in tableView:NSTableView) -> Int { assets?.count ?? 0 }
    func tableView(_ tableView:NSTableView, viewFor column:NSTableColumn?, row:Int) -> NSView? {
        guard row < assets.count, let column = column else { return nil }; let asset = assets.object(at:row)
        if column.identifier.rawValue == "image" {
            let view = NSImageView(); view.imageScaling = .scaleProportionallyUpOrDown
            images.requestImage(for:asset,targetSize:NSSize(width:88,height:88),contentMode:.aspectFit,options:nil) { [weak view] image,_ in DispatchQueue.main.async { view?.image = image } }
            return view
        }
        let field = NSTextField(labelWithString:""); field.font = .systemFont(ofSize:12); field.lineBreakMode = .byTruncatingMiddle
        if column.identifier.rawValue == "size" { field.stringValue = "\(asset.pixelWidth) × \(asset.pixelHeight)" }
        else {
            let resource = PHAssetResource.assetResources(for:asset).first { $0.type == .photo }
            let date = asset.creationDate.map { DateFormatter.localizedString(from:$0,dateStyle:.medium,timeStyle:.short) } ?? "未知时间"
            field.stringValue = (resource?.originalFilename ?? "实况照片") + "\n" + date
            field.maximumNumberOfLines = 2
        }
        return field
    }
    func tableViewSelectionDidChange(_ notification:Notification) {
        guard !importing else { return }; let count = table.selectedRowIndexes.count
        importButton.isEnabled = count > 0; importButton.title = count > 0 ? "导入 \(count) 张实况" : "导入选中实况"
    }
    @objc private func cancel() {
        if importing {
            token.cancel(); lock.lock(); let pending = requests; lock.unlock()
            for request in pending { PHAssetResourceManager.default().cancelDataRequest(request) }
            message.stringValue = "正在停止资源读取…"; cancelButton.isEnabled = false
        } else { finish([],nil) }
    }
    private func finish(_ urls:[URL],_ error:Error?) { parent?.endSheet(sheet); sheet?.orderOut(nil); completion(urls,error) }
    private func resource(_ resource:PHAssetResource,to url:URL) throws {
        guard !token.isCancelled else { throw Failure("已取消图库导入") }
        FileManager.default.createFile(atPath:url.path,contents:nil)
        let handle = try FileHandle(forWritingTo:url); defer { try? handle.close() }
        let done = DispatchSemaphore(value:0), writeLock = NSLock(); var failure: Error?
        let options = PHAssetResourceRequestOptions(); options.isNetworkAccessAllowed = true
        let request = PHAssetResourceManager.default().requestData(for:resource,options:options,dataReceivedHandler: { data in
            writeLock.lock(); defer { writeLock.unlock() }
            if failure == nil { do { try handle.write(contentsOf:data) } catch { failure = error } }
        },completionHandler: { error in writeLock.lock(); if let error = error { failure = error }; writeLock.unlock(); done.signal() })
        lock.lock(); requests.append(request); lock.unlock()
        if token.isCancelled { PHAssetResourceManager.default().cancelDataRequest(request) }
        done.wait()
        lock.lock(); requests.removeAll { $0 == request }; lock.unlock()
        if let error = failure { throw error }
        guard !token.isCancelled else { throw Failure("已取消图库导入") }
    }
    @objc private func importSelected() {
        guard !importing else { return }; importing = true; importButton.isEnabled = false; table.isEnabled = false
        let selected = table.selectedRowIndexes.map { assets.object(at:$0) }
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("motionlive-photos-" + UUID().uuidString)
        DispatchQueue.global(qos:.userInitiated).async {
            do {
                var urls: [URL] = []
                for (i,asset) in selected.enumerated() {
                    DispatchQueue.main.async { self.message.stringValue = "读取实况资源 \(i + 1) / \(selected.count)…（可能下载 iCloud 原件）" }
                    let resources = PHAssetResource.assetResources(for:asset)
                    let editedPhoto = resources.first { $0.type == .fullSizePhoto }, editedVideo = resources.first { $0.type == .fullSizePairedVideo }
                    let photo = (editedPhoto != nil && editedVideo != nil) ? editedPhoto : resources.first { $0.type == .photo }
                    let video = (editedPhoto != nil && editedVideo != nil) ? editedVideo : resources.first { $0.type == .pairedVideo }
                    guard let photo = photo, let video = video else { throw Failure("选中资源缺少完整实况配对，未用缩略图替代") }
                    let folder = root.appendingPathComponent(String(i)); try FileManager.default.createDirectory(at:folder,withIntermediateDirectories:true)
                    let imageURL = folder.appendingPathComponent(URL(fileURLWithPath:photo.originalFilename).lastPathComponent)
                    var videoURL = folder.appendingPathComponent(URL(fileURLWithPath:video.originalFilename).lastPathComponent)
                    if videoURL == imageURL { videoURL = folder.appendingPathComponent("paired.mov") }
                    try self.resource(photo,to:imageURL); try self.resource(video,to:videoURL)
                    if let date = asset.creationDate { for url in [imageURL,videoURL] { try FileManager.default.setAttributes([.modificationDate:date],ofItemAtPath:url.path) } }
                    _ = try AndroidExport.pair(imageURL); urls.append(imageURL)
                }
                DispatchQueue.main.async { self.finish(urls,nil) }
            } catch {
                try? FileManager.default.removeItem(at:root)
                DispatchQueue.main.async { self.finish([],error) }
            }
        }
    }
}
