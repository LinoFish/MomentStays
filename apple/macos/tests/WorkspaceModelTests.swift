import Foundation
import Photos

private final class ControlledService: ConversionServing {
    struct Scan {
        let sources: [URL]; let direction: ConversionDirection; let token: StopToken
        let rowsReady: ([ScanRow]) -> Void; let progress: (Int, Int) -> Void; let completion: (Result<[ScanRow], Error>) -> Void
    }
    struct Run {
        let job: ConversionJob; let token: StopToken; let decision: ExistingOutputPresenter
        let progress: (BatchProgress) -> Void; let completion: (Result<BatchOutcome, Error>) -> Void
    }
    var scans: [Scan] = []
    var runs: [Run] = []
    func scan(sources: [URL], direction: ConversionDirection, token: StopToken, rowsReady: @escaping ([ScanRow]) -> Void,
              progress: @escaping (Int, Int) -> Void, completion: @escaping (Result<[ScanRow], Error>) -> Void) {
        scans.append(Scan(sources: sources, direction: direction, token: token, rowsReady: rowsReady, progress: progress, completion: completion))
    }
    func run(job: ConversionJob, token: StopToken, existingOutput: @escaping ExistingOutputPresenter,
             progress: @escaping (BatchProgress) -> Void, completion: @escaping (Result<BatchOutcome, Error>) -> Void) {
        runs.append(Run(job: job, token: token, decision: existingOutput, progress: progress, completion: completion))
    }
}

