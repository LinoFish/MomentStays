import Foundation
@testable import MomentStaysBackend

@inline(never)
func measureParse(_ data: [UInt8]) throws -> Int {
    try autoreleasepool {
        let p = try PhotoFormat.parse(data)
        return p.still.count &+ p.video.count &+ (p.coverFrame ?? 0)
    }
}

@main
struct ParseBenchmark {
    static func main() throws {
        let data = Array(try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1])))
        let summary: [String: Any]
        do {
            summary = try autoreleasepool {
                let p = try PhotoFormat.parse(data)
                return ["format":p.format,"cover_frame":p.coverFrame.map { $0 as Any } ?? NSNull(),
                    "timestamp_us":p.timestamp.map { $0 as Any } ?? NSNull(),
                    "timestamp_tag":p.timestampTag.map { $0 as Any } ?? NSNull(),"ultra":p.ultra,
                    "still_sha256":sha(Data(p.still)),"video_sha256":sha(Data(p.video))]
            }
        } catch {
            print(String(data:try JSONSerialization.data(withJSONObject:["error":error.localizedDescription]),encoding:.utf8)!)
            return
        }
        var checksum = 0
        for _ in 0..<2 { checksum &+= try measureParse(data) }
        let start = DispatchTime.now().uptimeNanoseconds
        checksum &+= try measureParse(data)
        let elapsed = Double(DispatchTime.now().uptimeNanoseconds - start)
        let iterations = Int(min(100, max(1, ceil(10_000_000 / elapsed))))
        var samples: [Double] = []
        for _ in 0..<9 {
            let start = DispatchTime.now().uptimeNanoseconds
            for _ in 0..<iterations { checksum &+= try measureParse(data) }
            samples.append(Double(DispatchTime.now().uptimeNanoseconds - start) / 1_000_000 / Double(iterations))
        }
        let result: [String:Any] = ["summary":summary,"ms":samples,"iterations":iterations,"checksum":checksum]
        print(String(data:try JSONSerialization.data(withJSONObject:result),encoding:.utf8)!)
    }
}
