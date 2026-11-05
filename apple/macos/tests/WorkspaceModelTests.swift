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
        let plainStill = try fixtureStill(photo.still,uuid:"",updateIdentifier:false)
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

    static func navigationChecks(source: URL, row: ScanRow, root: URL) throws {
        try FileManager.default.createDirectory(at:root.appendingPathComponent("navigation-output"),withIntermediateDirectories:true)
        let order = PhotoQueueScope.allCases
        check(order.firstIndex(of:.completed) == order.firstIndex(of:.ready)! + 2, "manual tab sits between ready and completed")
        for (start, switches, cancelled, expected) in [
            (PhotoQueueScope.ready, [PhotoQueueScope](), false, PhotoQueueScope.completed),
            (.all, [], false, .all), (.completed, [], false, .completed),
            (.ready, [.all,.ready], false, .ready), (.ready,[.attention],false,.attention),
            (.ready,[.ready],false,.completed), (.ready,[],true,.ready)
        ] {
            let service = ControlledService()
            // Each case owns a service so progress/completion stay deterministic.
            let workspace = WorkspaceModel(service:service)
            workspace.chooseSources([source]); service.scans[0].completion(.success([row]))
            workspace.setOutput(root.appendingPathComponent("navigation-output"))
            workspace.selectQueueScope(start); workspace.start()
            check(service.runs.count == 1 && workspace.phase == .converting, "navigation starts a real model job")
            let snapshot = service.runs[0].job.sources
            for tab in switches { workspace.selectQueueScope(tab) }
            check(workspace.phase == .converting && service.runs[0].job.sources == snapshot && service.scans.count == 1,
                "browsing tabs keeps the active job and does not scan")
            let outcome = BatchOutcome(counts:["converted":1],report:nil,reportData:["files":[]],cancelled:cancelled)
            service.runs[0].completion(.success(outcome))
            check(workspace.queueScope == expected, "completion follows only an untouched ready tab")
        }
    }

    static func manualChecks(root: URL) throws {
        let a = root.appendingPathComponent("input/manual-a.jpg"), b = root.appendingPathComponent("input/manual-b.jpg")
        try Data([1]).write(to:a); try Data([2]).write(to:b)
        let out = root.appendingPathComponent("manual-output")
        try FileManager.default.createDirectory(at:out,withIntermediateDirectories:true)
        func row(_ url:URL, missing:Bool) -> ScanRow {
            ScanRow(source:url,relative:url.lastPathComponent,isMotion:true,hdr:"SDR",cover:missing ? "未记录" : "0.4 秒",detail:"fixture",version:SourceVersion.read(url))
        }
        let ra = row(a,missing:false), rb = row(b,missing:true)
        func setup() -> (WorkspaceModel,ControlledService) {
            let service = ControlledService()
            let workspace = WorkspaceModel(service:service)
            workspace.chooseSources([a,b]); service.scans[0].completion(.success([row(a,missing:false),row(b,missing:true)])); workspace.setOutput(out)
            return (workspace,service)
        }
        let (m,s) = setup()
        var allocations = 0
        m.prepareConversion = { _,output in allocations += 1; return output }
        m.toggleSelection(m.items[0]); m.selectQueueScope(.manual); m.toggleSelection(m.items[1])
        check(m.selectedIDs == Set([a.path,b.path]) && m.selection == b.path,"ordinary clicks accumulate across tabs independently of focus")
        m.toggleSelection(m.items[1]); m.selectQueueScope(.ready)
        check(m.selectedIDs == [a.path],"single deselection retains hidden selections")
        m.start(); check(m.queueScope == .manual && m.manualPromptPresented && s.runs.isEmpty,"preflight navigates before prompting")
        m.resolveManualPrompt(process:nil)
        check(s.runs.isEmpty && allocations == 0 && !m.manualPreparation && m.selectedIDs == [a.path],"cancel never starts conversion or clears selection")
        m.selectQueueScope(.ready); m.start(); m.resolveManualPrompt(process:false); RunLoop.current.run(until:Date(timeIntervalSinceNow:0.02))
        check(s.runs.count == 1 && allocations == 1 && s.runs[0].job.sources == [a] && m.manualItems.count == 1,"skip includes only selected ready sources and retains manual input")
        s.runs[0].completion(.success(BatchOutcome(counts:[:],report:nil,reportData:[:],cancelled:false)))
        check(m.queueScope == .completed,"system preflight navigation preserves completion following")
        let (only,os) = setup()
        only.toggleSelection(only.items[1]); only.start(); only.resolveManualPrompt(process:false); RunLoop.current.run(until:Date(timeIntervalSinceNow:0.02))
        check(os.runs.isEmpty && only.selectedIDs == [b.path],"only manual selection cannot fall back to all ready")
        only.selectQueueScope(.ready); only.start(); only.resolveManualPrompt(process:true)
        check(!only.canStart,"manual preparation blocks duplicate primary-action task allocation")
        only.start(); check(os.runs.isEmpty,"duplicate start cannot bypass manual confirmation")
        only.confirmManualMidpoint(only.items[1]); only.continueManualConversion()
        check(os.runs.count == 1 && os.runs[0].job.sources == [b] && os.runs[0].job.options.missingTime == "midpoint","confirmed manual strategy reaches the real batch with exact selected sources")
        only.selectQueueScope(.all)
        os.runs[0].completion(.success(BatchOutcome(counts:["failed":1],report:nil,reportData:[:],cancelled:false)))
        check(only.queueScope == .all,"user navigation during manual conversion disables following")
        let (both,bs) = setup()
        both.toggleSelection(both.items[0]); both.toggleSelection(both.items[1]); both.start(); both.resolveManualPrompt(process:true)
        both.confirmManualMidpoint(both.items[1]); both.continueManualConversion()
        check(Set(bs.runs[0].job.sources) == Set([a,b]),"manual processing preserves selected ready sources")
        bs.runs[0].completion(.success(BatchOutcome(counts:[:],report:nil,reportData:[:],cancelled:true)))
        both.rescan(); bs.scans.last!.completion(.success([ra,rb]))
        check(both.selectedIDs.count == 2 && both.manualItems.isEmpty,"same-version rescan preserves selections and manual policy")
        try Data([2,3]).write(to:b)
        both.rescan(); bs.scans.last!.completion(.success([ra,row(b,missing:true)]))
        check(both.selectedIDs == [a.path] && both.manualItems.count == 1,"source change invalidates only affected selection and manual policy")
        both.rescan(); bs.scans.last!.completion(.success([row(b,missing:true)]))
        check(both.selectedIDs.isEmpty && both.hasExplicitSelection,"deleted selected source never silently restores default all")
        let (empty,es) = setup()
        empty.toggleSelection(empty.items[0]); empty.toggleSelection(empty.items[0]); empty.start(); empty.resolveManualPrompt(process:false); RunLoop.current.run(until:Date(timeIntervalSinceNow:0.02))
        check(es.runs.isEmpty && empty.hasExplicitSelection,"explicit empty selection never converts the full queue")
        for dismissedFirst in [true,false] {
            let (life,ls) = setup()
            life.toggleSelection(life.items[0]); life.toggleSelection(life.items[1])
            life.start(outputOverride:out)
            if dismissedFirst { life.manualPromptPresented = false }
            life.resolveManualPrompt(process:false)
            if !dismissedFirst { life.manualPromptPresented = false }
            check(ls.runs.isEmpty && !life.canStart && life.manualDecisionPending,"skip waits until the alert event completes without a duplicate start")
            life.resolveManualPrompt(process:nil) // A duplicate dismissal cannot cancel an answered plan.
            pump(until:{ !ls.runs.isEmpty })
            check(ls.runs.count == 1 && ls.runs[0].job.sources == [a] && life.selectedIDs == Set([a.path,b.path]),"dismiss ordering preserves exact ready plan and hidden selection")
        }
        let (unrelated,us) = setup()
        unrelated.start(); try Data([2,3,4]).write(to:b)
        unrelated.manualPromptPresented = false
        unrelated.resolveManualPrompt(process:false); pump(until:{ !us.runs.isEmpty })
        check(us.runs[0].job.sources == [a],"a changed skipped manual source cannot block unchanged ready targets")
        let (aborted,aservice) = setup()
        aborted.start(); aborted.manualPromptPresented = false; aborted.resolveManualPrompt(process:nil)
        RunLoop.current.run(until:Date(timeIntervalSinceNow:0.02))
        check(aservice.runs.isEmpty && !aborted.manualPreparation,"native dismissal plus explicit cancel never starts a job")
        let (cap,_) = setup()
        var pending = [(Bool)->Void]()
        cap.inspectCoverCandidates = { _,reply in pending.append(reply) }
        let entry = cap.items[0]
        cap.requestCoverCandidates(entry)
        check(!cap.hasCoverCandidates(entry),"loading capability never exposes an empty cover action")
        pending[0](true)
        check(cap.hasCoverCandidates(entry),"only actual auxiliary candidates expose the cover action")
        cap.requestCoverCandidates(entry)
        check(pending.count == 1,"same-version capability reuses metadata without repeated decoding")
        cap.updateSettings { $0.direction = .androidToAndroid }
        check(!cap.hasCoverCandidates(entry),"direction invalidates candidate capability")
        pending[0](true)
        check(!cap.hasCoverCandidates(entry),"stale capability callback cannot cross direction")
        let (none,_) = setup()
        none.inspectCoverCandidates = { _,reply in reply(false) }
        none.requestCoverCandidates(none.items[0])
        check(!none.hasCoverCandidates(none.items[0]),"a decodable main video alone never exposes a cover action")
        let (late,_) = setup()
        var lateReply: ((Bool)->Void)?
        late.inspectCoverCandidates = { _,reply in lateReply=reply }
        let lateItem=late.items[0]; late.toggleSelection(lateItem); late.requestCoverCandidates(lateItem)
        try Data([7,8,9]).write(to:a);lateReply?(true)
        check(!late.hasCoverCandidates(lateItem) && late.selectedIDs == [a.path],"late metadata cannot resurrect a changed source capability or mutate selection")
        let (stale,ss) = setup()
        stale.start(); stale.resolveManualPrompt(process:true); stale.confirmManualMidpoint(stale.items[1])
        try Data([9]).write(to:b)
        stale.continueManualConversion()
        check(ss.runs.isEmpty && stale.notice != nil,"late source changes block an already confirmed manual transaction")
    }

    static func staticHDRChecks(root: URL) throws {
        let input = root.appendingPathComponent("static-input")
        try FileManager.default.createDirectory(at:input,withIntermediateDirectories:true)
        let hdr = input.appendingPathComponent("static-hdr.jpg"), motion = input.appendingPathComponent("motion.jpg")
        try Data([1]).write(to:hdr); try Data([2]).write(to:motion)
        let row = ScanRow(source:hdr,relative:"static-hdr.jpg",isMotion:false,hdr:"Ultra HDR",cover:"不适用",detail:"静态 HDR",confirmedStatic:true,version:SourceVersion.read(hdr))
        let dynamic = ScanRow(source:motion,relative:"motion.jpg",isMotion:true,hdr:"SDR",cover:"0.0 秒",detail:"",version:SourceVersion.read(motion))
        let service = ControlledService()
        let model = WorkspaceModel(service:service); model.chooseSources([hdr,motion]); service.scans[0].completion(.success([row,dynamic]))
        check(model.items[0].isConvertible && !model.items[0].isMotion && model.category(model.items[0]) == .ready,"static HDR is ready without claiming motion")
        model.toggleSelection(model.items[0]); model.selectQueueScope(.completed); model.toggleSelection(model.items[1])
        check(model.selectedIDs == Set([hdr.path,motion.path]),"static HDR and motion share cross-tab multiselection")
        let output=root.appendingPathComponent("static-output")
        try FileManager.default.createDirectory(at:output,withIntermediateDirectories:true)
        model.selectQueueScope(.ready); model.setOutput(output); model.start()
        check(service.runs.count == 1 && Set(service.runs[0].job.sources) == Set([hdr,motion]),"static HDR is included in the actual mixed conversion plan")
        // Source provenance is tested without opening a Photos library.
        let photosService=ControlledService()
        let libraryModel=WorkspaceModel(service:photosService)
        let origins=[hdr.path:LibraryOrigin(assetID:"static-asset",version:"v1",edited:false,filename:hdr.lastPathComponent),
                     motion.path:LibraryOrigin(assetID:"live-asset",version:"v1",edited:false,filename:motion.lastPathComponent)]
        libraryModel.appendSources([hdr,motion],origins:origins)
        photosService.scans[0].completion(.success([row,dynamic]))
        let staticItem=libraryModel.items.first { $0.source == hdr }!
        let motionItem=libraryModel.items.first { $0.source == motion }!
        check(!staticItem.isConvertible && libraryModel.category(staticItem) == .static,"Photos-origin static HDR stays outside the conversion queue")
        libraryModel.toggleSelection(staticItem)
        check(libraryModel.selectedIDs.isEmpty,"Photos-origin static HDR cannot enter a manual multiselection")
        libraryModel.toggleSelection(motionItem); libraryModel.setOutput(output); libraryModel.start()
        check(photosService.runs.count == 1 && photosService.runs[0].job.sources == [motion],"mixed Photos selection processes only actual motion photos")

    }

    static func staticHDRIntegration(source: URL, root: URL) throws {
        for direction:ConversionDirection in [.toApple,.toAndroid,.androidToAndroid] {
            let model=WorkspaceModel(); model.updateSettings { $0.direction=direction; $0.exportDataFiles=false }
            model.chooseSources([source]); pump(until:{ !model.isBusy })
            check(model.items.first?.row.isStaticHDR == true && model.items.first?.isMotion == false,"production static HDR scan stays static in every direction")
            model.toggleSelection(model.items[0]); let out=root.appendingPathComponent("hdr-" + direction.rawValue)
            try FileManager.default.createDirectory(at:out,withIntermediateDirectories:true)
            model.setOutput(out); model.start(); pump(until:{ !model.isBusy })
            check(model.report?.counts["converted"] == 1,"production model actually processes selected static HDR")
            let outcome=try NativeBatch.run(sources:[source],output:out,options:model.settings.options,token:StopToken(),progress:{ _ in })
            let delivered=try DeliveryStore.collect(outcome,settings:model.settings,origins:[:])
            check(delivered.count == 1 && delivered[0].files.count == 1 && delivered[0].standaloneHDR == true,"static result exports exactly one image without a video")
            try delivered[0].verify()
            var task=DeliveryTask(settings:model.settings,output:out.path,results:delivered)
            let store=DeliveryStore(root:root.appendingPathComponent("hdr-share-" + direction.rawValue))
            let staged=try store.stageShare(&task)
            let same = try Data(contentsOf:staged[0]) == Data(contentsOf:URL(fileURLWithPath:delivered[0].files[0]))
            check(staged.count == 1 && same,"static share retains complete output bytes")
        }
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
                    "Photos reverse retains Live Photo filtering; local static HDR does not broaden library reads")
            }
            check(!options.predicate!.evaluate(with: ["mediaType": PHAssetMediaType.video.rawValue, "mediaSubtypes": live]),
                "standalone videos are excluded from photo scanning")
        }
        check(AppName.display(languages:["en-US"]) == "MomentStays" && AppName.display(languages:["fr"]) == "MomentStays", "English and unsupported language product fallback")
        check(AppName.display(languages:["zh-Hans"]) == "留刻" && AppName.display(languages:["zh-Hant-TW"]) == "留刻", "all Chinese language product names")
        let oldPreferences:[String:Any] = ["old-only":7,"shared":"old"]
        let migrated = AppIdentityMigration.merged(previous:oldPreferences,current:["shared":"new"],oldID:"old")
        check(migrated["old-only"] as? Int == 7 && migrated["shared"] as? String == "new" && oldPreferences["shared"] as? String == "old", "migration copies missing preferences without overwriting either domain")
        check(AppIdentityMigration.merged(previous:["later":1],current:migrated,oldID:"old")["later"] == nil, "identity migration is traceable and idempotent")
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
        check(PhotoQueueItem(missingRow).category(settings:ConversionSettings()) == .manual && PhotoQueueItem(missingRow).category(settings:relaxed) == .ready,"cover classification follows the selected policy")
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

        try staticHDRChecks(root:root)
        try manualChecks(root:root)
        try navigationChecks(source:source,row:row,root:root)
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
            try manualIntegration(source: URL(fileURLWithPath: CommandLine.arguments[1]), root: root)
        }
        if CommandLine.arguments.count > 3 { try staticHDRIntegration(source:URL(fileURLWithPath:CommandLine.arguments[3]),root:root) }
        if CommandLine.arguments.count > 2 { try legacyIntegration(source:URL(fileURLWithPath:CommandLine.arguments[2]),root:root) }
        print("PASS \(checks) workspace state, cancellation and integration checks")
    }

    static func legacyIntegration(source:URL, root:URL) throws {
        let input = root.appendingPathComponent("legacy-source")
        try FileManager.default.createDirectory(at:input,withIntermediateDirectories:true)
        let copy = input.appendingPathComponent("legacy.jpg")
        try FileManager.default.copyItem(at:source,to:copy)
        let model = WorkspaceModel(); model.chooseSources([copy]); pump(until:{ !model.isBusy })
        check(model.manualItems.count == 1 && model.items.first?.requiresManualFrame == true,"production cache scans legacy input into the real manual tab")
        let item = model.items[0]
        model.confirmManualMidpoint(item)
        check(model.settings.manualMidpoints?[item.id] == nil && model.manualItems.count == 1,"legacy input cannot use midpoint approval")
        let output = root.appendingPathComponent("legacy-output")
        try FileManager.default.createDirectory(at:output,withIntermediateDirectories:true); model.setOutput(output)
        let frames = try RustCore.engine(["mode":"manual-cover-frames","input":copy.path,"output":root.appendingPathComponent("legacy-preview.mp4").path])
        let choice = ManualCoverChoice(source_sha256:frames["source_sha256"] as! String,source_version:frames["source_version"] as! String,track_index:0,frame_index:0)
        model.toggleSelection(item); model.start(); model.resolveManualPrompt(process:nil)
        check(model.settings.manualCovers == nil && model.report == nil,"cancelled legacy preflight confirms nothing")
        model.selectQueueScope(.ready); model.start(); model.resolveManualPrompt(process:true)
        model.confirmManualFrame(choice,for:item)
        check(model.manualItems.isEmpty && model.settings.manualCovers?[item.id] == choice && model.selectedIDs == [item.id],"explicit main-frame confirmation retains global selection")
        model.continueManualConversion(); pump(until:{ !model.isBusy })
        if model.report?.counts["converted"] != 1 {
            fputs("Legacy model failure: phase=\(model.phase) notice=\(model.notice ?? "") error=\(model.errorMessage ?? "") entries=\(model.report?.entries.map(\.detail) ?? [])\n",stderr)
        }
        check(model.report?.counts["converted"] == 1,"App model converts the selected legacy main frame through Rust")
        guard let report = model.report else { throw Failure("missing legacy report") }
        let entry = report.entries.first!
        let outcome = BatchOutcome(counts:["converted":1],report:nil,reportData:["files":[["status":"converted","source":item.id,"output":entry.output!]]],cancelled:false)
        // Real outcomes carry all archive names; reconstruct from the mandatory manifest.
        let manifest = try RustCore.engine(["mode":"verify","input":entry.output!])
        let delivered = try DeliveryStore.collect(BatchOutcome(counts:outcome.counts,report:nil,reportData:["files":[["status":"converted","source":item.id,"output":entry.output!,"output_files":manifest["output_files"]!]]],cancelled:false),settings:model.settings,origins:[:])
        check(delivered.count == 1 && delivered[0].files.count == 2 && delivered[0].hashes.count > 4,"Photos resources exclude hash-protected original archives")
        try delivered[0].verify()
        let shareStore = DeliveryStore(root:root.appendingPathComponent("legacy-share"))
        var task = DeliveryTask(settings:model.settings,output:output.path,results:delivered)
        let share = try shareStore.stageShare(&task)
        check(share.count == 1 && share[0].pathExtension == "zip","legacy sharing keeps the media, manifest and original archives together")
        try FileManager.default.removeItem(at:URL(fileURLWithPath:entry.output!).appendingPathComponent("source-tail.bin"))
        do { try delivered[0].verify(); check(false,"missing sidecar must reject delivery") } catch { check(true,"missing legacy sidecar blocks recent delivery") }
        model.rescan(); pump(until:{ !model.isBusy }); check(model.settings.manualCovers?[item.id] == choice,"unchanged rescan preserves selected frame")
        try (Data(contentsOf:copy) + Data([0])).write(to:copy)
        model.rescan(); pump(until:{ !model.isBusy })
        check(model.settings.manualCovers?[item.id] == nil && model.selectedIDs.isEmpty,"changed legacy source drops manual frame and conversion selection")
        model.confirmManualFrame(choice,for:item)
        check(model.settings.manualCovers?[item.id] == nil,"stale frame callback cannot restore a selection")
    }

    static func manualIntegration(source:URL, root:URL) throws {
        let parsed = try PhotoFormat.parse(Array(Data(contentsOf:source)))
        let input = root.appendingPathComponent("manual-real-input")
        try FileManager.default.createDirectory(at:input,withIntermediateDirectories:true)
        let unknown = input.appendingPathComponent("unknown.jpg")
        try Data(fixtureMotion(still:parsed.still,video:parsed.video,options:AndroidOptions(),time:-1,frame:0)).write(to:unknown)
        let original = try NativeBatch.hashFile(unknown)
        let model = WorkspaceModel()
        model.updateSettings { $0.exportDataFiles = true }
        model.chooseSources([unknown]); pump(until:{ !model.isBusy })
        check(model.manualItems.count == 1,"real supported unknown timestamp enters manual processing")
        let output = root.appendingPathComponent("manual-real-output")
        try FileManager.default.createDirectory(at:output,withIntermediateDirectories:true); model.setOutput(output)
        model.toggleSelection(model.items[0]); model.start(); model.resolveManualPrompt(process:true)
        model.confirmManualMidpoint(model.items[0]); model.continueManualConversion(); pump(until:{ !model.isBusy })
        check(model.report?.counts["converted"] == 1 && model.queueScope == .completed,"real manual confirmation converts through production Rust engine")
        check(try! NativeBatch.hashFile(unknown) == original,"manual processing retains the original source")
        let pair = output.appendingPathComponent("unknown.jpg.livephoto")
        let still = try FileManager.default.contentsOfDirectory(at:pair,includingPropertiesForKeys:nil).first { $0.pathExtension == "jpg" }!
        let stillBytes = Array(try Data(contentsOf:still))
        check(try! PhotoFormat.codingHash(stillBytes) == PhotoFormat.codingHash(parsed.still),"manual midpoint retains original static image coding")
        var options = model.settings.options; options.missingTime = "midpoint"
        let resumed = try NativeBatch.run(sources:[unknown],output:output,options:options,token:StopToken()) { _ in }
        check(resumed.counts["resumed"] == 1,"confirmed manual conversion resumes its published output")
        model.presentExistingOutput = { _,_,reply in reply(.overwrite) }
        model.selectQueueScope(.ready); model.start(); pump(until:{ !model.isBusy })
        check(model.report?.counts["converted"] == 1,"manual output supports the App overwrite/retry path")
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