@main
struct WorkspaceModelTests {
    static var checks = 0
    static func check(_ condition: @autoclosure () -> Bool, _ message: String) {
        guard condition() else { fputs("FAIL: \(message)\n", stderr); exit(1) }
        checks += 1
    }
    static func pump(until done: () -> Bool, seconds: TimeInterval = 60) {
        let end = Date().addingTimeInterval(seconds)
        while !done() && Date() < end { RunLoop.current.run(until: Date().addingTimeInterval(0.01)) }
        check(done(), "asynchronous operation must finish")
    }
    static func deliveryChecks(root: URL, workspace: WorkspaceModel) throws {
        let store = DeliveryStore(root: root.appendingPathComponent("history"))
        check(store.tasks().isEmpty && !FileManager.default.fileExists(atPath: store.root.path), "reading history does not create files")
        var settings = ConversionSettings()
        settings.destination = .share; settings.normalize()
        check(settings.destination == .folder, "forward cannot share Android output")
        settings.direction = .toAndroid; settings.destination = .share; settings.normalize()
        check(settings.destination == .share && settings.destination.usesCache, "reverse direct sharing uses owned cache")
        let sourceFolder = root.appendingPathComponent("user-output")
        try FileManager.default.createDirectory(at: sourceFolder, withIntermediateDirectories: true)
        var files: [DeliveredFile] = []
        for index in 0..<2 {
            let folder = sourceFolder.appendingPathComponent(String(index))
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            let photo = folder.appendingPathComponent("same.MP.jpg")
            try Data([UInt8(index), 2, 3]).write(to: photo)
            files.append(DeliveredFile(source: photo.path, folder: folder.path, files: [photo.path], key: String(index), hashes: [photo.path: try NativeBatch.hashFile(photo)]))
        }
        var task = DeliveryTask(settings: settings, output: sourceFolder.path, results: files)
        let staged = try store.stageShare(&task)
        check(staged.map(\.lastPathComponent) == ["same.MP.jpg", "same (2).MP.jpg"], "share collisions preserve Motion Photo suffix")
        for index in 0..<2 {
            let copied = try Data(contentsOf: staged[index]), original = try Data(contentsOf: URL(fileURLWithPath: files[index].files[0]))
            check(copied == original, "sharing retains complete file bytes")
        }
        let cancelled = StopToken(); cancelled.cancel()
        do { _ = try store.stageShare(&task, token: cancelled); check(false, "stopped share preparation cannot continue") } catch { check(true, "share preparation cancellation preserved files") }
        let retry = try store.stageShare(&task)
        check(retry == staged && store.tasks().first?.shareFiles == staged.map(\.path), "sharing retry restores persistent files")
        do { try store.removeTask(task, active: [task.id]); check(false, "active cache cleanup must refuse") } catch { check(true, "active cache protected") }
        let key1 = try DeliveryStore.key(source: staged[0], origin: nil, settings: settings)
        settings.imageQuality = 0.7
        let key2 = try DeliveryStore.key(source: staged[0], origin: nil, settings: settings)
        check(key1 != key2, "conversion parameters participate in dedup key")
        let origin = LibraryOrigin(assetID: "asset", version: "v1", edited: false, filename: "same.jpg")
        let stable1 = try DeliveryStore.key(source: staged[0], origin: origin, settings: settings)
        let stable2 = try DeliveryStore.key(source: staged[1], origin: origin, settings: settings)
        check(stable1 == stable2, "library identity survives resource cache path changes")
        let originalPhoto = URL(fileURLWithPath: files[0].files[0])
        let movie = originalPhoto.deletingPathExtension().appendingPathExtension("mov")
        try Data([4, 5, 6]).write(to: movie)
        let alias = root.appendingPathComponent("input-alias")
        try FileManager.default.createSymbolicLink(at: alias, withDestinationURL: originalPhoto.deletingLastPathComponent())
        let aliasPhoto = alias.appendingPathComponent(originalPhoto.lastPathComponent)
        let synthetic = BatchOutcome(counts: ["converted": 1], report: nil, reportData: ["files": [["source": NativeBatch.normalized(originalPhoto).path, "output": originalPhoto.deletingLastPathComponent().path, "status": "converted"]]], cancelled: false)
        let collected = try DeliveryStore.collect(synthetic, settings: ConversionSettings(), origins: [aliasPhoto.path: origin])
        check(collected.first?.origin == origin && collected.first?.hashes.count == 2, "canonical paths preserve library identity and output hashes")
        try collected[0].verify()
        let candidate = originalPhoto.deletingLastPathComponent().appendingPathComponent("candidate.jpg")
        let candidateMovie = candidate.deletingPathExtension().appendingPathExtension("mov")
        try Data([7,8,9]).write(to:candidate); try Data([10,11,12]).write(to:candidateMovie)
        var candidateSettings = ConversionSettings(); candidateSettings.exportMode = .withCandidates
        var names = ["still":originalPhoto.lastPathComponent,"video":movie.lastPathComponent,"candidate_0":candidate.lastPathComponent]
        func candidatesOutcome() -> BatchOutcome {
            BatchOutcome(counts:["converted":1],report:nil,reportData:["files":[["source":originalPhoto.path,
                "output":originalPhoto.deletingLastPathComponent().path,"status":"converted","output_files":names]]],cancelled:false)
        }
        let mixed = try DeliveryStore.collect(candidatesOutcome(),settings:candidateSettings,origins:[:])
        check(mixed.count == 2 && mixed[0].files.count == 2 && mixed[1].files.count == 1,"mixed mode delivers a Live Photo and a separate static image")
        candidateSettings.candidateDelivery = .livePhotos; names["candidate_video_0"] = candidateMovie.lastPathComponent
        let allLive = try DeliveryStore.collect(candidatesOutcome(),settings:candidateSettings,origins:[:])
        check(allLive.count == 2 && allLive.allSatisfy { $0.files.count == 2 },"all-live mode keeps each candidate's complete pair")
        check(Set(allLive.map(\.key)).count == 2 && mixed[1].key != allLive[1].key,"candidate roles and delivery modes have independent dedup keys")
        for file in allLive { try file.verify() }
        var encoded = try JSONSerialization.jsonObject(with:JSONEncoder().encode(candidateSettings)) as! [String:Any]
        encoded.removeValue(forKey:"candidateDelivery"); encoded.removeValue(forKey:"coverSelections")
        let legacy = try JSONDecoder().decode(ConversionSettings.self,from:JSONSerialization.data(withJSONObject:encoded))
        check(legacy.candidateDelivery == nil && legacy.coverSelections == nil,"older task settings remain readable")
        let beforeCover = try DeliveryStore.key(source:originalPhoto,origin:nil,settings:candidateSettings)
        candidateSettings.coverSelections = [originalPhoto.path:0]
        let afterCover = try DeliveryStore.key(source:originalPhoto,origin:nil,settings:candidateSettings)
        candidateSettings.coverSelections?[candidate.path] = 1
        let unrelatedCover = try DeliveryStore.key(source:originalPhoto,origin:nil,settings:candidateSettings)
        check(beforeCover != afterCover && afterCover == unrelatedCover,"dedup tracks this photo's cover, independently of other queue selections")
        candidateSettings.exportMode = .auxiliaryOnly
        check(candidateSettings.options.coverSelections.isEmpty,"standalone auxiliary export ignores saved photo cover selections")
        let pending = LibraryImportRecord(key: "key", state: "pending")
        let newCreates = try LibraryImportPolicy.shouldCreate(nil, assetExists: false)
        check(newCreates, "new source may create")
        let retryCreates = try LibraryImportPolicy.shouldCreate(pending, assetExists: false)
        check(retryCreates, "definitely failed submission may retry")
        for state in ["submitting", "needs-review", "created", "completed"] {
            let record = LibraryImportRecord(key: "key", assetID: "created-asset", state: state)
            let creates = try LibraryImportPolicy.shouldCreate(record, assetExists: true)
            check(!creates, "existing asset only retries association")
            do { _ = try LibraryImportPolicy.shouldCreate(record, assetExists: false); check(false, "missing committed asset must not recreate") } catch { check(true, "missing/uncertain asset protected") }
        }
        do { _ = try LibraryImportPolicy.shouldCreate(LibraryImportRecord(key: "key", state: "submitting"), assetExists: false); check(false, "unknown submission must not recreate") } catch { check(true, "unknown transaction protected") }
        try store.save(LibraryImportRecord(key: "key", assetID: "generated", state: "created"))
        check(store.generatedAssetIDs.contains("generated"), "own imported assets excluded from monitoring")
        try store.inspected("source", version: "v1", scope: "album")
        check(store.inspectionVersion("source", scope: "album") == "v1" && store.inspectionVersion("source", scope: "other") == nil, "inspection tracking isolates album scopes")
        let cached = staged[0], originalCache = try Data(contentsOf: cached)
        try Data([99]).write(to: cached)
        do { _ = try store.stageShare(&task); check(false, "corrupt shared cache must refuse") } catch { check(true, "shared cache hash protected") }
        try originalCache.write(to: cached)
        try store.removeTask(task, active: [])
        check(!FileManager.default.fileExists(atPath: store.taskFolder(task.id).path) && FileManager.default.fileExists(atPath: files[0].files[0]), "cache cleanup preserves user output")
        var bad = task; bad.id = "../escape"
        do { try store.save(bad); check(false, "invalid task id must refuse") } catch { check(true, "task path traversal refused") }
        try Data("broken".utf8).write(to: store.root.appendingPathComponent("imports.json"))
        do { try store.save(pending); check(false, "corrupt ledger cannot reset dedup") } catch { check(true, "corrupt ledger stops imports") }
        workspace.sharing = true
        check(!workspace.canStart && workspace.beginLibraryImport() == nil, "active sharing prevents replacing inputs")
        workspace.sharing = false
        let token = workspace.beginDelivery("testing")
        workspace.stop()
        check(token.isCancelled, "delivery respects stop token")
        workspace.finishDelivery("preserved")
        check(workspace.phase == .stopped && workspace.deliverySummary == "preserved", "stopped delivery retains result status")
    }

