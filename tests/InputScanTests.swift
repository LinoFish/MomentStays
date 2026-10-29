import Foundation

private final class PendingInspections: ConversionServing {
    struct Request {
        let sources:[URL]; let direction:ConversionDirection; let token:StopToken
        let rows:([ScanRow])->Void; let finish:(Result<[ScanRow],Error>)->Void
    }
    var requests:[Request] = []
    var jobs:[ConversionJob] = []
    func scan(sources:[URL], direction:ConversionDirection, token:StopToken, rowsReady:@escaping ([ScanRow])->Void, progress:@escaping (Int,Int)->Void, completion:@escaping (Result<[ScanRow],Error>)->Void) {
        requests.append(Request(sources:sources,direction:direction,token:token,rows:rowsReady,finish:completion))
    }
    func run(job:ConversionJob,token:StopToken,existingOutput:@escaping ExistingOutputPresenter,progress:@escaping (BatchProgress)->Void,completion:@escaping (Result<BatchOutcome,Error>)->Void) { jobs.append(job) }
}

@main struct InputScanTests {
    static var checks = 0
    static func check(_ value:Bool,_ message:String) {
        guard value else { fputs("FAIL: \(message)\n",stderr); exit(1) }; checks += 1
    }
    static func main() throws {
        let root=URL(fileURLWithPath:CommandLine.arguments[1]).standardizedFileURL
        if CommandLine.arguments.count>3 && CommandLine.arguments[2]=="--benchmark" {
            try ScanBenchmark.run(root:root,input:URL(fileURLWithPath:CommandLine.arguments[3])); return
        }
        if CommandLine.arguments.count>3 && CommandLine.arguments[2]=="--model-benchmark" {
            try ScanBenchmark.modelRun(root:root,input:URL(fileURLWithPath:CommandLine.arguments[3])); return
        }
        let fm=FileManager.default
        try fm.createDirectory(at:root,withIntermediateDirectories:true)
        let a=root.appendingPathComponent("a.jpg"),b=root.appendingPathComponent("b.jpg"),c=root.appendingPathComponent("c.jpg"),d=root.appendingPathComponent("d.jpg")
        for f in [a,b,c,d] { try Data([1,2,3]).write(to:f) }
        func row(_ f:URL,_ relative:String?=nil)->ScanRow {
            ScanRow(source:f,relative:relative ?? f.lastPathComponent,isMotion:true,hdr:"HDR",cover:"0.4 秒",detail:"diagnostic",manufacturer:"Google",manufacturerDetail:"source metadata",version:SourceVersion.read(f))
        }
        let service=PendingInspections(),model=WorkspaceModel(service:service)
        let ar=row(a),br=row(b),cr=row(c),dr=row(d)
        model.appendSources([a]);service.requests[0].finish(.success([ar]));model.selection=a.path
        model.appendSources([b])
        check(model.items.count==1 && model.items[0].row.version==ar.version,"append must retain old scan immediately, before new rows arrive")
        check(model.items[0].scanDetail=="diagnostic" && model.items[0].hdr=="HDR" && model.items[0].isMotion && model.selection==a.path,"metadata/support/selection survive append")
        service.requests[1].rows([ar,br]);check(model.items.count==2,"streamed cached rows upsert instead of duplicate")
        model.stop();service.requests[1].finish(.success([br]))
        check(model.items.count==2 && model.phase == .stopped && !model.canStart,"cancel preserves unvisited old rows but never enables a partial batch")
        model.appendSources([c]);let obsolete=service.requests.last!
        model.appendSources([d]);check(obsolete.token.isCancelled && model.sources.count==4,"append while inspecting extends batch and supersedes request")
        obsolete.finish(.success([cr]));check(model.items.contains{$0.source==a},"stale completion cannot replace retained results")
        service.requests.last!.finish(.success([ar,br,cr,dr]))
        check(model.items.count==4 && model.selection==a.path,"latest completion contains all old and new inputs")
        let count=service.requests.count;model.appendSources([b,a]);check(service.requests.count==count,"duplicate file addition does not restart inspection")
        model.selectCover(1,for:a)
        model.updateSettings{$0.direction = .androidToAndroid};let projection=service.requests.last!
        let e=root.appendingPathComponent("e.jpg");try Data([1]).write(to:e)
        model.appendSources([e]);check(projection.token.isCancelled && service.requests.last!.direction == .androidToAndroid,"addition after fast direction change uses latest projection")
        projection.finish(.success([ar]));service.requests.last!.finish(.success([ar,br,cr,dr,row(e)]))
        check(model.items.count==5,"late old direction cannot drop appended rows")
        check(model.settings.coverSelections?[a.path]==1,"unchanged source keeps its manual cover across direction changes")
        try Data([9]).write(to:a);try fm.removeItem(at:b)
        let f=root.appendingPathComponent("f.jpg");try Data([0]).write(to:f)
        model.appendSources([f]);check(!model.items.contains{$0.source==a || $0.source==b},"changed/deleted sources invalidate old rows immediately")
        check(model.settings.coverSelections?[a.path]==nil,"changed file must not inherit the previous file's manual cover")
        service.requests.last!.finish(.success([row(a),cr,dr,row(e),row(f)]))
        check(model.items.count==5 && model.items.first(where:{$0.source==a})!.row.version != ar.version,"new versions replace metadata, deleted files stay removed")
        let delayedService=PendingInspections(),delayed=WorkspaceModel(service:delayedService)
        delayed.appendSources([a]);delayedService.requests.last!.finish(.success([row(a)]));delayed.selectCover(1,for:a)
        delayed.updateSettings{$0.direction = .androidToAndroid}
        try Data([8]).write(to:a)
        delayed.appendSources([f]);delayedService.requests.last!.finish(.success([row(a),row(f)]))
        check(delayed.settings.coverSelections?[a.path]==nil,"rapid direction/addition cannot detach cover validity from its source version")
        let startService=PendingInspections(),startModel=WorkspaceModel(service:startService)
        startModel.appendSources([a]);startService.requests.last!.finish(.success([row(a)]));startModel.selectCover(1,for:a)
        let destination=root.deletingLastPathComponent().appendingPathComponent(root.lastPathComponent+"-output")
        try fm.createDirectory(at:destination,withIntermediateDirectories:true);startModel.setOutput(destination)
        try Data([7]).write(to:a);startModel.start()
        check(startService.jobs.count==1 && startService.jobs[0].options.coverSelections[a.path]==nil,"conversion cannot reuse a manual cover if source changes after inspection")
        if CommandLine.arguments.count>3 && CommandLine.arguments[2]=="--fixture" {
            try ScanBenchmark.productionChecks(root:root.appendingPathComponent("real"),input:URL(fileURLWithPath:CommandLine.arguments[3]))
        }
        print("PASS \(checks) input append/cancel/stale-direction regressions")
    }
}
