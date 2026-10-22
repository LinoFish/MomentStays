import Foundation
import Photos
import ImageIO
import CoreLocation

struct LibraryScope: Codable, Equatable {
    var albumID: String?
    var direction: ConversionDirection
    var version: LibraryVersion
    var networkAllowed: Bool
    var key: String { "\(albumID ?? "system")|\(direction.rawValue)|\(version.rawValue)" }
}
struct LibraryReadResult {
    var root: URL
    var urls: [URL] = []
    var origins: [String: LibraryOrigin] = [:]
    var skipped = 0
    var deferred = 0
    var warnings: [String] = []
}

final class PhotoLibraryService {
    private let lock = NSLock()
    private var requests: Set<PHAssetResourceDataRequestID> = []
    func cancel() {
        lock.lock(); let values = requests; lock.unlock()
        for id in values { PHAssetResourceManager.default().cancelDataRequest(id) }
    }
    static func authorize(_ completion: @escaping (Result<Void, Error>) -> Void) {
        func finish(_ status: PHAuthorizationStatus) {
            DispatchQueue.main.async {
                completion(status == .authorized || status == .limited ? .success(()) : .failure(Failure("需要照片图库读写权限；请在系统设置中授权，也可继续使用文件夹输出。")))
            }
        }
        let status = PHPhotoLibrary.authorizationStatus(for: .readWrite)
        if status == .notDetermined { PHPhotoLibrary.requestAuthorization(for: .readWrite, handler: finish) }
        else { finish(status) }
    }
    static func albums() -> [PHAssetCollection] {
        let values = PHAssetCollection.fetchAssetCollections(with: .album, subtype: .any, options: nil)
        var result: [PHAssetCollection] = []
        values.enumerateObjects { album, _, _ in if album.canPerform(.addContent) { result.append(album) } }
        return result.sorted { ($0.localizedTitle ?? "") < ($1.localizedTitle ?? "") }
    }
    static func fetch(_ scope: LibraryScope) -> PHFetchResult<PHAsset> {
        let options = PHFetchOptions()
        let live = PHAssetMediaSubtype.photoLive.rawValue
        options.predicate = NSPredicate(format: "mediaType == %d AND (mediaSubtypes & %d) \(scope.direction == .toAndroid ? "!=" : "==") 0", PHAssetMediaType.image.rawValue, live)
        options.sortDescriptors = [NSSortDescriptor(key: "creationDate", ascending: false)]
        if let id = scope.albumID, let album = PHAssetCollection.fetchAssetCollections(withLocalIdentifiers: [id], options: nil).firstObject {
            return PHAsset.fetchAssets(in: album, options: options)
        }
        // A deleted selected album must not silently expand scope to the whole library.
        if scope.albumID != nil { return PHAsset.fetchAssets(withLocalIdentifiers: [], options: nil) }
        return PHAsset.fetchAssets(with: options)
    }
    static func version(_ asset: PHAsset, edited: Bool) -> String {
        let resources = PHAssetResource.assetResources(for: asset).map { "\($0.type.rawValue):\($0.originalFilename)" }.sorted().joined(separator: "|")
        return "\(asset.modificationDate?.timeIntervalSince1970 ?? asset.creationDate?.timeIntervalSince1970 ?? 0)|\(edited)|\(resources)"
    }
    static func origin(_ asset: PHAsset, edited: Bool, filename: String) -> LibraryOrigin {
        LibraryOrigin(assetID: asset.localIdentifier, version: version(asset, edited: edited), edited: edited,
            filename: filename, created: asset.creationDate, latitude: asset.location?.coordinate.latitude, longitude: asset.location?.coordinate.longitude)
    }
    private func read(_ resource: PHAssetResource, to url: URL, network: Bool, token: StopToken) throws {
        guard !token.isCancelled else { throw Failure("已取消图库读取") }
        guard FileManager.default.createFile(atPath: url.path, contents: nil) else { throw Failure("无法创建原件缓存") }
        let handle = try FileHandle(forWritingTo: url); defer { try? handle.close() }
        let semaphore = DispatchSemaphore(value: 0), writeLock = NSLock()
        var failure: Error?
        let options = PHAssetResourceRequestOptions(); options.isNetworkAccessAllowed = network
        let request = PHAssetResourceManager.default().requestData(for: resource, options: options, dataReceivedHandler: { data in
            writeLock.lock(); defer { writeLock.unlock() }
            guard failure == nil else { return }
            do { try handle.write(contentsOf: data) } catch { failure = error }
        }, completionHandler: { error in
            writeLock.lock(); if let error = error { failure = error }; writeLock.unlock(); semaphore.signal()
        })
        lock.lock(); requests.insert(request); lock.unlock()
        while semaphore.wait(timeout: .now() + 0.1) == .timedOut {
            if token.isCancelled { PHAssetResourceManager.default().cancelDataRequest(request) }
        }
        lock.lock(); requests.remove(request); lock.unlock()
        if let error = failure { throw error }
        guard !token.isCancelled else { throw Failure("已取消图库读取") }
    }
    // Called on a worker. Only selected candidates are materialized; cloud reads are opt-in.
    func export(_ assets: [PHAsset], scope: LibraryScope, store: DeliveryStore, token: StopToken,
                progress: @escaping (Int, Int, String) -> Void) throws -> LibraryReadResult {
        let fm = FileManager.default
        var result = LibraryReadResult(root: NativeBatch.normalized(fm.temporaryDirectory.appendingPathComponent("motionlive-photos-" + UUID().uuidString)))
        try fm.createDirectory(at: result.root, withIntermediateDirectories: true)
        let excluded = store.generatedAssetIDs
        for (index, asset) in assets.enumerated() {
            if token.isCancelled { break }
            let resources = PHAssetResource.assetResources(for: asset)
            progress(index, assets.count, "读取原件 \(index + 1) / \(assets.count)")
            if excluded.contains(asset.localIdentifier) { result.skipped += 1; continue }
            let edited = scope.version == .edited
            let photo = resources.first { $0.type == (edited ? .fullSizePhoto : .photo) }
            let movie = resources.first { $0.type == (edited ? .fullSizePairedVideo : .pairedVideo) }
            let folder = result.root.appendingPathComponent(String(index))
            do {
                guard let photo = photo, scope.direction != .toAndroid || movie != nil else {
                    throw Failure(edited ? "缺少完整编辑版配对；未混用原片" : "缺少完整原片资源")
                }
                let filename = URL(fileURLWithPath: photo.originalFilename).lastPathComponent
                guard ["jpg", "jpeg", "heic", "heif"].contains(URL(fileURLWithPath: filename).pathExtension.lowercased()) else { result.skipped += 1; continue }
                try fm.createDirectory(at: folder, withIntermediateDirectories: true)
                let image = folder.appendingPathComponent(filename)
                try read(photo, to: image, network: scope.networkAllowed, token: token)
                if scope.direction == .toAndroid, let movie = movie {
                    var clip = folder.appendingPathComponent(URL(fileURLWithPath: movie.originalFilename).lastPathComponent)
                    if clip == image { clip = folder.appendingPathComponent("paired.mov") }
                    try read(movie, to: clip, network: scope.networkAllowed, token: token)
                    _ = try AndroidExport.pair(image)
                } else {
                    do { _ = try PhotoFormat.parse(Array(Data(contentsOf: image))) }
                    catch {
                        if error.localizedDescription == PhotoFormat.notMotion {
                            result.skipped += 1; try? fm.removeItem(at: folder)
                            try store.inspected(asset.localIdentifier, version: Self.version(asset, edited: edited), scope: scope.key)
                            progress(index + 1, assets.count, "静态照片已跳过"); continue
                        }
                        // Keep malformed motion candidates in the queue for an actionable scan reason.
                    }
                }
                if let date = asset.creationDate { try fm.setAttributes([.modificationDate: date], ofItemAtPath: image.path) }
                try store.inspected(asset.localIdentifier, version: Self.version(asset, edited: edited), scope: scope.key)
                result.urls.append(image)
                result.origins[image.path] = Self.origin(asset, edited: edited, filename: filename)
                progress(index + 1, assets.count, "已识别 \(result.urls.count) 张动态候选 · 静态跳过 \(result.skipped)")
            } catch {
                try? fm.removeItem(at: folder)
                if !token.isCancelled {
                    result.deferred += 1
                    result.warnings.append("\(photo?.originalFilename ?? "照片")：\(scope.networkAllowed ? "" : "未允许下载 iCloud 原件；")\(error.localizedDescription)")
                }
            }
        }
        if result.urls.isEmpty { try? fm.removeItem(at: result.root) }
        return result
    }