    static func batchPlanningChecks(source: URL, root: URL) throws {
        let folder = root.appendingPathComponent("mixed"), destination = root.appendingPathComponent("mixed-output")
        try FileManager.default.createDirectory(at:folder,withIntermediateDirectories:true)
        let motion = folder.appendingPathComponent("motion.jpg"), still = folder.appendingPathComponent("static.jpg")
        let data = try Data(contentsOf:source), photo = try PhotoFormat.parse(Array(data))
        let plainStill = try PhotoFormat.liveStill(photo.still,uuid:"",updateIdentifier:false)
        try data.write(to:motion); try Data(plainStill).write(to:still)
        var rows = try NativeBatch.scan(sources:[folder],token:StopToken()) { _,_ in }
        check(rows.count == 2 && rows.filter(\.confirmedStatic).count == 1,"scan explicitly distinguishes a complete static photo")
        let staticTarget = destination.appendingPathComponent("static.jpg.livephoto")
        try FileManager.default.createDirectory(at:staticTarget,withIntermediateDirectories:true)
        try Data([123]).write(to:staticTarget.appendingPathComponent("sentinel"))
        var updates: [BatchProgress] = [], decisions: [String] = []
        let outcome = try NativeBatch.run(sources:[folder],output:destination,options:ConversionOptions(),token:StopToken(),scanRows:rows,
            existingOutput:{ url,_ in decisions.append(url.lastPathComponent); return .skip }) { updates.append($0) }
        check(outcome.counts["converted"] == 1 && outcome.counts["skipped"] == 1,"mixed batch converts only motion photos")
        check(decisions.isEmpty && updates.allSatisfy { $0.total == 1 },"static photo bypasses overwrite decisions and conversion progress")
        check(try! Data(contentsOf:staticTarget.appendingPathComponent("sentinel")) == Data([123]),"static output is untouched")
        let files = outcome.reportData["files"] as! [[String:Any]]
        check(files.map { URL(fileURLWithPath:$0["source"] as! String).lastPathComponent } == ["motion.jpg","static.jpg"],"parallel report retains source order")
        check(files[1]["conversion_seconds"] == nil && files[1]["output"] == nil,"static photo never enters the converter or allocates output")
        let unknown = folder.appendingPathComponent("unknown.jpg")
        try Data(plainStill + [1,2,3]).write(to:unknown)
        let unknownRow = NativeBatch.scanFile(unknown,relative:"unknown.jpg")
        check(unknownRow.needsInspection && !unknownRow.confirmedStatic,"unknown trailing payload is not called a static photo")
        do {
            _ = try NativeBatch.run(sources:[folder],output:destination,options:ConversionOptions(),token:StopToken(),scanRows:rows) { _ in }
            check(false,"new source must invalidate the batch snapshot")
        } catch { check(error.localizedDescription.contains("重新扫描"),"new source requests a rescan") }
        try FileManager.default.removeItem(at:unknown)
        try data.write(to:still,options:.atomic)
        let changed = try NativeBatch.run(sources:[folder],output:root.appendingPathComponent("changed-output"),options:ConversionOptions(),token:StopToken(),scanRows:rows) { _ in }
        check(changed.counts["converted"] == 2 && changed.counts["skipped"] == 0,"static replaced by motion is identified again")
        try Data(plainStill).write(to:still,options:.atomic)
        try FileManager.default.removeItem(at:motion)
        rows = try NativeBatch.scan(sources:[folder],token:StopToken()) { _,_ in }
        var options = ConversionOptions(); options.exportDataFiles = false
        let staticOutput = root.appendingPathComponent("all-static-output")
        let allStatic = try NativeBatch.run(sources:[folder],output:staticOutput,options:options,token:StopToken(),scanRows:rows) { _ in }
        check(allStatic.counts["skipped"] == 1 && !FileManager.default.fileExists(atPath:staticOutput.path),"all-static batch creates no output staging or fake conversion")
        let lock = NSLock(); var active = 0, peak = 0, finished = 0
        let token = StopToken(), entered = DispatchGroup()
        entered.enter(); entered.enter()
        BatchScheduling.perform(files:Array(repeating:still,count:8),limit:2,token:token,work:{ index in
            lock.lock(); active += 1; peak = max(peak,active); lock.unlock()
            entered.leave()
            _ = entered.wait(timeout:.now() + 2)
            lock.lock(); active -= 1; lock.unlock()
            return index
        },receive:{ _,_ in finished += 1; if finished == 1 { token.cancel() } })
        check(peak == 2 && finished <= 2,"bounded workers overlap and stop dispatching after cancellation")
    }

