#if canImport(Flutter)
import Flutter
import UIKit
import Photos
import PhotosUI
import Combine
import UniformTypeIdentifiers

/// Retained by the channel handlers. Call once per Flutter engine on the main thread.
@MainActor public final class FlutterNativeBridge: NSObject, FlutterStreamHandler, PHPickerViewControllerDelegate, UIDocumentPickerDelegate {
    private let methods: FlutterMethodChannel
    private let events: FlutterEventChannel
    private var sink: FlutterEventSink?
    private let scanEvents: FlutterEventChannel
    private let scanStream = ScanStream()
    private var scanTask: Task<Void, Never>?
    private var scanSnapshot: PhotoScanProgress?
    private var stagedInputs: [String: URL] = [:]
    private var pickerResult: FlutterResult?
    private var pickerDirection = ""
    private var exporting = false
    private var exportCount = 0
    private var inputs: [String: BatchInput] = [:]
    private var works: [UUID: BatchWork] = [:]
    private var outputOwners: [String: UUID] = [:]
    private var recoveryOwners: [String: UUID] = [:]
    private var active: UUID?
    private var recoveryWaiters: [([String], FlutterResult)] = []
    private var saving = false
    private var observation: AnyCancellable?
    private let engine: any ConversionEngine

    public static func attach(to messenger: FlutterBinaryMessenger) {
        _ = FlutterNativeBridge(messenger: messenger)
    }
    private init(messenger: FlutterBinaryMessenger) {
        methods = FlutterMethodChannel(name: "momentstays/native", binaryMessenger: messenger)
        events = FlutterEventChannel(name: "momentstays/batch", binaryMessenger: messenger)
        scanEvents = FlutterEventChannel(name: "momentstays/scan", binaryMessenger: messenger)
        #if MOTIONLIVE_NATIVE_ENGINE
        engine = NativeConversionAdapter()
        #else
        engine = UnavailableEngine()
        #endif
        super.init()
        PhotoSession.removeExpired()
        methods.setMethodCallHandler { [self] call, result in handle(call, result: result) }
        events.setStreamHandler(self)
        scanEvents.setStreamHandler(scanStream)
    }
    public func onListen(withArguments arguments: Any?, eventSink events: @escaping FlutterEventSink) -> FlutterError? { sink = events; return nil }
    public func onCancel(withArguments arguments: Any?) -> FlutterError? { sink = nil; return nil }
    private func error(_ code: String, _ message: String) -> FlutterError { FlutterError(code: code, message: message, details: nil) }
    private func handle(_ call: FlutterMethodCall, result: @escaping FlutterResult) {
        let args = call.arguments as? [String: Any] ?? [:]
        switch call.method {
        case "capabilities":
            #if MOTIONLIVE_NATIVE_ENGINE
            let conversion = true
            #else
            let conversion = false
            #endif
            result(["photos": true, "files": true, "scan": conversion, "conversion": conversion, "share": true, "saveToPhotos": true, "saveToFiles": true, "shareLivePhoto": false, "saveLivePhotoToFiles": false])
        case "takePendingShare":
            guard pickerResult == nil, active == nil, !saving else { result(error("busy", "Another operation is in progress.")); return }
            do { result(try consumeHandoff()) }
            catch { result(self.error("handoff_failed", error.localizedDescription)) }
        case "pick":
            guard let source = args["source"] as? String, let direction = args["direction"] as? String, ["motionToLive", "liveToMotion"].contains(direction) else { result(error("invalid", "Invalid selection request.")); return }
            guard pickerResult == nil, active == nil, !saving else { result(error("busy", "Another operation is in progress.")); return }
            if source == "scan" {
                guard direction == "motionToLive" else { result(error("unsupported", "Scanning is only available for Motion Photos.")); return }
                #if MOTIONLIVE_NATIVE_ENGINE
                pickerResult = result
                startScan()
                #else
                result(error("unavailable", "Scanning is not available in this build."))
                #endif
                return
            }
            if source == "files" {
                guard direction == "motionToLive" else { result(error("unsupported", "Live Photos require Photos library selection.")); return }
                guard let presenter = presenter() else { result(error("unavailable", "No active view controller.")); return }
                pickerResult = result
                let picker = UIDocumentPickerViewController(forOpeningContentTypes: [.image], asCopy: false)
                picker.allowsMultipleSelection = true; picker.delegate = self
                presenter.present(picker, animated: true)
                return
            }
            guard source == "photos" else { result(error("unsupported", "Selection source is not supported.")); return }
            pickerResult = result; pickerDirection = direction
            Task {
                let access = await PHPhotoLibrary.requestAuthorization(for: .readWrite)
                guard access == .authorized || access == .limited else { self.finishPicker(self.error("permission", "Photos access is required.")); return }
                guard let presenter = self.presenter() else { self.finishPicker(self.error("unavailable", "No active view controller.")); return }
                var config = PHPickerConfiguration(photoLibrary: .shared())
                config.selectionLimit = 0
                config.filter = direction == "liveToMotion" ? .livePhotos : .images
                let picker = PHPickerViewController(configuration: config); picker.delegate = self
                presenter.present(picker, animated: true)
            }
        case "convert":
            #if !MOTIONLIVE_NATIVE_ENGINE
            result(error("unavailable", "Conversion is not available in this build.")); return
            #else
            guard active == nil, pickerResult == nil, !saving, sink != nil else { result(error("busy", "A batch is running or the event stream is not listening.")); return }
            guard let ids = args["ids"] as? [String], !ids.isEmpty, Set(ids).count == ids.count, let direction = args["direction"] as? String, let brand = args["brand"] as? String else { result(error("invalid", "Invalid batch.")); return }
            let selected = ids.compactMap { inputs[$0] }
            guard selected.count == ids.count, selected.allSatisfy({ input in
                switch input.source { case .livePhoto, .livePair: return direction == "liveToMotion"; case .originalPhoto, .scannedPhoto, .folderFile: return direction == "motionToLive" }
            }) else { result(error("invalid", "Inputs are missing or have the wrong direction.")); return }
            let id = UUID(); let work = BatchWork(inputs: selected, engine: engine); works[id] = work; recoveryOwners[ids.sorted().joined(separator: ",")] = id; active = id
            observation = work.$rows.sink { [weak self, weak work] rows in
                guard let self, let work else { return }; self.emit(work, rows: rows, finished: false)
            }
            result(nil)
            Task { await work.start(brand: brand); self.emit(work, finished: true); self.observation = nil; self.active = nil; self.finishRecoveryWaiters() }
            #endif
        case "stop":
            if let id = active { works[id]?.stopAfterCurrent() }; scanTask?.cancel(); result(nil)
        case "recoverBatch":
            guard let ids = args["ids"] as? [String], !ids.isEmpty, Set(ids).count == ids.count else { result(error("invalid", "Invalid batch recovery request.")); return }
            if active != nil { recoveryWaiters.append((ids, result)) }
            else { recoverBatch(ids, result: result) }
        case "discard":
            guard active == nil, pickerResult == nil, !saving else { result(error("busy", "Wait for the current operation to finish first.")); return }
            let ids = args["ids"] as? [String] ?? []
            for id in ids { inputs.removeValue(forKey: id); outputOwners.removeValue(forKey: id); if let url = stagedInputs.removeValue(forKey: id) { PhotoSession.remove(containing: url) } }
            // A BatchWork owns the entire batch; retain it while any result is still referenced.
            for id in Array(works.keys) where !outputOwners.values.contains(id) {
                works.removeValue(forKey: id)?.discard()
                recoveryOwners = recoveryOwners.filter { $0.value != id }
            }
            result(nil)
        case "saveToPhotos":
            guard active == nil, pickerResult == nil, !saving else { result(error("busy", "Wait for the current operation to finish.")); return }
            let ids = args["ids"] as? [String] ?? []
            let outputs = ids.compactMap { output(for: $0) }
            guard !ids.isEmpty, outputs.count == ids.count, outputs.allSatisfy({ $0.direction == .androidToLive && $0.files.count == 2 }) else { result(error("unsupported", "Only converted Live Photos can be saved to Photos.")); return }
            saving = true
            Task {
                defer { self.saving = false }
                do {
                    try await PHPhotoLibrary.shared().performChanges {
                        for output in outputs {
                            let request = PHAssetCreationRequest.forAsset()
                            request.addResource(with: .photo, fileURL: output.files[0], options: nil)
                            request.addResource(with: .pairedVideo, fileURL: output.files[1], options: nil)
                        }
                    }
                    result(nil)
                } catch { result(self.error("save_failed", error.localizedDescription)) }
            }
        case "share", "saveToFiles":
            guard active == nil, pickerResult == nil, !saving else { result(error("busy", "Wait for the current operation to finish.")); return }
            let ids = args["ids"] as? [String] ?? []
            let outputs = ids.compactMap { output(for: $0) }
            guard !ids.isEmpty, Set(ids).count == ids.count, outputs.count == ids.count,
                  outputs.allSatisfy({ $0.direction == .liveToAndroid && $0.files.count == 1 && FileManager.default.fileExists(atPath: $0.files[0].path) }) else {
                result(error("unsupported", "Only converted Motion Photo files can be exported or shared. Live Photo pairs cannot be shared here.")); return
            }
            guard let presenter = presenter() else { result(error("unavailable", "No active view controller.")); return }
            let urls = outputs.flatMap { $0.files }
            if call.method == "saveToFiles" {
                pickerResult = result; exporting = true; exportCount = urls.count
                let picker = UIDocumentPickerViewController(forExporting: urls, asCopy: true)
                picker.delegate = self
                presenter.present(picker, animated: true)
            } else {
                saving = true
                let sheet = UIActivityViewController(activityItems: urls, applicationActivities: nil)
                if let popover = sheet.popoverPresentationController {
                    popover.sourceView = presenter.view
                    popover.sourceRect = CGRect(x: presenter.view.bounds.midX, y: presenter.view.bounds.midY, width: 1, height: 1)
                }
                sheet.completionWithItemsHandler = { [self] _, completed, _, failure in
                    saving = false
                    if let failure { result(error("share_failed", failure.localizedDescription)) }
                    else if completed { result(nil) }
                    else { result(error("cancelled", "Sharing was cancelled.")) }
                }
                presenter.present(sheet, animated: true)
            }
        default: result(FlutterMethodNotImplemented)
        }
    }
    /// Claim only after a complete private copy exists; a copy failure leaves the
    /// extension's manifest intact for a later retry.
    private func consumeHandoff() throws -> [String: Any]? {
        guard let group = FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: "group.com.app9x3k2q.momentstays") else { throw ConversionFailure.unavailable }
        let manifest = group.appendingPathComponent("pending.json")
        guard FileManager.default.fileExists(atPath: manifest.path) else { return nil }
        guard let data = try? Data(contentsOf: manifest),
              let object = (try? JSONSerialization.jsonObject(with: data)) as? [String: String],
              let directoryName = object["directory"], directoryName.hasPrefix("Handoff-"),
              UUID(uuidString: String(directoryName.dropFirst("Handoff-".count))) != nil,
              let photoName = object["photo"], photoName == URL(fileURLWithPath: photoName).lastPathComponent,
              ["jpg", "jpeg", "heic", "heif"].contains(URL(fileURLWithPath: photoName).pathExtension.lowercased()) else {
            try? FileManager.default.removeItem(at: manifest); throw ConversionFailure.unreadable
        }
        let folder = group.appendingPathComponent(directoryName, isDirectory: true)
        let folderValues = try folder.resourceValues(forKeys: [.isDirectoryKey, .isSymbolicLinkKey])
        guard folderValues.isDirectory == true, folderValues.isSymbolicLink != true else { throw ConversionFailure.unreadable }
        let source = folder.appendingPathComponent(photoName)
        func validate(_ url: URL) throws {
            let values = try url.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey])
            guard values.isRegularFile == true, values.isSymbolicLink != true else { throw ConversionFailure.unreadable }
        }
        try validate(source)
        let videoName = object["video"] ?? ""
        if !videoName.isEmpty {
            guard videoName == URL(fileURLWithPath: videoName).lastPathComponent,
                  ["mov", "mp4"].contains(URL(fileURLWithPath: videoName).pathExtension.lowercased()) else { throw ConversionFailure.unreadable }
            try validate(folder.appendingPathComponent(videoName))
        }
        let staging = try PhotoSession.directory()
        do {
            let photo = staging.appendingPathComponent("LivePhoto").appendingPathExtension(source.pathExtension)
            try FileManager.default.copyItem(at: source, to: photo)
            let sourceType: BatchInput.Source
            let direction: String
            if videoName.isEmpty { sourceType = .folderFile(photo); direction = "motionToLive" }
            else {
                let original = folder.appendingPathComponent(videoName)
                let video = staging.appendingPathComponent("LivePhoto").appendingPathExtension(original.pathExtension)
                try FileManager.default.copyItem(at: original, to: video)
                sourceType = .livePair(photo, video); direction = "liveToMotion"
            }
            try FileManager.default.removeItem(at: manifest)
            try? FileManager.default.removeItem(at: folder)
            let input = BatchInput(name: photoName, source: sourceType)
            inputs[input.id.uuidString] = input; stagedInputs[input.id.uuidString] = staging
            return ["direction": direction, "item": ["id": input.id.uuidString, "name": input.name]]
        } catch { PhotoSession.remove(containing: staging); throw error }
    }
    private func startScan() {
        scanSnapshot = nil
        let engine = self.engine
        let scanner = PhotoScanner(source: PhotosLibrarySource()) { resource in
            let url = resource.temporaryFileURL()
            try resource.data.write(to: url)
            defer { try? FileManager.default.removeItem(at: url) }
            let token = InspectionCancellation()
            let inspected: InspectionResult = try await withTaskCancellationHandler(operation: {
                try await withCheckedThrowingContinuation { continuation in
                    engine.inspect(input: url, cancellation: token) { continuation.resume(with: $0) }
                }
            }, onCancel: { token.cancel() })
            switch inspected.classification {
            case .motion: return true
            case .staticImage: return false
            case .unknown: throw ConversionFailure.unavailable
            }
        }
        scanTask = Task {
            do {
                let found = try await scanner.scan { progress in
                    await MainActor.run {
                        self.scanSnapshot = progress
                        self.scanStream.sink?(["completed": progress.completed, "total": progress.total,
                                               "matches": progress.motionAssetIdentifiers.count,
                                               "issues": progress.issues.count])
                    }
                }
                guard found.access == .authorized || found.access == .limited else { throw LiveInputError.access }
                self.scanTask = nil; self.finishPicker(self.scanRows(found))
            } catch is CancellationError {
                let partial = self.scanSnapshot.map { PhotoScanResult(access: $0.access, motionAssetIdentifiers: $0.motionAssetIdentifiers, issues: $0.issues, matchedResources: $0.matchedResources) }
                self.scanTask = nil
                self.finishPicker(partial.map { self.scanRows($0) } ?? [])
            } catch {
                self.scanTask = nil; self.finishPicker(self.error("scan_failed", error.localizedDescription))
            }
        }
    }
    private func scanRows(_ found: PhotoScanResult) -> [[String: Any]] {
        var rows: [[String: Any]] = []
        for assetID in found.motionAssetIdentifiers {
            guard let identity = found.matchedResources[assetID] else { continue }
            let input = BatchInput(name: identity.filename, source: .scannedPhoto(assetID, identity))
            inputs[input.id.uuidString] = input
            rows.append(["id": input.id.uuidString, "name": input.name])
        }
        for (assetID, issue) in found.issues {
            let asset = PHAsset.fetchAssets(withLocalIdentifiers: [assetID], options: nil).firstObject
            let name = asset.flatMap { PHAssetResource.assetResources(for: $0).first?.originalFilename } ?? "Photo"
            let reason = issue == .originalUnavailable ? "Original is unavailable locally." : "Could not identify this original."
            rows.append(["id": UUID().uuidString, "name": name, "unavailableReason": reason])
        }
        pruneInputs(keeping: Set(rows.compactMap { $0["id"] as? String }), onlyIfNonempty: !rows.isEmpty)
        return rows
    }
    public func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) {
        if exporting { exporting = false; finishPicker(error("cancelled", "Export was cancelled.")) }
        else { finishPicker([]) }
    }
    public func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) {
        if exporting {
            exporting = false
            if urls.isEmpty { finishPicker(error("cancelled", "Export was cancelled.")) }
            else if urls.count != exportCount { finishPicker(error("partial_export", "Some files may not have been exported. Check Files before retrying.")) }
            else { finishPicker(nil) }
            exportCount = 0
            return
        }
        Task {
            var rows: [[String: Any]] = []
            for source in urls {
                let id = UUID()
                let name = source.lastPathComponent
                do {
                    let directory = try PhotoSession.directory()
                    let destination = directory.appendingPathComponent(name)
                    do {
                        try await Task.detached(priority: .utility) {
                            let scoped = source.startAccessingSecurityScopedResource()
                            defer { if scoped { source.stopAccessingSecurityScopedResource() } }
                            var coordinationError: NSError?
                            var copyError: Error?
                            NSFileCoordinator().coordinate(readingItemAt: source, options: [], error: &coordinationError) { url in
                                do {
                                    let values = try url.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey, .isUbiquitousItemKey, .ubiquitousItemDownloadingStatusKey])
                                    if values.isUbiquitousItem == true && values.ubiquitousItemDownloadingStatus != .current { throw LiveInputError.originalUnavailable }
                                    guard values.isRegularFile == true, values.isSymbolicLink != true else { throw ConversionFailure.unreadable }
                                    try FileManager.default.copyItem(at: url, to: destination)
                                } catch { copyError = error }
                            }
                            if let error = coordinationError ?? (copyError as NSError?) { throw error }
                        }.value
                    } catch { PhotoSession.remove(containing: directory); throw error }
                    let input = BatchInput(id: id, name: name, source: .folderFile(destination))
                    inputs[id.uuidString] = input; stagedInputs[id.uuidString] = directory
                    rows.append(["id": id.uuidString, "name": name])
                } catch {
                    rows.append(["id": id.uuidString, "name": name, "unavailableReason": error.localizedDescription])
                }
            }
            pruneInputs(keeping: Set(rows.compactMap { $0["id"] as? String }), onlyIfNonempty: !rows.isEmpty)
            finishPicker(rows)
        }
    }
    private func pruneInputs(keeping ids: Set<String>, onlyIfNonempty: Bool) {
        guard onlyIfNonempty else { return }
        for id in Array(inputs.keys) where !ids.contains(id) && outputOwners[id] == nil {
            inputs.removeValue(forKey: id)
            if let directory = stagedInputs.removeValue(forKey: id) { PhotoSession.remove(containing: directory) }
        }
    }
    private func output(for id: String) -> ConversionOutput? {
        guard let owner = outputOwners[id], let work = works[owner], let uuid = UUID(uuidString: id), let row = work.rows.first(where: { $0.id == uuid }), case .converted(let output) = row.state else { return nil }
        return output
    }
    private func finishRecoveryWaiters() {
        let waiters = recoveryWaiters; recoveryWaiters.removeAll()
        for (ids, result) in waiters { recoverBatch(ids, result: result) }
    }
    private func recoverBatch(_ ids: [String], result: @escaping FlutterResult) {
        guard let owner = recoveryOwners[ids.sorted().joined(separator: ",")], let work = works[owner] else {
            result(error("recovery_unavailable", "The conversion result could not be confirmed.")); return
        }
        result(["items": encodeRows(work.rows), "finished": true])
    }
    private func emit(_ work: BatchWork, rows snapshot: [BatchRowResult]? = nil, finished: Bool) {
        let rows = encodeRows(snapshot ?? work.rows)
        sink?(["items": rows, "finished": finished])
    }
    private func encodeRows(_ snapshot: [BatchRowResult]) -> [[String: Any]] {
        let rows: [[String: Any]] = snapshot.map { row in
            var value: [String: Any] = ["id": row.id.uuidString, "name": row.name]
            switch row.state {
            case .pending: value["status"] = "waiting"
            case .converting: value["status"] = "converting"
            case .cancelled: value["status"] = "skipped"
            case .failed(let reason): value["status"] = "failed"; value["reason"] = reason
            case .converted: value["status"] = "success"; value["outputId"] = row.id.uuidString; if let active { outputOwners[row.id.uuidString] = active }
            }
            return value
        }
        return rows
    }
    private func presenter() -> UIViewController? {
        let scenes = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.filter { $0.activationState == .foregroundActive }
        guard let root = scenes.flatMap({ $0.windows }).first(where: { $0.isKeyWindow })?.rootViewController else { return nil }
        var current = root; while let next = current.presentedViewController { current = next }; return current
    }
    private func finishPicker(_ value: Any?) { let callback = pickerResult; pickerResult = nil; callback?(value) }
    public func picker(_ picker: PHPickerViewController, didFinishPicking results: [PHPickerResult]) {
        picker.dismiss(animated: true)
        var picked: [[String: Any]] = []
        for item in results {
            guard let assetID = item.assetIdentifier else {
                picked.append(["id": UUID().uuidString, "name": "Photo", "unavailableReason": "The selected photo is not accessible."])
                continue
            }
            let asset = PHAsset.fetchAssets(withLocalIdentifiers: [assetID], options: nil).firstObject
            let name = asset.flatMap { PHAssetResource.assetResources(for: $0).first?.originalFilename } ?? "Photo"
            let input = BatchInput(name: name, source: pickerDirection == "liveToMotion" ? .livePhoto(assetID) : .originalPhoto(assetID))
            inputs[input.id.uuidString] = input
            var row: [String: Any] = ["id": input.id.uuidString, "name": name]
            if asset == nil { row["unavailableReason"] = "Photo is not accessible with the current Photos permission." }
            picked.append(row)
        }
        pruneInputs(keeping: Set(picked.compactMap { $0["id"] as? String }), onlyIfNonempty: !picked.isEmpty)
        finishPicker(picked)
    }
}
/// Scan progress is independent from batch events: {completed,total,matches,issues}.
private final class ScanStream: NSObject, FlutterStreamHandler {
    var sink: FlutterEventSink?
    func onListen(withArguments arguments: Any?, eventSink events: @escaping FlutterEventSink) -> FlutterError? { sink = events; return nil }
    func onCancel(withArguments arguments: Any?) -> FlutterError? { sink = nil; return nil }
}

#endif