    private static func fileMetadata(_ url: URL) -> (Date?, CLLocation?) {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
              let props = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [String: Any] else { return (nil, nil) }
        var date: Date?
        if let exif = props[kCGImagePropertyExifDictionary as String] as? [String: Any], let text = exif[kCGImagePropertyExifDateTimeOriginal as String] as? String {
            let formatter = DateFormatter(); formatter.locale = Locale(identifier: "en_US_POSIX")
            formatter.dateFormat = "yyyy:MM:dd HH:mm:ss"; date = formatter.date(from: text)
        }
        var location: CLLocation?
        if let gps = props[kCGImagePropertyGPSDictionary as String] as? [String: Any],
           let lat = (gps[kCGImagePropertyGPSLatitude as String] as? NSNumber)?.doubleValue,
           let lon = (gps[kCGImagePropertyGPSLongitude as String] as? NSNumber)?.doubleValue {
            location = CLLocation(latitude: gps[kCGImagePropertyGPSLatitudeRef as String] as? String == "S" ? -lat : lat,
                longitude: gps[kCGImagePropertyGPSLongitudeRef as String] as? String == "W" ? -lon : lon)
        }
        return (date, location)
    }
    private func change(_ changes: @escaping () -> Void) throws {
        let semaphore = DispatchSemaphore(value: 0); var failure: Error?
        PHPhotoLibrary.shared().performChanges(changes) { success, error in
            if !success { failure = error ?? Failure("照片图库写入失败") }; semaphore.signal()
        }
        semaphore.wait(); if let failure = failure { throw failure }
    }
    private func album(_ settings: ConversionSettings) throws -> PHAssetCollection {
        if let id = settings.albumID {
            guard let value = PHAssetCollection.fetchAssetCollections(withLocalIdentifiers: [id], options: nil).firstObject,
                  value.canPerform(.addContent) else { throw Failure("目标相册已删除或不可写；请选择相册后重试导入") }
            return value
        }
        let options = PHFetchOptions(); options.predicate = NSPredicate(format: "title == %@", settings.albumTitle)
        let matches = PHAssetCollection.fetchAssetCollections(with: .album, subtype: .albumRegular, options: options)
        if let first = matches.firstObject { return first }
        var id: String?
        try change { id = PHAssetCollectionChangeRequest.creationRequestForAssetCollection(withTitle: settings.albumTitle).placeholderForCreatedAssetCollection.localIdentifier }
        guard let id = id, let created = PHAssetCollection.fetchAssetCollections(withLocalIdentifiers: [id], options: nil).firstObject else { throw Failure("相册创建后无法读取") }
        return created
    }
    // Each media pair is one Photos transaction. Album association is retriable separately.
    func importResults(_ task: inout DeliveryTask, store: DeliveryStore, token: StopToken,
                       progress: @escaping (String) -> Void) throws -> String {
        try store.validate()
        guard PHPhotoLibrary.authorizationStatus(for: .readWrite) == .authorized || PHPhotoLibrary.authorizationStatus(for: .readWrite) == .limited else { throw Failure("图库权限已撤销，请重新授权后重试导入") }
        let collection = try album(task.settings)
        var imported = 0, existing = 0, failures: [String] = []
        for (index, file) in task.results.enumerated() {
            if token.isCancelled { break }
            progress("导入相册 \(index + 1) / \(task.results.count)")
            do {
                var record = store.records()[file.key]
                var asset: PHAsset?
                if let id = record?.assetID { asset = PHAsset.fetchAssets(withLocalIdentifiers: [id], options: nil).firstObject }
                let shouldCreate = try LibraryImportPolicy.shouldCreate(record, assetExists: asset != nil)
                if shouldCreate {
                    try file.verify()
                    guard file.files.count == 2 else { throw Failure("缺少完整实况配对") }
                    let still = URL(fileURLWithPath: file.files[0]), movie = URL(fileURLWithPath: file.files[1])
                    let paired = try AndroidExport.pair(still)
                    guard paired.movie.standardizedFileURL == movie.standardizedFileURL else { throw Failure("配对视频不匹配") }
                    if PhotoHEIF.matches(Array(try Data(contentsOf: still))) { _ = try PhotoHEIF.layout(Array(Data(contentsOf: still))) }
                    else { guard try MLJPEG.staticOnly(Array(Data(contentsOf: still))) == Array(Data(contentsOf: still)) else { throw Failure("静态图片仍含嵌入视频或额外载荷") } }
                    var pending = LibraryImportRecord(key: file.key, state: "pending")
                    try store.save(pending)
                    let fallback = Self.fileMetadata(still)
                    do {
                        try change {
                            let request = PHAssetCreationRequest.forAsset()
                            let imageOptions = PHAssetResourceCreationOptions(); imageOptions.originalFilename = still.lastPathComponent; imageOptions.shouldMoveFile = false
                            let movieOptions = PHAssetResourceCreationOptions(); movieOptions.originalFilename = movie.lastPathComponent; movieOptions.shouldMoveFile = false
                            request.creationDate = file.origin?.created ?? fallback.0
                            if let lat = file.origin?.latitude, let lon = file.origin?.longitude { request.location = CLLocation(latitude: lat, longitude: lon) }
                            else { request.location = fallback.1 }
                            // Persist the placeholder before the block can commit. If that fails,
                            // never attach resources: validation fails rather than creating an untracked pair.
                            guard let placeholder = request.placeholderForCreatedAsset?.localIdentifier else {
                                pending.message = "无法取得入库资产标识，未附加资源"; return
                            }
                            pending.assetID = placeholder
                            pending.state = "submitting"
                            do { try store.save(pending) }
                            catch { pending.message = error.localizedDescription; return }
                            request.addResource(with: .photo, fileURL: still, options: imageOptions)
                            request.addResource(with: .pairedVideo, fileURL: movie, options: movieOptions)
                        }
                    } catch {
                        pending.state = "pending"; pending.assetID = nil; pending.message = error.localizedDescription
                        try store.save(pending); throw error
                    }
                    guard let id = pending.assetID, let created = PHAsset.fetchAssets(withLocalIdentifiers: [id], options: nil).firstObject else {
                        pending.state = "needs-review"; try store.save(pending); throw Failure("入库完成但新资产不可读，需核对")
                    }
                    asset = created; pending.state = "created"; try store.save(pending); record = pending; imported += 1
                } else { existing += 1 }
                guard let asset = asset, asset.mediaSubtypes.contains(.photoLive) else { throw Failure("入库结果未识别为 Live Photo") }
                let resources = PHAssetResource.assetResources(for: asset)
                guard resources.contains(where: { $0.type == .photo }) && resources.contains(where: { $0.type == .pairedVideo }),
                      resources.first(where: { $0.type == .photo })?.originalFilename == URL(fileURLWithPath: file.files[0]).lastPathComponent,
                      resources.first(where: { $0.type == .pairedVideo })?.originalFilename == URL(fileURLWithPath: file.files[1]).lastPathComponent else { throw Failure("入库结果的配对资源或文件名未通过校验") }
                if let expected = file.origin?.created, abs((asset.creationDate ?? .distantPast).timeIntervalSince(expected)) > 1 { throw Failure("入库拍摄时间不一致") }
                if let lat = file.origin?.latitude, let lon = file.origin?.longitude {
                    guard let loc = asset.location, abs(loc.coordinate.latitude - lat) < 0.00001, abs(loc.coordinate.longitude - lon) < 0.00001 else { throw Failure("入库位置不一致") }
                }
                let members = PHAsset.fetchAssets(in: collection, options: nil)
                var inAlbum = false; members.enumerateObjects { item, _, stop in if item.localIdentifier == asset.localIdentifier { inAlbum = true; stop.pointee = true } }
                if !inAlbum { try change { PHAssetCollectionChangeRequest(for: collection)?.addAssets([asset] as NSArray) } }
                let check = PHAsset.fetchAssets(in: collection, options: nil)
                var confirmed = false; check.enumerateObjects { item, _, stop in if item.localIdentifier == asset.localIdentifier { confirmed = true; stop.pointee = true } }
                guard confirmed else { throw Failure("照片已入库但加入相册未确认，可重试关联") }
                var completed = record ?? LibraryImportRecord(key: file.key, assetID: asset.localIdentifier, state: "created")
                completed.albumID = collection.localIdentifier; completed.state = "completed"; completed.message = ""
                try store.save(completed)
            } catch { failures.append(URL(fileURLWithPath: file.source).lastPathComponent + "：" + error.localizedDescription) }
        }
        let summary = "入库新增 \(imported) · 已有 \(existing) · 未完成 \(failures.count)" + (token.isCancelled ? " · 已停止" : "")
        task.importSummary = summary + (failures.isEmpty ? "" : "\n" + failures.joined(separator: "\n")); try store.save(task)
        return task.importSummary
    }
}