    static func appendAndClearChecks(source: URL, root: URL) throws {
        let service = ControlledService(), model = WorkspaceModel(service:ControlledService())
        let workspace = WorkspaceModel(service:service)
        let destination = NativeBatch.normalized(root.appendingPathComponent("chosen-output"))
        try FileManager.default.createDirectory(at:destination,withIntermediateDirectories:true)
        workspace.setOutput(destination)
        workspace.appendSources([source,source])
        check(workspace.sources.count == 1 && workspace.output?.path == destination.path,"adding first source preserves a preselected output")
        let row = ScanRow(source:NativeBatch.normalized(source),relative:"test.jpg",isMotion:true,hdr:"SDR",cover:"0.4 秒",detail:"ready")
        service.scans[0].completion(.success([row]))
        workspace.selection = row.source.path; workspace.selectCover(1,for:source)
        workspace.updateSettings { $0.exportDataFiles = false }
        let second = root.appendingPathComponent("input/second.jpg")
        try Data([1]).write(to:second)
        workspace.appendSources([source,second])
        check(workspace.sources.count == 2 && workspace.output?.path == destination.path,"append merges sources and retains output")
        check(workspace.settings.coverIndex(for:source) == 1 && !workspace.settings.exportDataFiles,"append retains cover and export settings")
        let secondRow = ScanRow(source:NativeBatch.normalized(second),relative:"second.jpg",isMotion:true,hdr:"SDR",cover:"0.4 秒",detail:"ready")
        workspace.clearSources()
        check(workspace.sources.count == 2,"clear is blocked while scanning")
        service.scans[1].completion(.success([row,secondRow]))
        check(workspace.items.count == 2 && workspace.selection == row.source.path,"append retains existing photo selection")
        let scans = service.scans.count
        workspace.appendSources([second,source])
        check(service.scans.count == scans && workspace.items.count == 2 && workspace.notice != nil,"duplicate inputs give feedback without replacing or rescanning the batch")
        workspace.clearSources()
        check(workspace.sources.isEmpty && workspace.items.isEmpty && workspace.selection == nil && !workspace.canStart && workspace.notice == nil,"clear empties current task and feedback")
        check(workspace.output?.path == destination.path && !workspace.settings.exportDataFiles && workspace.settings.coverSelections == nil,"clear keeps output and export settings, resets photo covers")
        check(FileManager.default.fileExists(atPath:source.path) && FileManager.default.fileExists(atPath:destination.path),"clear preserves source and output files")
        service.scans[1].completion(.success([row,secondRow]))
        check(workspace.items.isEmpty,"late callbacks cannot refill a cleared queue")
        workspace.appendSources([second]); service.scans.last!.completion(.success([secondRow]))
        check(workspace.items.count == 1 && workspace.output?.path == destination.path,"adding after clear reuses the selected output")
        workspace.sharing = true; workspace.clearSources()
        check(workspace.items.count == 1,"clear is blocked while sharing")
        workspace.sharing = false

        let library = WorkspaceModel(service:service)
        let original = LibraryOrigin(assetID:"asset-1",version:"v1",edited:false,filename:"test.jpg")
        library.appendSources([source],origins:[NativeBatch.normalized(source).path:original])
        service.scans.last!.completion(.success([row]))
        let before = service.scans.count
        library.appendSources([second],origins:[NativeBatch.normalized(second).path:original])
        check(service.scans.count == before && library.sources.count == 1,"same library asset/version deduplicates across temporary copies")
        var edited = original; edited.edited = true
        library.appendSources([second],origins:[NativeBatch.normalized(second).path:edited])
        check(library.sources.count == 2,"different library versions remain independently selectable")
        check(!model.canStart,"unrelated workspace stays empty")
    }

