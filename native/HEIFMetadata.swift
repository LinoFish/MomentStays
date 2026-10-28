import Foundation

// Repack item payloads without touching any coded image bytes. Rebuild iloc
// rather than guessing how offsets move when meta changes size.
enum HEIFMetadata {
    static func box(_ type:String, _ payload:[UInt8]) throws -> [UInt8] {
        guard payload.count <= Int(UInt32.max) - 8 else { throw Failure("HEIC box 超过 4 GiB") }
        return Binary.bytes(payload.count + 8,4) + Binary.string(type) + payload
    }
    static func write(_ data:[UInt8], xmp:[UInt8]) throws -> [UInt8] {
        let layout = try PhotoHEIF.layout(data)
        guard layout.boxes.allSatisfy({ ["ftyp","meta","mdat","free"].contains($0.type) }) else { throw Failure("HEIC 含未知顶层结构，未冒险修改编码或私有引用") }
        guard let primary = layout.children.first(where: { $0.type == "pitm" }), primary.end - primary.payload >= 6,
              data[primary.payload] <= 1 else { throw Failure("HEIC 主图引用无效") }
        let primaryID = try Binary.int(data,primary.payload + 4,data[primary.payload] == 0 ? 2 : 4)
        let mime = layout.items.filter { $0.type == "mime" }
        var primaryDescriptions: [Int] = []
        if let refs = layout.children.first(where: { $0.type == "iref" }) {
            guard refs.end - refs.payload >= 4, data[refs.payload] <= 1 else { throw Failure("HEIC iref 版本不受支持") }
            let width = data[refs.payload] == 0 ? 2 : 4
            for ref in try MotionVideoMetadata.boxes(data,refs.payload + 4,refs.end) where ref.type == "cdsc" {
                guard ref.end - ref.payload >= width + 2 else { throw Failure("HEIC 元数据引用被截断") }
                let from = try Binary.int(data,ref.payload,width), count = try Binary.int(data,ref.payload + width,2)
                guard ref.end - ref.payload == width + 2 + count * width else { throw Failure("HEIC 元数据引用长度无效") }
                for i in 0..<count where try Binary.int(data,ref.payload + width + 2 + i * width,width) == primaryID { primaryDescriptions.append(from) }
            }
        }
        let associated = mime.filter { primaryDescriptions.contains($0.id) }
        guard associated.count <= 1 else { throw Failure("HEIC 主图有多份 XMP，未推测替换") }
        let selected = associated.first
        let newID = selected?.id ?? ((layout.items.map { $0.id }.max() ?? 0) + 1)
        guard newID <= Int(UInt32.max), layout.items.count < 4096 else { throw Failure("HEIC 项目数量超限") }
        let ftyp = layout.boxes.first { $0.type == "ftyp" }!
        let prefix = Array(data[(ftyp.payload - 8)..<ftyp.end])
        var payload: [UInt8] = [], locations: [(Int,Int,Int)] = []
        for item in layout.items {
            let bytes = item.id == newID ? xmp : PhotoHEIF.bytes(data,item)
            locations.append((item.id,prefix.count + 8 + payload.count,bytes.count)); payload += bytes
        }
        if selected == nil { locations.append((newID,prefix.count + 8 + payload.count,xmp.count)); payload += xmp }
        var iloc: [UInt8] = [2,0,0,0,0x88,0] + Binary.bytes(locations.count,4)
        for (id,offset,length) in locations {
            iloc += Binary.bytes(id,4) + [0,0,0,0,0,1] + Binary.bytes(offset,8) + Binary.bytes(length,8)
        }
        let info = layout.children.first { $0.type == "iinf" }!
        let oldCountWidth = data[info.payload] == 0 ? 2 : 4
        var infe = Array(data[(info.payload + 4 + oldCountWidth)..<info.end])
        if selected == nil {
            let item = [UInt8]([3,0,0,0]) + Binary.bytes(newID,4) + [0,0] + Binary.string("mime\0application/rdf+xml\0\0")
            infe += try box("infe",item)
        }
        let iinf = [UInt8]([0,0,0,0]) + Binary.bytes(locations.count,2) + infe
        var description: [UInt8] = []
        let reference = layout.children.first { $0.type == "iref" }
        if selected == nil {
            let version = reference.map { data[$0.payload] } ?? (newID > 65535 || primaryID > 65535 ? 1 : 0)
            guard version <= 1, version == 1 || max(newID,primaryID) <= 65535 else { throw Failure("HEIC 元数据引用 ID 超限") }
            let width = version == 0 ? 2 : 4
            let cdsc = try box("cdsc",Binary.bytes(newID,width) + [0,1] + Binary.bytes(primaryID,width))
            description = reference.map { Array(data[$0.payload..<$0.end]) } ?? [version,0,0,0]
            description += cdsc
        }
        var children: [UInt8] = []
        for child in layout.children {
            if child.type == "iloc" { children += try box("iloc",iloc) }
            else if child.type == "iinf" { children += try box("iinf",iinf) }
            else if child.type == "iref" && !description.isEmpty { children += try box("iref",description) }
            else if child.type != "idat" { children += data[(child.payload - 8)..<child.end] }
        }
        if reference == nil && !description.isEmpty { children += try box("iref",description) }
        let result = prefix + (try box("mdat",payload)) + (try box("meta",[0,0,0,0] + children))
        guard try PhotoHEIF.codingHash(result) == PhotoHEIF.codingHash(data) else { throw Failure("HEIC 图像、增益图或方向/色彩属性发生变化") }
        return result
    }
}
