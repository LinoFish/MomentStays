import Foundation
import Darwin

struct ScanBenchmark {
 static func modelRun(root:URL,input:URL) throws {
  let fm=FileManager.default;try fm.createDirectory(at:root,withIntermediateDirectories:true)
  let data=try Data(contentsOf:input);var files:[URL]=[]
  for i in 0..<13 {let file=root.appendingPathComponent("photo-\(i).jpg");try data.write(to:file);files.append(file)}
  func wait(_ done:()->Bool) throws {
   let deadline=Date().addingTimeInterval(180)
   while !done() && Date()<deadline {_ = RunLoop.main.run(mode:.default,before:Date().addingTimeInterval(0.001))}
   guard done() else {throw Failure("production model scan timed out")}
  }
  var records:[[String:Any]]=[]
  for round in -1..<7 {
   for mode in (round % 2 == 0 ? ["incremental","full-rescan"] : ["full-rescan","incremental"]) {
    let service=ConversionService(),model=WorkspaceModel(service:service)
    var start=ProcessInfo.processInfo.systemUptime;model.appendSources(Array(files.prefix(12)));try wait{!model.isBusy}
    let firstMS=(ProcessInfo.processInfo.systemUptime-start)*1000
    guard model.items.count==12,model.items.allSatisfy({$0.isMotion}) else {throw Failure("initial model output incomplete")}
    let old=model.items.map{$0.source.path};model.selection=old[0]
    if mode=="full-rescan" {
     service.clearInspections();var cleared=false
     service.project(sources:[],direction:.toApple,token:StopToken(),rowsReady:{_ in},progress:{_,_ in},completion:{_ in cleared=true})
     try wait{cleared}
    }
    let before=service.inspections.parseCount
    start=ProcessInfo.processInfo.systemUptime;model.appendSources([files.last!])
    guard Set(model.items.map{$0.source.path})==Set(old) else {throw Failure("append hid retained rows")}
    try wait{!model.isBusy};let addMS=(ProcessInfo.processInfo.systemUptime-start)*1000
    guard model.items.count==13,model.items.allSatisfy({$0.isMotion}),model.selection==old[0] else {throw Failure("model append changed results/selection")}
    let parsed=service.inspections.parseCount-before
    guard parsed == (mode=="incremental" ? 1 : 13) else {throw Failure("model did unexpected parses")}
    model.updateSettings{$0.direction = .androidToAndroid};try wait{!model.isBusy}
    if mode=="full-rescan" {
     service.clearInspections();var cleared=false
     service.project(sources:[],direction:.toApple,token:StopToken(),rowsReady:{_ in},progress:{_,_ in},completion:{_ in cleared=true})
     try wait{cleared}
    }
    let backBefore=service.inspections.parseCount
    start=ProcessInfo.processInfo.systemUptime;model.updateSettings{$0.direction = .toApple};try wait{!model.isBusy}
    let backMS=(ProcessInfo.processInfo.systemUptime-start)*1000
    guard model.items.count==13,service.inspections.parseCount-backBefore == (mode=="incremental" ? 0 : 13) else {throw Failure("direction return reparsed or lost sources")}
    if round>=0 {records.append(["round":round,"mode":mode,"initial_ms":firstMS,"append_ms":addMS,"return_ms":backMS,"append_parses":parsed,"rows":model.items.count])}
   }
  }
  let encoded=try JSONSerialization.data(withJSONObject:["scope":"production ConversionService + WorkspaceModel, worker and main queue response; no GUI rendering/PhotoKit","warmup_rounds":1,"rounds":7,"old_count":12,"added_count":1,"records":records],options:[.prettyPrinted,.sortedKeys]);FileHandle.standardOutput.write(encoded)
 }
 static func productionChecks(root:URL,input:URL) throws {
  let fm=FileManager.default;try fm.createDirectory(at:root,withIntermediateDirectories:true)
  let original=try NativeBatch.hashFile(input),a=root.appendingPathComponent("old.jpg"),b=root.appendingPathComponent("new.jpg")
  let bytes=try Data(contentsOf:input);try bytes.write(to:a);try bytes.write(to:b)
  let cache=SourceInspectionCache()
  func rows(_ files:[URL],_ token:StopToken = StopToken()) throws -> [ScanRow] { try cache.rows(sources:files,direction:.androidToAndroid,token:token,rowsReady:{_ in},progress:{_,_ in}) }
  let first=try rows([a]),merged=try rows([a,b])
  InputScanTests.check(first.count==1 && merged.count==2 && merged.allSatisfy{$0.isMotion},"real increment preserves supported old/new sources")
  InputScanTests.check(cache.parseCount==2 && cache.sourceReadCount==2 && cache.projectionHitCount==1,"adding one source parses/reads only that source")
  let stopped=StopToken();stopped.cancel();let empty=root.appendingPathComponent("cancelled-folder");try fm.createDirectory(at:empty,withIntermediateDirectories:true)
  _ = try rows([empty],stopped);_ = try rows([a,b])
  InputScanTests.check(cache.parseCount==2,"cancelled inventory never evicts previously validated entries")
  var options=ConversionOptions();options.direction="android-to-android"
  let output=root.deletingLastPathComponent().appendingPathComponent("converted")
  let result=try NativeBatch.run(sources:[a,b],output:output,options:options,token:StopToken(),scanRows:merged){_ in}
  InputScanTests.check(result.counts["converted"]==2 && result.counts["failed"]==0,"real production batch converts mixed old and new inputs")
  let photo=try PhotoFormat.parse(Array(bytes))
  let outputs=(fm.enumerator(at:output,includingPropertiesForKeys:nil)?.allObjects as? [URL] ?? []).filter{$0.lastPathComponent.hasSuffix(".MP.jpg")}
  InputScanTests.check(outputs.count==2,"both inputs publish complete media")
  for file in outputs { try AndroidTransfer.validate(photo,result:Array(Data(contentsOf:file))) }
  let resumed=try NativeBatch.run(sources:[a,b],output:output,options:options,token:StopToken(),scanRows:merged){_ in}
  InputScanTests.check(resumed.counts["resumed"]==2 && resumed.counts["failed"]==0,"mixed input outputs resume through production safety checks")
  let finalHash=try NativeBatch.hashFile(input),firstBytes=try Data(contentsOf:a),secondBytes=try Data(contentsOf:b)
  InputScanTests.check(finalHash==original && firstBytes==bytes && secondBytes==bytes,"conversion leaves original and both inputs unchanged")
  try bytes.write(to:a);try fm.removeItem(at:b)
  let changed=try rows([root])
  InputScanTests.check(changed.count==1 && cache.parseCount==3 && changed[0].version != first[0].version,"real folder refresh invalidates changed source and removes deletion")
 }
 static func run(root:URL,input:URL) throws {
  let fm=FileManager.default
  try fm.createDirectory(at:root,withIntermediateDirectories:true)
  let original=try NativeBatch.hashFile(input), data=try Data(contentsOf:input)
  var files:[URL]=[]
  for i in 0..<25 { let file=root.appendingPathComponent("photo-\(i).jpg");if !fm.fileExists(atPath:file.path) { try data.write(to:file) };files.append(file) }
  let old=Array(files.prefix(24))
  func inspect(_ cache:SourceInspectionCache,_ sources:[URL],_ direction:ConversionDirection = .toApple) throws -> [ScanRow] {
   try cache.rows(sources:sources,direction:direction,token:StopToken(),rowsReady:{_ in},progress:{_,_ in})
  }
  func signature(_ rows:[ScanRow])->[String] { rows.map { [$0.source.path,$0.relative,String($0.isMotion),$0.hdr,$0.cover,$0.detail,$0.manufacturer,$0.manufacturerDetail,String(describing:$0.version)].joined(separator:"|") }.sorted() }
  var records:[[String:Any]]=[]
  for round in -1..<7 {
   for mode in (round % 2 == 0 ? ["incremental","full-rescan"] : ["full-rescan","incremental"]) {
    let cache=SourceInspectionCache()
    var start=ProcessInfo.processInfo.systemUptime
    let initial=try inspect(cache,old)
    let initialMS=(ProcessInfo.processInfo.systemUptime-start)*1000
    guard initial.count==24,initial.allSatisfy({$0.isMotion}) else { throw Failure("invalid benchmark initial workload") }
    if mode=="full-rescan" { cache.clear() }
    let beforeParse=cache.parseCount,beforeRead=cache.sourceReadCount,beforeHit=cache.projectionHitCount
    start=ProcessInfo.processInfo.systemUptime
    let appended=try inspect(cache,files)
    let addMS=(ProcessInfo.processInfo.systemUptime-start)*1000
    let parses=cache.parseCount-beforeParse,reads=cache.sourceReadCount-beforeRead,hits=cache.projectionHitCount-beforeHit
    guard appended.count==25,appended.allSatisfy({$0.isMotion}),parses == (mode=="incremental" ? 1 : 25),reads==parses else { throw Failure("incremental read/parse/workload contract failed") }
    // Both modes perform the same extra direction validation before timing return.
    _ = try inspect(cache,files,.androidToAndroid)
    if mode=="full-rescan" { cache.clear() }
    let beforeReturn=cache.parseCount
    start=ProcessInfo.processInfo.systemUptime
    let returned=try inspect(cache,files)
    let returnMS=(ProcessInfo.processInfo.systemUptime-start)*1000
    guard signature(appended)==signature(returned) else { throw Failure("direction return changed metadata/support") }
    var usage=rusage();_ = getrusage(RUSAGE_SELF,&usage)
    if round>=0 { records.append(["round":round,"mode":mode,"initial_ms":initialMS,"append_ms":addMS,"return_ms":returnMS,"append_parses":parses,"append_source_reads":reads,"append_projection_hits":hits,"return_parses":cache.parseCount-beforeReturn,"rows":returned.count,"peak_process_rss_bytes":Int64(usage.ru_maxrss)]) }
   }
  }
  guard try NativeBatch.hashFile(input)==original else { throw Failure("benchmark modified original") }
  let result:[String:Any] = ["rounds":7,"warmup_rounds":1,"old_count":24,"added_count":1,"input_bytes":data.count,"reference":"correct full reinspection; broken disappearing-row UI is not a timing baseline","scope":"production owned Rust ABI + SourceInspectionCache; excludes GUI rendering/PhotoKit","records":records]
  let encoded=try JSONSerialization.data(withJSONObject:result,options:[.prettyPrinted,.sortedKeys]);FileHandle.standardOutput.write(encoded)
 }
}