final class PhotoLibraryMonitor: NSObject, PHPhotoLibraryChangeObserver {
    private var fetched: PHFetchResult<PHAsset>
    let scope: LibraryScope
    var onChange: ([PHAsset]) -> Void = { _ in }
    init(scope: LibraryScope) {
        self.scope = scope; fetched = PhotoLibraryService.fetch(scope)
        super.init(); PHPhotoLibrary.shared().register(self)
    }
    deinit { PHPhotoLibrary.shared().unregisterChangeObserver(self) }
    func photoLibraryDidChange(_ changeInstance: PHChange) {
        DispatchQueue.main.async {
            guard let changes = changeInstance.changeDetails(for: self.fetched) else { return }
            let before = self.fetched; self.fetched = changes.fetchResultAfterChanges
            if changes.hasIncrementalChanges { self.onChange(changes.insertedObjects + changes.changedObjects) }
            else {
                var old: [String: String] = [:]; before.enumerateObjects { a, _, _ in old[a.localIdentifier] = PhotoLibraryService.version(a, edited: self.scope.version == .edited) }
                var changed: [PHAsset] = []; self.fetched.enumerateObjects { a, _, _ in if old[a.localIdentifier] != PhotoLibraryService.version(a, edited: self.scope.version == .edited) { changed.append(a) } }
                self.onChange(changed)
            }
        }
    }
}
