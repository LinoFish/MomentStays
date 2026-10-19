import Foundation

@main
struct NativeDriver {
    static func main() {
        do {
            guard let req = try JSONSerialization.jsonObject(with:FileHandle.standardInput.readDataToEndOfFile()) as? [String:Any],
                  let mode = req["mode"] as? String else { throw Failure("Missing request") }
            let inputs = (req["inputs"] as? [String] ?? (req["input"] as? String).map { [$0] } ?? []).map { URL(fileURLWithPath:$0) }
            switch mode {
            case "fixture": try emit(fixture(req))
            case "android-still":
                let options = AndroidOptions(vendor:"google",container:req["container"] as? String ?? "jpeg",hdr:"ultra",imageQuality:req["image_quality"] as? Double ?? 0.95,gainQuality:req["gain_quality"] as? Double ?? 0.95,preserveJPEG:req["preserve_jpeg"] as? Bool ?? true)
                let (still,hdr) = try AndroidHDR.prepare(inputs[0],options:options)
                try Data(still).write(to:URL(fileURLWithPath:req["output"] as! String))
                try emit(["hdr":hdr,"bytes":still.count])
            case "format":
                let photo = try PhotoFormat.parse(Array(Data(contentsOf:URL(fileURLWithPath:req["input"] as! String))))
                var result: [String:Any] = ["timestamp_us":photo.timestamp.map { $0 as Any } ?? NSNull(),
                    "ultra":photo.ultra,"coding_hash":try PhotoFormat.codingHash(photo.still)]
                if let output = req["output"] as? String {
                    let still = try PhotoFormat.liveStill(photo.still,uuid:"CCA01975-5655-4A4C-88C6-D620975EF319")
                    try Data(still).write(to:URL(fileURLWithPath:output))
                    result["output_coding_hash"] = try PhotoFormat.codingHash(still)
                }
                try emit(result)
            case "batch":
                let token = StopToken()
                var options = ConversionOptions(legacyHDR:req["legacy_hdr"] as? Bool ?? true,
                    missingTime:req["missing_time"] as? String ?? "error",timeDomain:req["time_domain"] as? String ?? "auto",videoTrack:req["video_track"] as? String ?? "primary",outputMode:req["output_mode"] as? String ?? "livephoto",repairAuxiliaryDuration:req["repair_auxiliary_duration"] as? Bool ?? false,exportDataFiles:req["export_data_files"] as? Bool ?? true)
                options.direction = req["direction"] as? String ?? "to-apple"
                options.android = AndroidOptions(vendor:req["vendor"] as? String ?? "google",container:req["container"] as? String ?? "jpeg",hdr:"ultra",imageQuality:req["image_quality"] as? Double ?? 0.95,gainQuality:req["gain_quality"] as? Double ?? 0.95,preserveJPEG:req["preserve_jpeg"] as? Bool ?? true)
                let resolver: ((URL,URL)->ExistingOutputDecision)? = (req["existing_output"] as? String).map { policy in
                    { _,_ in policy == "overwrite" ? .overwrite : policy == "skip" ? .skip : .cancel }
                }
                let outcome = try NativeBatch.run(sources:inputs,
                    output:URL(fileURLWithPath:req["output"] as! String),options:options,token:token,existingOutput:resolver) { update in
                        if let after = req["cancel_after"] as? Int, update.index >= after { token.cancel() }
                    }
                var result: [String:Any] = ["counts":outcome.counts,"report":outcome.report.map { $0.path as Any } ?? NSNull(),"cancelled":outcome.cancelled]
                if outcome.report == nil { result["report_data"] = outcome.reportData }
                try emit(result)
            case "scan":
                let rows = try NativeBatch.scan(sources:inputs,direction:req["direction"] as? String ?? "to-apple",token:StopToken()) { _,_ in }
                try emit(["rows":rows.map { ["relative":$0.relative,"motion":$0.isMotion,"hdr":$0.hdr,"cover":$0.cover,"manufacturer":$0.manufacturer,"manufacturer_detail":$0.manufacturerDetail] as [String:Any] }])
            default: throw Failure("Unknown mode")
            }
        } catch { try? emit(["error":error.localizedDescription]); exit(1) }
    }
}