    static func main() throws {
        // Evaluate fetch predicates without requesting permission or accessing a real library.
        for direction: ConversionDirection in [.toApple,.toAndroid] {
            let scope = LibraryScope(direction: direction, version: .original, networkAllowed: false)
            let options = PhotoLibraryService.fetchOptions(scope)
            let image = PHAssetMediaType.image.rawValue, live = PHAssetMediaSubtype.photoLive.rawValue
            check(options.fetchLimit == 0, "library scan has no result limit")
            check(options.includeAssetSourceTypes == [.typeUserLibrary, .typeCloudShared, .typeiTunesSynced],
                "both scan directions include user, shared-album and synced assets")
            for subtype in [UInt(0), live, live | PHAssetMediaSubtype.photoHDR.rawValue] {
                check(options.predicate!.evaluate(with: ["mediaType": image, "mediaSubtypes": subtype]) == (direction == .toApple || subtype & live != 0),
                    "Android original inspection includes Live Photo assets; reverse requires Live Photo")
            }
            check(!options.predicate!.evaluate(with: ["mediaType": PHAssetMediaType.video.rawValue, "mediaSubtypes": live]),
                "standalone videos are excluded from photo scanning")
        }
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("motionlive-model-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: root.appendingPathComponent("input"), withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let source = root.appendingPathComponent("input/test.jpg")
        try Data([0]).write(to: source)
        try appendAndClearChecks(source:source,root:root)
        let row = ScanRow(source: source, relative: "test.jpg", isMotion: true, hdr: "Ultra HDR", cover: "0.4 秒", detail: "ready")
        let staticRow = ScanRow(source:source,relative:"test.jpg",isMotion:false,hdr:"SDR",cover:"—",detail:"static",confirmedStatic:true)
        check(PhotoQueueItem(staticRow).category(settings:ConversionSettings()) == .static,"confirmed static has its own category")
        let unknownRow = ScanRow(source:source,relative:"test.jpg",isMotion:false,hdr:"—",cover:"—",detail:PhotoFormat.notMotion)
        check(PhotoQueueItem(unknownRow).category(settings:ConversionSettings()) == .inspect,"notMotion text alone cannot prove static")
        let missingRow = ScanRow(source:source,relative:"test.jpg",isMotion:true,hdr:"SDR",cover:"未记录",detail:"missing")
        var relaxed = ConversionSettings(); relaxed.missingTime = .midpoint
        check(PhotoQueueItem(missingRow).category(settings:ConversionSettings()) == .inspect && PhotoQueueItem(missingRow).category(settings:relaxed) == .ready,"cover classification follows the selected policy")
        var transfer = ConversionSettings()
        transfer.direction = .androidToAndroid; transfer.vendor = .samsung; transfer.container = .heic
        transfer.exportMode = .withCandidates; transfer.missingTime = .midpoint
        transfer.destination = .photos; transfer.normalize()
        check(transfer.destination == .folder && transfer.destinations == [.folder,.share,.folderAndShare], "transfer cannot import into Apple library")
        let transferOptions = transfer.options.json as NSDictionary
        transfer.vendor = .vivo; transfer.exportMode = .auxiliaryOnly; transfer.coverSelections = [source.path:2]
        check(transfer.options.json as NSDictionary == transferOptions, "hidden Apple/vendor/transcode options cannot alter transfer")
        check(transfer.options.direction == "android-to-android" && transfer.options.android.vendor == "google", "transfer uses independent fixed Google path")
        check(transfer.summary.contains("暂仅支持 Google → 荣耀"), "transfer states its precise scope")
        let encodedTransfer = try JSONEncoder().encode(transfer)
        let restoredTransfer = try JSONDecoder().decode(ConversionSettings.self,from:encodedTransfer)
        check(restoredTransfer.direction == .androidToAndroid, "third direction settings restore")
        let oldSettings = ConversionSettings()
        let restoredOld = try JSONDecoder().decode(ConversionSettings.self,from:JSONEncoder().encode(oldSettings))
        check(restoredOld.direction == .toApple, "old settings restore unchanged")
        let transferService = ControlledService(), transferModel = WorkspaceModel(service:ControlledService())
        transferModel.updateSettings { $0.direction = .androidToAndroid }
        check(transferModel.beginLibraryImport() == nil, "transfer has no Apple library input")
        let rapidService = ControlledService(), rapid = WorkspaceModel(service:ControlledService())
        rapid.updateSettings { $0.direction = .toApple }
        check(rapid.settings.destination == .ask && !rapid.outputIsChosen, "new forward task requires an explicit choice")
        let switching = WorkspaceModel(service:rapidService)
        switching.chooseSources([source]); switching.updateSettings { $0.direction = .toAndroid }
        switching.updateSettings { $0.direction = .androidToAndroid }
        check(rapidService.scans.count == 3 && rapidService.scans[0].token.isCancelled && rapidService.scans[1].token.isCancelled, "rapid switching cancels obsolete inspection requests")
        rapidService.scans[0].completion(.success([row])); rapidService.scans[1].completion(.success([row]))
        check(switching.phase == .projecting && switching.items.isEmpty, "stale direction callbacks cannot refill the current queue")
        rapidService.scans[2].completion(.success([unknownRow]))
        check(switching.settings.direction == .androidToAndroid && switching.items.first?.category(settings:switching.settings) == .inspect, "latest direction projection wins")
        let pick = WorkspaceModel(service:ControlledService())
        let initialSettings = pick.settings
        pick.confirmOutput(destination:.folder,folder:nil,albumID:nil,albumTitle:nil)
        check(pick.settings == initialSettings && !pick.outputIsChosen, "cancelled or incomplete output choice does not mutate settings")
        pick.confirmOutput(destination:.folderAndPhotos,folder:root,albumID:nil,albumTitle:nil)
        check(pick.settings == initialSettings, "combined output requires both choices before commit")
        pick.confirmOutput(destination:.photos,folder:nil,albumID:"album",albumTitle:"chosen")
        check(pick.settings.destination == .photos && pick.settings.outputConfirmed == true, "album selection commits an explicit output")
        pick.updateSettings { $0.direction = .toAndroid }; pick.updateSettings { $0.direction = .toApple }
        check(pick.settings.destination == .photos && pick.settings.albumID == "album", "direction switches restore each direction's chosen output")

        let switched = WorkspaceModel(service:transferService)
        switched.chooseSources([source]); transferService.scans[0].completion(.success([row]))
        switched.updateSettings { $0.direction = .androidToAndroid }
        check(transferService.scans.count == 2 && transferService.scans[1].direction == .androidToAndroid, "direction switch rescans with transfer gate")
        transferService.scans[1].completion(.success([unknownRow]))
        check(switched.items.first?.category(settings:switched.settings) == .inspect, "unsupported transfer input remains inspectable")
        let service = ControlledService(), model = WorkspaceModel(service: ControlledService())
        check(!model.canStart, "empty workspace cannot convert")
        let workspace = WorkspaceModel(service: service)
        var expectedGUIOptions = ConversionOptions()
        expectedGUIOptions.exportDataFiles = false
        check(workspace.settings.options.json as NSDictionary == expectedGUIOptions.json as NSDictionary, "GUI defaults omit optional data files and preserve conversion settings")
        check(workspace.settings.options.android.json as NSDictionary == AndroidOptions().json as NSDictionary, "reverse defaults remain compatible")
        workspace.updateSettings { $0.vendor = .samsung; $0.container = .heic }
        check(workspace.settings.container == .heic, "supported vendors retain HEIC")
        workspace.updateSettings { $0.vendor = .honor }
        check(workspace.settings.container == .jpeg, "unsupported vendors normalize to JPEG")
        workspace.chooseSources([source, source])
        check(service.scans.count == 1 && service.scans[0].sources.count == 1, "duplicate sources deduplicate before scan")
        workspace.updateSettings { $0.track = .primary }
        check(workspace.settings.direction == .toApple, "non-direction settings stay locked during scan")
        service.scans[0].rowsReady([row])
        check(workspace.items.count == 1 && !workspace.canStart,"incremental rows do not allow an incomplete scan to convert")
        service.scans[0].completion(.success([row]))
        check(!workspace.canStart && workspace.settings.destination == .ask, "scan cannot silently choose an output location")
        let confirmedFolder = root.appendingPathComponent("confirmed-output")
        try FileManager.default.createDirectory(at:confirmedFolder,withIntermediateDirectories:true)
        workspace.setOutput(confirmedFolder)
        check(workspace.canStart && workspace.phase == .ready, "explicit output selection enables conversion")
        let originalOutput = workspace.output
        workspace.setOutput(source.deletingLastPathComponent())
        check(workspace.output == originalOutput && workspace.errorMessage != nil, "unsafe output rejected without changing destination")
        workspace.selection = source.path
        workspace.updateSettings { $0.exportDataFiles = false; $0.track = .secondary }
        workspace.start()
        check(service.runs.count == 1 && !service.runs[0].job.options.exportDataFiles, "job receives settings snapshot")
        workspace.updateSettings { $0.track = .primary }
        check(workspace.settings.track == .secondary && service.runs[0].job.options.videoTrack == "secondary", "running parameters stay immutable")
        let update = BatchProgress(index: 1, total: 1, relative: "test.jpg", status: "failed", detail: "sample failure")
        service.runs[0].progress(update)
        check(workspace.selectedItem?.state == .failed && workspace.selectedItem?.detail == "sample failure", "progress updates selected row with stable ID")
        var pendingDecision: ((ExistingOutputDecision) -> Void)?
        var reply: ExistingOutputDecision?
        var cancelCalls = 0
        workspace.presentExistingOutput = { _, _, completion in pendingDecision = completion }
        workspace.cancelPlatformActivity = { cancelCalls += 1 }
        service.runs[0].decision(source, originalOutput!) { reply = $0 }
        check(workspace.phase == .awaitingDecision, "overwrite prompt is an explicit task phase")
        workspace.stop()
        check(workspace.phase == .stopping && service.runs[0].token.isCancelled && cancelCalls == 1, "stop cancels worker and platform prompt")
        pendingDecision?(.overwrite)
        check(reply == .cancel && workspace.phase == .stopping, "late overwrite response cannot resume cancelled job")
        service.runs[0].progress(update)
        check(workspace.status.contains("正在停止"), "late progress cannot hide stopping state")
        let outcome = BatchOutcome(counts: ["failed": 1], report: nil, reportData: ["files": [["source": source.path, "status": "failed", "error": "sample failure"]]], cancelled: true)
        service.runs[0].completion(.success(outcome))
        check(workspace.phase == .stopped && workspace.report?.url == nil && workspace.report?.entries.first?.detail == "sample failure", "in-memory report survives cancelled job without JSON")
        workspace.updateSettings { $0.direction = .toAndroid }
        check(service.scans.count == 2 && service.scans[1].direction == .toAndroid, "direction change rescans with typed direction")
        service.runs[0].progress(update)
        service.scans[0].completion(.success([]))
        check(workspace.phase == .projecting && workspace.items.isEmpty, "old job callbacks cannot overwrite newer projection")
        service.scans[1].completion(.success([row]))
        check(workspace.selection == source.path, "rescan preserves valid selection")
        let library = workspace.beginLibraryImport()!
        workspace.stop()
        workspace.finishLibraryImport(id: library, urls: [], error: Failure("cancelled"), output: root)
        check(workspace.phase == .stopped && workspace.sources == [source] && workspace.errorMessage == nil, "library cancellation preserves source and is not an error")
        workspace.finishLibraryImport(id: UUID(), urls: [], error: Failure("stale"), output: root)
        check(workspace.errorMessage == nil, "stale library callback ignored")

        try deliveryChecks(root: root, workspace: workspace)

        let latch = OutputDecisionLatch(), token = StopToken(), done = DispatchSemaphore(value: 0)
        DispatchQueue.global().async {
            check(latch.wait(token: token) == .cancel, "cancelled pending decision unblocks without UI callback")
            done.signal()
        }
        token.cancel()
        check(done.wait(timeout: .now() + 2) == .success, "decision cancellation does not deadlock")
        latch.resolve(.overwrite)

        if CommandLine.arguments.count > 1 {
            try batchPlanningChecks(source:URL(fileURLWithPath:CommandLine.arguments[1]),root:root)
            try integration(source: URL(fileURLWithPath: CommandLine.arguments[1]), root: root)
        }
        print("PASS \(checks) workspace state, cancellation and integration checks")
    }

    static func integration(source: URL, root: URL) throws {
        let originalHash = try NativeBatch.hashFile(source)
        let model = WorkspaceModel()
        model.chooseSources([source])
        pump(until: { !model.isBusy })
        check(!model.canStart && model.items.first?.isMotion == true, "real service inspects motion photo without silently selecting output")
        let forward = root.appendingPathComponent("forward")
        try FileManager.default.createDirectory(at: forward, withIntermediateDirectories: true)
        model.setOutput(forward)
        model.start()
        pump(until: { !model.isBusy })
        check(model.report?.counts["converted"] == 1, "SwiftUI model drives real forward conversion")
        check(model.items.first?.state == .converted, "real conversion reaches queue row")
        let pair = forward.appendingPathComponent(source.lastPathComponent + ".livephoto")
        let still = try FileManager.default.contentsOfDirectory(at: pair, includingPropertiesForKeys: nil).first { ["jpg", "jpeg", "heic"].contains($0.pathExtension.lowercased()) }!
        model.presentExistingOutput = { _, _, reply in reply(.skip) }
        model.start()
        pump(until: { !model.isBusy })
        check(model.report?.counts["skipped"] == 1, "real overwrite callback supports skip")
        model.presentExistingOutput = { _, _, reply in reply(.overwrite) }
        model.start()
        pump(until: { !model.isBusy })
        check(model.report?.counts["converted"] == 1, "real overwrite callback preserves engine publish path")
        let reverse = WorkspaceModel()
        reverse.updateSettings { $0.direction = .toAndroid; $0.exportDataFiles = false }
        reverse.chooseSources([still])
        pump(until: { !reverse.isBusy })
        check(reverse.canStart, "reverse scan recognizes output pair")
        let reverseOutput = root.appendingPathComponent("reverse")
        try FileManager.default.createDirectory(at: reverseOutput, withIntermediateDirectories: true)
        reverse.setOutput(reverseOutput); reverse.start()
        pump(until: { !reverse.isBusy })
        check(reverse.report?.counts["converted"] == 1 && reverse.report?.url == nil, "real reverse conversion retains in-memory report without JSON")
        let transferSource = reverseOutput.appendingPathComponent(still.lastPathComponent + ".motionphoto").appendingPathComponent(still.deletingPathExtension().lastPathComponent + ".MP.jpg")
        let transfer = WorkspaceModel()
        transfer.updateSettings { $0.direction = .androidToAndroid; $0.vendor = .samsung; $0.container = .heic; $0.destination = .folderAndShare }
        transfer.chooseSources([transferSource]); pump(until:{ !transfer.isBusy })
        check(transfer.items.first?.isMotion == true && transfer.canStart, "real transfer scan accepts standard Google JPEG")
        let transferOutput = root.appendingPathComponent("transfer")
        try FileManager.default.createDirectory(at:transferOutput,withIntermediateDirectories:true)
        transfer.setOutput(transferOutput); transfer.start(); pump(until:{ !transfer.isBusy })
        check(transfer.report?.counts["converted"] == 1, "model executes third direction without Apple pair")
        let transferFolder = transferOutput.appendingPathComponent(transferSource.lastPathComponent + ".motionphoto")
        let outputPhoto = transferFolder.appendingPathComponent(transferSource.deletingPathExtension().lastPathComponent + ".MP.jpg")
        let outcome = BatchOutcome(counts:["converted":1],report:nil,reportData:["files":[["status":"converted","source":transferSource.path,"output":transferFolder.path],["status":"failed","source":source.path]]],cancelled:false)
        let delivered = try DeliveryStore.collect(outcome,settings:transfer.settings,origins:[:])
        check(delivered.count == 1 && delivered[0].files == [outputPhoto.path], "transfer shares only published JPEG despite hidden HEIC selection")
        var task = DeliveryTask(settings:transfer.settings,output:transferOutput.path,results:delivered)
        let share = try DeliveryStore(root:root.appendingPathComponent("transfer-share")).stageShare(&task)
        check(share.count == 1 && share[0].pathExtension == "jpg", "transfer reuses complete-file sharing")
        let cache = SourceInspectionCache()
        func project(_ direction:ConversionDirection, token:StopToken = StopToken()) throws -> [ScanRow] {
            try cache.rows(sources:[transferSource],direction:direction,token:token,rowsReady:{ _ in },progress:{ _,_ in })
        }
        let neutralRows = try project(.toApple)
        check(neutralRows.first?.isMotion == true, "cache builds neutral source inspection")
        let parsedCount = cache.parseCount
        let transferRows = try project(.androidToAndroid)
        check(transferRows.first?.isMotion == true, "missing transfer validation uses the neutral parse")
        _ = try project(.toAndroid)
        let pairCount = cache.pairValidationCount, transferCount = cache.transferValidationCount
        _ = try project(.toApple); _ = try project(.toAndroid); _ = try project(.androidToAndroid)
        check(cache.parseCount == parsedCount && cache.pairValidationCount == pairCount && cache.transferValidationCount == transferCount, "switching reuses neutral parsing and all valid direction projections")
        let stopped = StopToken(); stopped.cancel(); let cancelledRows = try project(.toApple,token:stopped)
        check(cancelledRows.isEmpty && cache.parseCount == parsedCount, "cancelled projection does not parse or return partial work")
        let originalTransfer = try Data(contentsOf:transferSource)
        try (originalTransfer + Data([0])).write(to:transferSource)
        _ = try project(.toApple)
        check(cache.parseCount == parsedCount + 1, "modified source invalidates neutral inspection")
        try originalTransfer.write(to:transferSource)
        _ = try project(.toApple)
        check(cache.parseCount == parsedCount + 2, "restored bytes with a new version are inspected again")
        let folderCache = SourceInspectionCache(), folder = root.appendingPathComponent("cache-folder")
        try FileManager.default.createDirectory(at:folder,withIntermediateDirectories:true)
        let first = folder.appendingPathComponent("one.jpg"), second = folder.appendingPathComponent("two.jpg")
        try originalTransfer.write(to:first)
        _ = try folderCache.rows(sources:[folder],direction:.toApple,token:StopToken(),rowsReady:{_ in},progress:{_,_ in})
        try originalTransfer.write(to:second); try FileManager.default.removeItem(at:first)
        let changedRows = try folderCache.rows(sources:[folder],direction:.toApple,token:StopToken(),rowsReady:{_ in},progress:{_,_ in})
        check(changedRows.map(\.source) == [second] && folderCache.parseCount == 2, "folder changes inspect only additions and remove deleted files")
        let enumerations = folderCache.enumerationCount
        _ = try folderCache.rows(sources:[folder],direction:.toApple,token:StopToken(),rowsReady:{_ in},progress:{_,_ in})
        check(folderCache.enumerationCount == enumerations && folderCache.parseCount == 2, "unchanged folders reuse their inventory without rescanning the directory tree")
        _ = try cache.rows(sources:[still],direction:.toAndroid,token:StopToken(),rowsReady:{_ in},progress:{_,_ in})
        let pairBefore = cache.pairValidationCount
        let movie = still.deletingPathExtension().appendingPathExtension("mov")
        let movieBytes = try Data(contentsOf:movie); try movieBytes.write(to:movie)
        _ = try cache.rows(sources:[still],direction:.toAndroid,token:StopToken(),rowsReady:{_ in},progress:{_,_ in})
        check(cache.pairValidationCount == pairBefore + 1, "paired video modifications invalidate only Apple pair validation")
        let finalHash = try NativeBatch.hashFile(source)
        check(finalHash == originalHash, "UI integration leaves original unchanged")
    }
}
