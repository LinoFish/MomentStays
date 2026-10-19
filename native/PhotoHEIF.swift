import Foundation

// Bounded HEIF item parsing. No pixel decode/encode: existing image items,
// properties, grids, thumbnails and references remain byte-for-byte intact.
enum PhotoHEIF {
    typealias Box = MotionVideoMetadata.Box
    struct Item {
        let id: Int
        let type: String
        let mime: String
        let method: Int
        let offsetAt: Int
        let lengthAt: Int
        let offsetWidth: Int
        let lengthWidth: Int
        let base: Int
        let ranges: [Range<Int>]
    }
    struct Layout {
        let boxes: [Box]
        let children: [Box]
        let items: [Item]
    }
    static func matches(_ data: [UInt8]) -> Bool {
        data.count >= 16 && Array(data[4..<8]) == Binary.string("ftyp") &&
        ["heic","heix","mif1"].contains(String(decoding:data[8..<12],as:UTF8.self))
    }
    static func layout(_ data: [UInt8]) throws -> Layout {
        let boxes = try MotionVideoMetadata.boxes(data,0,data.count)
        guard boxes.first?.type == "ftyp", !boxes.contains(where: { ["moov","mpvd"].contains($0.type) }),
              boxes.filter({ $0.type == "meta" }).count == 1 else { throw Failure("HEIC 静态容器结构不受支持或仍包含视频") }
        let meta = boxes.first { $0.type == "meta" }!
        guard meta.end - meta.payload >= 4, data[meta.payload] == 0 else { throw Failure("HEIC meta 版本不受支持") }
        let children = try MotionVideoMetadata.boxes(data,meta.payload + 4,meta.end)
        func one(_ type: String) throws -> Box {
            let found = children.filter { $0.type == type }
            guard found.count == 1 else { throw Failure("HEIC 缺少唯一的 \(type)") }
            return found[0]
        }
        let handler = try one("hdlr")
        guard handler.end - handler.payload >= 12, Array(data[handler.payload + 8..<handler.payload + 12]) == Binary.string("pict") else { throw Failure("HEIC 不是静态图像容器") }
        let info = try one("iinf"), loc = try one("iloc")
        var types: [Int:(String,String)] = [:]
        guard info.end - info.payload >= 6 else { throw Failure("HEIC iinf 被截断") }
        let infoVersion = data[info.payload]
        guard infoVersion <= 1 else { throw Failure("HEIC iinf 版本不受支持") }
        let countWidth = infoVersion == 0 ? 2 : 4
        let entries = try MotionVideoMetadata.boxes(data,info.payload + 4 + countWidth,info.end)
        guard entries.count <= 4096, try Binary.int(data,info.payload + 4,countWidth) == entries.count else { throw Failure("HEIC 项目数量无效") }
        for entry in entries {
            guard entry.type == "infe", entry.end - entry.payload >= 13 else { throw Failure("HEIC infe 无效") }
            let p = entry.payload, version = data[p]
            guard version == 2 || version == 3 else { throw Failure("HEIC infe 版本不受支持") }
            let width = version == 2 ? 2 : 4
            let id = try Binary.int(data,p + 4,width), typeAt = p + 6 + width
            guard typeAt + 4 < entry.end, try Binary.int(data,p + 4 + width,2) == 0 else { throw Failure("HEIC 加密项目不受支持") }
            let type = String(decoding:data[typeAt..<typeAt + 4],as:UTF8.self)
            guard ["hvc1","grid","Exif","mime","tmap","iden","jpeg"].contains(type), types[id] == nil else { throw Failure("HEIC 含未知或重复项目，未推测处理") }
            var mime = ""
            if type == "mime" {
                guard let zero = data[(typeAt + 4)..<entry.end].firstIndex(of:0), let end = data[(zero + 1)..<entry.end].firstIndex(of:0) else { throw Failure("HEIC MIME 项目被截断") }
                mime = String(decoding:data[(zero + 1)..<end],as:UTF8.self)
                guard mime == "application/rdf+xml" else { throw Failure("HEIC 未知 MIME 项目不受支持") }
            }
            types[id] = (type,mime)
        }
        var p = loc.payload
        func take(_ width: Int) throws -> Int {
            guard width >= 0, width <= 8, p <= loc.end, width <= loc.end - p else { throw Failure("HEIC iloc 字段被截断") }
            if width == 0 { return 0 }
            let value = try Binary.int(data,p,width); p += width; return value
        }
        let version = try take(1); _ = try take(3)
        guard version <= 2 else { throw Failure("HEIC iloc 版本不受支持") }
        let sizes = try take(1), bases = try take(1)
        let offsetWidth = sizes >> 4, lengthWidth = sizes & 15, baseWidth = bases >> 4, indexWidth = version == 0 ? 0 : bases & 15
        guard [4,8].contains(offsetWidth), [4,8].contains(lengthWidth), [0,4,8].contains(baseWidth), [0,4,8].contains(indexWidth) else { throw Failure("HEIC iloc 字段宽度不受支持") }
        let count = try take(version == 2 ? 4 : 2)
        guard count == types.count else { throw Failure("HEIC 项目描述与位置表数量不同") }
        var items: [Item] = [], owned: [Range<Int>] = []
        for _ in 0..<count {
            let id = try take(version == 2 ? 4 : 2), method = version == 0 ? 0 : try take(2)
            guard try take(2) == 0, method == 0 || method == 1, let type = types[id], !items.contains(where: { $0.id == id }) else { throw Failure("HEIC 项目引用或存储方式不受支持") }
            let base = try take(baseWidth), extentCount = try take(2)
            guard extentCount > 0, extentCount <= 1024 else { throw Failure("HEIC extent 数量无效") }
            var ranges: [Range<Int>] = [], offsetAt = 0, lengthAt = 0
            for _ in 0..<extentCount {
                guard try take(indexWidth) == 0 else { throw Failure("HEIC extent 索引不受支持") }
                offsetAt = p; let offset = try take(offsetWidth)
                lengthAt = p; let length = try take(lengthWidth)
                let storage = method == 1 ? children.filter { $0.type == "idat" } : boxes.filter { $0.type == "mdat" }
                guard method == 0 || storage.count == 1 else { throw Failure("HEIC idat 不唯一") }
                let origin = method == 1 ? storage[0].payload : 0
                guard base <= data.count, offset <= data.count - base, origin <= data.count - base - offset else { throw Failure("HEIC extent 偏移越界") }
                let start = origin + base + offset
                guard length > 0, length <= data.count - start,
                      storage.contains(where: { start >= $0.payload && start + length <= $0.end }) else { throw Failure("HEIC extent 不在图像数据范围") }
                let range = start..<start + length
                guard !owned.contains(where: { $0.overlaps(range) }) else { throw Failure("HEIC 项目数据重叠") }
                ranges.append(range); owned.append(range)
            }
            items.append(Item(id:id,type:type.0,mime:type.1,method:method,offsetAt:offsetAt,lengthAt:lengthAt,
                offsetWidth:offsetWidth,lengthWidth:lengthWidth,base:base,ranges:ranges))
        }
        guard p == loc.end else { throw Failure("HEIC iloc 尾部数据无效") }
        return Layout(boxes:boxes,children:children,items:items)
    }
    static func bytes(_ data: [UInt8], _ item: Item) -> [UInt8] { item.ranges.flatMap { Array(data[$0]) } }
    static func exif(_ data: [UInt8]) throws -> [UInt8] {
        let items = try layout(data).items.filter { $0.type == "Exif" }
        guard items.count == 1 else { throw Failure("HEIC 缺少唯一 EXIF 项目，暂不新建项目") }
        let raw = bytes(data,items[0])
        guard raw.count >= 12 else { throw Failure("HEIC EXIF 被截断") }
        let offset = try Binary.int(raw,0,4)
        guard offset <= raw.count - 12 else { throw Failure("HEIC EXIF TIFF 偏移越界") }
        let tiff = Array(raw[(4 + offset)...]); _ = try MLTIFF(tiff)
        return MLJPEG.exif + tiff
    }
    static func codingHash(_ data: [UInt8]) throws -> String {
        let value = try layout(data)
        var bytes: [UInt8] = []
        for item in value.items.sorted(by: { $0.id < $1.id }) where !["Exif","mime"].contains(item.type) {
            let payload = Self.bytes(data,item)
            bytes += Binary.bytes(item.id,4) + Binary.string(item.type) + Binary.bytes(payload.count,8) + payload
        }
        for box in value.children where ["iprp","iref","pitm"].contains(box.type) {
            if box.type == "iref" {
                guard box.end - box.payload >= 4, data[box.payload] <= 1 else { throw Failure("HEIC iref 版本不受支持") }
                let refs = try MotionVideoMetadata.boxes(data,box.payload + 4,box.end).filter { $0.type != "cdsc" }
                // Metadata descriptions do not change the image coding graph.
                if !refs.isEmpty {
                    bytes += data[box.payload..<box.payload + 4]
                    for ref in refs { bytes += data[(ref.payload - 8)..<ref.end] }
                }
            } else { bytes += data[box.payload..<box.end] }
        }
        return sha(Data(bytes))
    }
    static func parse(_ data: [UInt8]) throws -> NativeMotionPhoto {
        // Samsung SEF mpv2 contains an absolute pointer, not an EOF length.
        let vendor = try VendorMotion.locate(data,imageEnd:0)
        var p = 0, mpvdStart: Int?, mpvdEnd = 0
        while p < data.count {
            guard data.count - p >= 8 else { throw Failure("HEIC 顶层 box 被截断") }
            var size = try Binary.int(data,p,4), header = 8
            if size == 1 { size = try Binary.int(data,p + 8,8); header = 16 }
            guard size >= header, size <= data.count - p else { throw Failure("HEIC 顶层 box 长度无效") }
            if Array(data[p + 4..<p + 8]) == Binary.string("mpvd") { mpvdStart = p; mpvdEnd = p + size; break }
            p += size
        }
        guard mpvdStart != nil else { throw Failure(PhotoFormat.notMotion) }
        guard let boundary = mpvdStart, mpvdEnd == data.count,
              vendor == nil || (vendor!.format == "samsung-sef" && vendor!.start == boundary + 8 && vendor!.end <= mpvdEnd) else { throw Failure("三星 HEIC 的 mpvd/SEF 视频边界不一致") }
        let still = Array(data[..<boundary]); let value = try layout(still)
        let xmps = value.items.filter { $0.type == "mime" }
        guard xmps.count == 1 else { throw Failure("三星 HEIC 缺少唯一 XMP 项目") }
        let wrapped = [UInt8]([255,216]) + (try MLJPEG.app(225,MLXMP.header + bytes(still,xmps[0]))) + [255,217]
        let packets = try MLXMP.packets(wrapped)
        guard try MLXMP.integer(packets,"MotionPhoto") == 1 else { throw Failure(PhotoFormat.notMotion) }
        var time = try MLXMP.integer(packets,"MotionPhotoPresentationTimestampUs")
        if time == nil || time! < 0 { time = try MLXMP.integer(packets,"MicroVideoPresentationTimestampUs") }
        if let t = time, t < 0 { time = nil }
        let video = Array(data[(vendor?.start ?? boundary + 8)..<(vendor?.end ?? mpvdEnd)])
        let top = try MotionVideoMetadata.boxes(video,0,video.count)
        guard top.first?.type == "ftyp", top.contains(where: { $0.type == "moov" }), top.contains(where: { $0.type == "mdat" }) else { throw Failure("HEIC 动态视频无效") }
        _ = try exif(still)
        return NativeMotionPhoto(still:still,video:video,timestamp:time,timestampTag:time == nil ? nil : "MotionPhotoPresentationTimestampUs",ultra:false,format:vendor == nil ? "google-heic" : "samsung-heic-sef")
    }
    static func liveStill(_ data: [UInt8], uuid: String) throws -> [UInt8] {
        let value = try layout(data)
        let payload = try MLJPEG.exifIdentifier(exif(data),uuid:uuid)
        var output = data
        for item in value.items where ["Exif","mime"].contains(item.type) {
            guard item.ranges.count == 1, item.method == 0, item.base == 0 else { throw Failure("HEIC 元数据位置暂不支持无损更新") }
            let replacement: [UInt8]
            if item.type == "Exif" { replacement = [0,0,0,0] + Array(payload.dropFirst(6)) }
            else {
                let wrapper = [UInt8]([255,216]) + (try MLJPEG.app(225,MLXMP.header + bytes(data,item))) + [255,217]
                let cleaned = try PhotoFormat.liveStill(wrapper,uuid:uuid)
                let packets = try MLXMP.packets(cleaned)
                guard packets.count == 1 else { throw Failure("HEIC XMP 清理失败") }
                replacement = Array(packets[0].document.xmlData(options:[]))
            }
            // Zero obsolete metadata, including its motion pointer. Image item
            // bytes and all their absolute offsets remain unchanged.
            for range in item.ranges { output.replaceSubrange(range,with:[UInt8](repeating:0,count:range.count)) }
            let start = output.count + 8
            output += Binary.bytes(replacement.count + 8,4) + Binary.string("mdat") + replacement
            try Binary.put(start,into:&output,at:item.offsetAt,count:item.offsetWidth)
            try Binary.put(replacement.count,into:&output,at:item.lengthAt,count:item.lengthWidth)
        }
        guard try codingHash(output) == codingHash(data) else { throw Failure("HEIC 图像编码或色彩/方向属性发生变化") }
        return output
    }
}
