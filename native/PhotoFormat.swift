import Foundation
import CryptoKit

// The GUI uses this native implementation; no interpreter or external executable.
enum Binary {
    static func read(_ d: [UInt8], _ p: Int, _ count: Int, little: Bool = false) throws -> UInt64 {
        guard p >= 0, count > 0, count <= 8, p <= d.count, count <= d.count - p else { throw Failure("文件元数据被截断") }
        var result: UInt64 = 0
        for i in 0..<count { result = (result << 8) | UInt64(d[p + (little ? count - i - 1 : i)]) }
        return result
    }
    static func int(_ d: [UInt8], _ p: Int, _ count: Int, little: Bool = false) throws -> Int {
        let value = try read(d, p, count, little: little)
        guard value <= UInt64(Int.max) else { throw Failure("文件偏移超出范围") }
        return Int(value)
    }
    static func bytes(_ value: Int, _ count: Int, little: Bool = false) -> [UInt8] {
        (0..<count).map { UInt8(truncatingIfNeeded: value >> (8 * (little ? $0 : count - $0 - 1))) }
    }
    static func put(_ value: Int, into data: inout [UInt8], at: Int, count: Int = 4, little: Bool = false) throws {
        guard at >= 0, at + count <= data.count, value >= 0,
              count != 4 || value <= Int(UInt32.max) else { throw Failure("无法安全修正文件偏移") }
        data.replaceSubrange(at..<at + count, with: bytes(value, count, little: little))
    }
    static func string(_ value: String) -> [UInt8] { Array(value.utf8) }
}

struct JPEGSegment {
    let marker: UInt8
    let start: Int
    let end: Int
    let payload: [UInt8]
}

struct IFDField {
    let tag: Int
    let type: Int
    let count: Int
    var value: [UInt8]
    func offset(_ little: Bool) throws -> Int { try Binary.int(value, 0, 4, little: little) }
}

struct MLTIFF {
    let data: [UInt8]
    let little: Bool
    init(_ data: [UInt8]) throws {
        guard data.count >= 8 else { throw Failure("TIFF 头被截断") }
        guard Array(data[0..<2]) == [73, 73] || Array(data[0..<2]) == [77, 77] else { throw Failure("TIFF 字节序无效") }
        self.data = data
        little = data[0] == 73
        guard try Binary.int(data, 2, 2, little: little) == 42 else { throw Failure("TIFF 版本无效") }
    }
    func directory(_ offset: Int) throws -> ([IFDField], Int) {
        guard offset >= 8 else { throw Failure("TIFF 目录偏移无效") }
        return try Self.directory(data, at: offset, little: little)
    }
    static func directory(_ data: [UInt8], at: Int, little: Bool) throws -> ([IFDField], Int) {
        let n = try Binary.int(data, at, 2, little: little)
        guard at >= 0, at + 2 + n * 12 + 4 <= data.count else { throw Failure("TIFF 目录被截断") }
        var result: [IFDField] = []
        for i in 0..<n {
            let p = at + 2 + i * 12
            result.append(IFDField(tag: try Binary.int(data, p, 2, little: little),
                type: try Binary.int(data, p + 2, 2, little: little), count: try Binary.int(data, p + 4, 4, little: little),
                value: Array(data[p + 8..<p + 12])))
        }
        return (result, try Binary.int(data, at + 2 + n * 12, 4, little: little))
    }
    func root() throws -> ([IFDField], Int) { try directory(Binary.int(data, 4, 4, little: little)) }
    func exif() throws -> ([IFDField], Int) {
        let (fields, _) = try root()
        guard let pointer = fields.first(where: { $0.tag == 0x8769 }) else { return ([], 0) }
        return try directory(pointer.offset(little))
    }
    func field(_ tag: Int, _ type: Int, _ count: Int, _ value: Int) -> IFDField {
        IFDField(tag: tag, type: type, count: count, value: Binary.bytes(value, 4, little: little))
    }
    static func serialize(_ fields: [IFDField], next: Int = 0, little: Bool) -> [UInt8] {
        var out = Binary.bytes(fields.count, 2, little: little)
        for f in fields.sorted(by: { $0.tag < $1.tag }) {
            out += Binary.bytes(f.tag, 2, little: little) + Binary.bytes(f.type, 2, little: little)
            out += Binary.bytes(f.count, 4, little: little) + f.value
        }
        return out + Binary.bytes(next, 4, little: little)
    }
}

enum MLJPEG {
    static let exif = Binary.string("Exif\0\0")
    static let mpf = Binary.string("MPF\0")
    static let icc = Binary.string("ICC_PROFILE\0")
    static func segments(_ d: [UInt8]) throws -> [JPEGSegment] {
        guard d.starts(with: [255, 216]) else { throw Failure("不是 JPEG 文件") }
        var position = 2
        var result: [JPEGSegment] = []
        while position < d.count {
            let start = position
            guard d[position] == 255 else { throw Failure("JPEG 标记无效") }
            while position < d.count && d[position] == 255 { position += 1 }
            guard position < d.count else { break }
            let marker = d[position]
            position += 1
            if marker == 217 {
                result.append(JPEGSegment(marker: marker, start: start, end: position, payload: []))
                return result
            }
            guard marker != 216, marker != 1, !(208...215).contains(marker) else { throw Failure("JPEG 中出现意外独立标记") }
            let length = try Binary.int(d, position, 2)
            let end = position + length
            guard length >= 2, end <= d.count else { throw Failure("JPEG 数据段被截断") }
            let payload = Array(d[position + 2..<end])
            position = end
            if marker == 218 {
                while position < d.count {
                    guard let at = d[position...].firstIndex(of: 255) else { throw Failure("JPEG 扫描数据未结束") }
                    var p = at + 1
                    while p < d.count && d[p] == 255 { p += 1 }
                    guard p < d.count else { throw Failure("JPEG 扫描标记被截断") }
                    if d[p] == 0 || (208...215).contains(d[p]) { position = p + 1 }
                    else { position = at; break }
                }
            }
            result.append(JPEGSegment(marker: marker, start: start, end: position, payload: payload))
        }
        throw Failure("缺少 JPEG 结束标记")
    }
    static func codingHash(_ d: [UInt8]) throws -> String {
        // Hash each kept segment range in place. Concatenating them first
        // would copy the whole compressed image on every verification.
        var hash = SHA256()
        for segment in try segments(d) where !(224...239).contains(segment.marker) && segment.marker != 254 {
            hashRange(&hash, d, segment.start..<segment.end)
        }
        return hex(hash.finalize())
    }
    static func app(_ marker: UInt8, _ payload: [UInt8]) throws -> [UInt8] {
        guard payload.count + 2 <= 65535 else { throw Failure("JPEG 元数据段超过 64 KiB，无法安全写入") }
        return [255, marker] + Binary.bytes(payload.count + 2, 2) + payload
    }
    static func maker(_ uuid: String, old: [UInt8]) throws -> [UInt8] {
        let value = Binary.string(uuid + "\0")
        let header = Binary.string("Apple iOS\0\0\u{1}")
        if old.starts(with: header) {
            guard old.count >= 16, Array(old[12..<14]) == [73,73] || Array(old[12..<14]) == [77,77] else {
                throw Failure("Apple MakerNote 无效")
            }
            let little = old[12] == 73
            let (all, next) = try MLTIFF.directory(old, at: 14, little: little)
            guard next == 0 else { throw Failure("不支持链接的 Apple MakerNote 目录") }
            let end = 16 + all.count * 12 + 4
            let kept = all.filter { $0.tag != 17 }
            let delta = (kept.count + 1 - all.count) * 12
            let sizes = [1:1, 2:1, 3:2, 4:4, 5:8, 6:1, 7:1, 8:2, 9:4, 10:8, 11:4, 12:8]
            var fields: [IFDField] = []
            for var f in kept {
                guard let size = sizes[f.type] else { throw Failure("Apple MakerNote 字段类型未知") }
                if size * f.count > 4 {
                    let offset = try f.offset(little)
                    guard offset >= end, offset + size * f.count <= old.count else { throw Failure("Apple MakerNote 值偏移无效") }
                    f.value = Binary.bytes(offset + delta, 4, little: little)
                }
                fields.append(f)
            }
            fields.append(IFDField(tag: 17, type: 2, count: value.count, value: Binary.bytes(old.count + delta, 4, little: little)))
            return Array(old[..<14]) + MLTIFF.serialize(fields, little: little) + Array(old[end...]) + value
        }
        let f = IFDField(tag: 17, type: 2, count: value.count, value: Binary.bytes(32, 4))
        return header + [77,77] + MLTIFF.serialize([f], little: false) + value
    }
    static func exifIdentifier(_ payload: [UInt8]?, uuid: String) throws -> [UInt8] {
        let raw = payload.map { Array($0.dropFirst(6)) } ?? [77,77,0,42,0,0,0,8,0,0,0,0,0,0]
        let t = try MLTIFF(raw)
        let (root, rootNext) = try t.root()
        let (exifFields, exifNext) = try t.exif()
        var old: [UInt8] = []
        if let f = exifFields.first(where: { $0.tag == 0x927c }) {
            let offset = try f.offset(t.little)
            guard f.type == 7, f.count > 4, offset + f.count <= raw.count else { throw Failure("MakerNote 布局不支持") }
            old = Array(raw[offset..<offset + f.count])
        }
        let note = try maker(uuid, old: old)
        var out = raw
        if out.count % 2 != 0 { out.append(0) }
        let noteAt = out.count
        out += note
        if out.count % 2 != 0 { out.append(0) }
        let exifAt = out.count
        out += MLTIFF.serialize(exifFields.filter { $0.tag != 0x927c } + [t.field(0x927c, 7, note.count, noteAt)], next: exifNext, little: t.little)
        let rootAt = out.count
        out += MLTIFF.serialize(root.filter { $0.tag != 0x8769 } + [t.field(0x8769, 4, 1, exifAt)], next: rootNext, little: t.little)
        try Binary.put(rootAt, into: &out, at: 4, little: t.little)
        return exif + out
    }
    static func edit(_ data: [UInt8], replacements: [Int: [UInt8]], insert: [UInt8] = []) throws -> [UInt8] {
        let segs = try segments(data)
        var edits: [(Int,Int,[UInt8])] = segs.compactMap { s in replacements[s.start].map { (s.start,s.end,$0) } }
        if !insert.isEmpty { edits.append((2,2,insert)) }
        edits.sort { $0.0 == $1.0 ? $0.1 < $1.1 : $0.0 < $1.0 }
        func mapped(_ p: Int) throws -> Int {
            var delta = 0
            for (a,b,replacement) in edits {
                if b <= p { delta += replacement.count - (b-a) }
                else if a < p && p < b { throw Failure("MPF 指针指向被替换的元数据") }
            }
            return p + delta
        }
        var out: [UInt8] = [], cursor = 0
        for (a,b,replacement) in edits {
            guard a >= cursor else { throw Failure("JPEG 修改范围重叠") }
            out += data[cursor..<a]; out += replacement; cursor = b
        }
        out += data[cursor...]
        let delta = out.count - data.count
        for s in segs where s.marker == 226 && s.payload.starts(with: mpf) && replacements[s.start] == nil {
            let oldBase = s.start + 8
            let newBase = try mapped(s.start) + 8
            let t = try MLTIFF(Array(s.payload.dropFirst(4)))
            let (fields, _) = try t.root()
            guard let f = fields.first(where: { $0.tag == 0xb002 }) else { throw Failure("MPF 索引缺失") }
            let offset = try f.offset(t.little)
            guard f.count % 16 == 0, offset + f.count <= t.data.count else { throw Failure("MPF 索引无效") }
            for i in 0..<f.count / 16 {
                let at = offset + i * 16
                let size = try Binary.int(t.data, at + 4, 4, little: t.little)
                let relative = try Binary.int(t.data, at + 8, 4, little: t.little)
                if i == 0 {
                    guard relative == 0 else { throw Failure("MPF 主图偏移无效") }
                    try Binary.put(size + delta, into: &out, at: newBase + at + 4, little: t.little)
                } else if relative > 0 {
                    let target = oldBase + relative
                    guard target + size <= data.count, Array(data[target..<min(target + 2, data.count)]) == [255,216] else {
                        throw Failure("MPF 附加图片超出范围")
                    }
                    try Binary.put(mapped(target) - newBase, into: &out, at: newBase + at + 8, little: t.little)
                }
            }
        }
        guard try codingHash(data) == codingHash(out) else { throw Failure("主图压缩数据校验失败") }
        return out
    }
    static func auxiliary(_ data: [UInt8]) throws -> [[UInt8]] {
        for s in try segments(data) where s.marker == 226 && s.payload.starts(with: mpf) {
            let t = try MLTIFF(Array(s.payload.dropFirst(4)))
            let (fields, _) = try t.root()
            guard let f = fields.first(where: { $0.tag == 0xb002 }) else { throw Failure("MPF 索引缺失") }
            let offset = try f.offset(t.little)
            guard f.count % 16 == 0, offset + f.count <= t.data.count else { throw Failure("MPF 索引无效") }
            var images: [[UInt8]] = []
            for i in 0..<f.count / 16 {
                let at = offset + i * 16
                let size = try Binary.int(t.data, at + 4, 4, little: t.little)
                let relative = try Binary.int(t.data, at + 8, 4, little: t.little)
                let start = i == 0 ? 0 : s.start + 8 + relative
                guard size > 0, start + size <= data.count else { throw Failure("MPF 图片被截断") }
                images.append(Array(data[start..<start + size]))
            }
            return images
        }
        return []
    }
    // Only marker-delimited JPEG images belong in a still. Rebuild MPF image
    // ranges instead of retaining padding, vendor trailers or appended media.
    static func imageRanges(_ data: [UInt8]) throws -> [Range<Int>] {
        let segs = try segments(data)
        let primaryEnd = segs.last!.end
        var ranges = [0..<primaryEnd]
        let indexes = segs.filter { $0.marker == 226 && $0.payload.starts(with:mpf) }
        guard indexes.count <= 1 else { throw Failure("JPEG 包含多个 MPF 索引") }
        if let s = indexes.first {
            let t = try MLTIFF(Array(s.payload.dropFirst(4))), fields = try t.root().0
            guard let f = fields.first(where: { $0.tag == 0xb002 }), f.count >= 16, f.count % 16 == 0, f.count / 16 <= 64 else { throw Failure("MPF 图片索引无效") }
            let at = try f.offset(t.little)
            guard at <= t.data.count, f.count <= t.data.count - at else { throw Failure("MPF 图片索引被截断") }
            guard try Binary.int(t.data,at + 8,4,little:t.little) == 0 else { throw Failure("MPF 主图偏移无效") }
            for i in 1..<f.count / 16 {
                let size = try Binary.int(t.data,at + i * 16 + 4,4,little:t.little)
                let relative = try Binary.int(t.data,at + i * 16 + 8,4,little:t.little)
                let start = s.start + 8 + relative
                guard size > 0, start <= data.count, size <= data.count - start else { throw Failure("MPF 附加图超出范围") }
                let range = start..<start + size
                guard !ranges.contains(where: { $0.overlaps(range) }), try segments(Array(data[range])).last!.end == size else { throw Failure("MPF 附加图包含非图片数据或范围重叠") }
                ranges.append(range)
            }
        }
        return ranges
    }
    static func staticOnly(_ data: [UInt8]) throws -> [UInt8] {
        let ranges = try imageRanges(data)
        let honorHDR = try PhotoHDR.honorPrivate(data,imageEnd:ranges.map { $0.upperBound }.max()!)
        var output: [UInt8] = [], starts: [Int] = []
        for range in ranges { starts.append(output.count); output += data[range] }
        if let s = try segments(output).first(where: { $0.marker == 226 && $0.payload.starts(with:mpf) }) {
            let t = try MLTIFF(Array(s.payload.dropFirst(4))), fields = try t.root().0
            let at = try fields.first(where: { $0.tag == 0xb002 })!.offset(t.little)
            for i in ranges.indices {
                try Binary.put(ranges[i].count,into:&output,at:s.start + 8 + at + i * 16 + 4,little:t.little)
                try Binary.put(i == 0 ? 0 : starts[i] - s.start - 8,into:&output,at:s.start + 8 + at + i * 16 + 8,little:t.little)
            }
        }
        // A recognized static EHDR extension is metadata, not appended video.
        // The parser passes only bytes preceding the bounded vendor MP4.
        return output + (honorHDR ?? [])
    }
}

struct XMPPacket {
    let segment: JPEGSegment
    let document: XMLDocument
    let guid: String?
    let fragments: [JPEGSegment]
    init(segment: JPEGSegment, document: XMLDocument, guid: String? = nil, fragments: [JPEGSegment] = []) {
        self.segment = segment; self.document = document; self.guid = guid; self.fragments = fragments
    }
}

enum MLXMP {
    static let header = Binary.string("http://ns.adobe.com/xap/1.0/\0")
    static let extended = Binary.string("http://ns.adobe.com/xmp/extension/\0")
    static let camera = "http://ns.google.com/photos/1.0/camera/"
    static let container = "http://ns.google.com/photos/1.0/container/"
    static let item = "http://ns.google.com/photos/1.0/container/item/"
    static let hdr = "http://ns.adobe.com/hdr-gain-map/1.0/"
    static let rdf = "http://www.w3.org/1999/02/22-rdf-syntax-ns#"
    static let apple = "http://ns.apple.com/HDRGainMap/1.0/"
    static let pixel = "http://ns.apple.com/pixeldatainfo/1.0/"
    static let note = "http://ns.adobe.com/xmp/note/"
    static func all(_ node: XMLNode) -> [XMLElement] {
        var result: [XMLElement] = []
        if let element = node as? XMLElement { result.append(element) }
        for child in node.children ?? [] { result += all(child) }
        return result
    }
    static func uri(_ node: XMLNode) -> String? {
        if let uri = node.uri, !uri.isEmpty { return uri }
        return (node.parent as? XMLElement)?.resolveNamespace(forName: node.name ?? "")?.stringValue
    }
    static func values(_ node: XMLNode, _ namespace: String, _ name: String) -> [String] {
        var result: [String] = []
        for element in all(node) {
            if element.localName == name && uri(element) == namespace, let text = element.stringValue { result.append(text.trimmingCharacters(in: .whitespacesAndNewlines)) }
            for attr in element.attributes ?? [] where attr.localName == name && uri(attr) == namespace {
                if let text = attr.stringValue { result.append(text) }
            }
        }
        return result
    }
    static func property(_ element: XMLElement, _ namespace: String, _ name: String) -> String? {
        if let value = element.attributes?.first(where: { $0.localName == name && uri($0) == namespace })?.stringValue { return value }
        return element.children?.first(where: { $0.localName == name && uri($0) == namespace })?.stringValue
    }
    static func packets(_ data: [UInt8]) throws -> [XMPPacket] {
        var result: [XMPPacket] = []
        var groups: [String:[(JPEGSegment,Int,Int,[UInt8])]] = [:]
        func document(_ bytes: [UInt8]) throws -> XMLDocument {
            var xml = bytes
            while xml.last == 0 { xml.removeLast() }
            let doc = try XMLDocument(data: Data(xml), options: [.nodePreserveAll, .nodeLoadExternalEntitiesNever])
            guard doc.rootElement() != nil else { throw Failure("XMP 文档无根节点") }
            return doc
        }
        for s in try MLJPEG.segments(data) where s.marker == 225 {
            if s.payload.starts(with: extended) {
                let p = extended.count
                guard s.payload.count >= p + 40 else { throw Failure("Extended XMP 数据头被截断") }
                let guid = String(decoding:s.payload[p..<p + 32],as:UTF8.self).uppercased()
                guard guid.count == 32, guid.allSatisfy({ $0.isHexDigit && $0.isASCII }) else { throw Failure("Extended XMP 标识无效") }
                let size = try Binary.int(s.payload,p + 32,4), offset = try Binary.int(s.payload,p + 36,4)
                let bytes = Array(s.payload.dropFirst(p + 40))
                guard size > 0, size <= data.count, offset <= size, bytes.count <= size - offset else { throw Failure("Extended XMP 分片偏移无效") }
                groups[guid,default:[]].append((s,size,offset,bytes))
            } else if s.payload.starts(with: header) {
                result.append(XMPPacket(segment:s,document:try document(Array(s.payload.dropFirst(header.count)))))
            }
        }
        for guid in groups.keys.sorted() {
            let chunks = groups[guid]!.sorted { $0.2 < $1.2 }
            let size = chunks[0].1
            var xml: [UInt8] = []
            for (_,total,offset,bytes) in chunks {
                guard total == size, offset == xml.count else { throw Failure("Extended XMP 分片缺失、重叠或长度冲突") }
                xml += bytes
            }
            guard xml.count == size else { throw Failure("Extended XMP 分片不完整") }
            let digest = Insecure.MD5.hash(data:Data(xml)).map { String(format:"%02X",$0) }.joined()
            guard digest == guid else { throw Failure("Extended XMP 校验失败") }
            let fragments = chunks.map { $0.0 }.sorted { $0.start < $1.start }
            result.append(XMPPacket(segment:fragments[0],document:try document(xml),guid:guid,fragments:fragments))
        }
        for packet in result where packet.guid == nil {
            for guid in values(packet.document,note,"HasExtendedXMP") where groups[guid.uppercased()] == nil {
                throw Failure("XMP 引用的扩展数据缺失：\(guid)")
            }
        }
        return result
    }
    // Unchanged extension segments are left byte-identical (Pixel processing data
    // and other auxiliary metadata must not be reserialized unnecessarily).
    static func rewrites(_ packets: [XMPPacket], changed initial: Set<Int>) throws -> [Int:[UInt8]] {
        var changed = initial, replacements: [Int:[UInt8]] = [:], ids: [String:String] = [:]
        for i in initial.sorted() {
            let packet = packets[i]
            guard let guid = packet.guid else { continue }
            let xml = Array(packet.document.xmlData(options:[]))
            guard xml.count <= Int(UInt32.max) else { throw Failure("Extended XMP 数据过大") }
            let newGUID = Insecure.MD5.hash(data:Data(xml)).map { String(format:"%02X",$0) }.joined()
            ids[guid] = newGUID
            let maximum = 65533 - extended.count - 40
            var bytes: [UInt8] = []
            for offset in stride(from:0,to:xml.count,by:maximum) {
                let chunk = Array(xml[offset..<min(offset + maximum,xml.count)])
                bytes += try MLJPEG.app(225,extended + Binary.string(newGUID) + Binary.bytes(xml.count,4) + Binary.bytes(offset,4) + chunk)
            }
            for fragment in packet.fragments { replacements[fragment.start] = [] }
            replacements[packet.segment.start] = bytes
        }
        for (old,new) in ids {
            var linked = false
            for (i,packet) in packets.enumerated() where packet.guid == nil {
                for element in all(packet.document) {
                    var fields = (element.attributes ?? []).filter { $0.localName == "HasExtendedXMP" && uri($0) == note }
                    if element.localName == "HasExtendedXMP" && uri(element) == note { fields.append(element) }
                    for field in fields where field.stringValue?.uppercased() == old {
                        field.stringValue = new; changed.insert(i); linked = true
                    }
                }
            }
            guard linked else { throw Failure("需要更新的 Extended XMP 缺少主数据引用") }
        }
        for i in changed where packets[i].guid == nil { replacements[packets[i].segment.start] = try segment(packets[i].document) }
        return replacements
    }
    static func integer(_ packets: [XMPPacket], _ name: String) throws -> Int? {
        let text = packets.flatMap { values($0.document, camera, name) }
        let numbers = try text.map { text -> Int in
            guard let number = Int(text.trimmingCharacters(in: .whitespacesAndNewlines)) else { throw Failure("XMP 时间或偏移数值无效：\(name)") }
            return number
        }
        let unique = Set(numbers)
        guard unique.count <= 1 else { throw Failure("XMP 字段冲突：\(name)") }
        return unique.first
    }
    static func segment(_ document: XMLDocument) throws -> [UInt8] {
        try MLJPEG.app(225, header + Array(document.xmlData(options: [])))
    }
}

struct NativeMotionPhoto {
    let still: [UInt8]
    let video: [UInt8]
    let timestamp: Int?
    let timestampTag: String?
    let ultra: Bool
    var coverFrame: Int? = nil
    var format: String = "google"
}

// Bounded ISO BMFF walk for the Google still-frame timestamp when an editor
// removed the main XMP packet but retained the complete appended MP4.
enum MotionVideoMetadata {
    struct Box { let type: String; let payload: Int; let end: Int }
    static func boxes(_ data: [UInt8], _ start: Int, _ end: Int) throws -> [Box] {
        var result: [Box] = [], p = start
        guard start >= 0, end <= data.count, start <= end else { throw Failure("MP4 元数据范围无效") }
        while p < end {
            guard end - p >= 8 else { throw Failure("MP4 元数据头被截断") }
            var size = try Binary.int(data,p,4), header = 8
            if size == 1 {
                guard end - p >= 16 else { throw Failure("MP4 扩展数据头被截断") }
                size = try Binary.int(data,p + 8,8); header = 16
            } else if size == 0 { size = end - p }
            guard size >= header, size <= end - p else { throw Failure("MP4 元数据长度无效") }
            result.append(Box(type:String(decoding:data[p + 4..<p + 8],as:UTF8.self),payload:p + header,end:p + size))
            p += size
        }
        return result
    }
    static func child(_ data: [UInt8], _ parent: Box, _ type: String) throws -> Box {
        let found = try boxes(data,parent.payload,parent.end).filter { $0.type == type }
        guard found.count == 1 else { throw Failure("Google 视频元数据缺少唯一的 \(type)") }
        return found[0]
    }
    static func word(_ data: [UInt8], _ box: Box, _ offset: Int, _ width: Int = 4) throws -> Int {
        guard offset >= 0, box.payload + offset + width <= box.end else { throw Failure("Google 视频元数据表被截断") }
        return try Binary.int(data,box.payload + offset,width)
    }
    struct Descriptor { let tag: Int; let payload: Int; let end: Int }
    static func descriptors(_ data: [UInt8], _ start: Int, _ end: Int) throws -> [Descriptor] {
        var p = start, result: [Descriptor] = []
        while p < end {
            let tag = Int(data[p]); p += 1
            var size = 0, complete = false
            for _ in 0..<4 {
                guard p < end else { throw Failure("Google 封面描述符被截断") }
                let value = data[p]; p += 1
                size = (size << 7) | Int(value & 127)
                if value & 128 == 0 { complete = true; break }
            }
            guard complete, size <= end - p else { throw Failure("Google 封面描述符长度无效") }
            result.append(Descriptor(tag:tag,payload:p,end:p + size)); p += size
        }
        return result
    }
    static func imageSamples(_ data: [UInt8]) throws -> [Range<Int>] {
        let top = try boxes(data,0,data.count)
        guard let moov = top.first(where: { $0.type == "moov" }) else { return [] }
        var samples: [Range<Int>] = []
        for track in try boxes(data,moov.payload,moov.end) where track.type == "trak" {
            let mdia = try child(data,track,"mdia"), handler = try child(data,mdia,"hdlr")
            guard handler.end - handler.payload >= 12,
                  String(decoding:data[handler.payload + 8..<handler.payload + 12],as:UTF8.self) == "meta" else { continue }
            let stbl = try child(data,child(data,mdia,"minf"),"stbl"), stsd = try child(data,stbl,"stsd")
            guard try word(data,stsd,4) == 1 else { continue }
            let entries = try boxes(data,stsd.payload + 8,stsd.end)
            guard entries.count == 1, entries[0].type == "mett" else { continue }
            let entry = entries[0]
            let fields = data[entry.payload..<entry.end].split(separator:0).map { String(decoding:$0,as:UTF8.self) }
            guard fields.contains("application/motionphoto-image-meta") else { continue }
            let sizes = try child(data,stbl,"stsz"), chunks = try child(data,stbl,"stsc")
            guard try word(data,sizes,8) == 1, try word(data,chunks,4) == 1,
                  try word(data,chunks,8) == 1, try word(data,chunks,12) == 1, try word(data,chunks,16) == 1 else {
                throw Failure("Google 封面元数据不是单样本布局")
            }
            let tables = try boxes(data,stbl.payload,stbl.end).filter { ["stco","co64"].contains($0.type) }
            guard tables.count == 1, try word(data,tables[0],4) == 1 else { throw Failure("Google 封面块偏移无效") }
            let offset = try word(data,tables[0],8,tables[0].type == "co64" ? 8 : 4)
            let fixed = try word(data,sizes,4), size = try fixed == 0 ? word(data,sizes,12) : fixed
            guard size > 0, offset <= data.count, size <= data.count - offset,
                  top.contains(where: { $0.type == "mdat" && offset >= $0.payload && offset + size <= $0.end }) else {
                throw Failure("Google 封面样本超出媒体数据范围")
            }
            samples.append(offset..<offset + size)
        }
        return samples
    }
    struct ImageTimes: Equatable {
        let primary: Int?
        let highResolution: [Int?]
    }
    static func imageTimes(_ data: [UInt8]) throws -> ImageTimes? {
        var times: [ImageTimes] = []
        for sample in try imageSamples(data) {
            let roots = try descriptors(data,sample.lowerBound,sample.upperBound)
            guard roots.count == 1, roots[0].tag == 0xc0 else { throw Failure("Google 封面根描述符无效") }
            let scores = try descriptors(data,roots[0].payload,roots[0].end).filter { $0.tag == 0xc1 }
            guard scores.count == 1, scores[0].end - scores[0].payload >= 4 else { throw Failure("Google 封面评分描述符无效") }
            let children = try descriptors(data,scores[0].payload + 4,scores[0].end)
            let frames = children.filter { $0.tag == 0xc3 }
            guard frames.count == 1, frames[0].end - frames[0].payload == 12 else { throw Failure("Google 原始封面时间描述符无效") }
            let raw = try Binary.read(data,frames[0].payload + 4,8)
            let signed = Int64(bitPattern:raw)
            var high: [Int?] = []
            let tracks = children.filter { $0.tag == 0xc2 }
            guard tracks.count <= 1 else { throw Failure("Google 高清候选时间描述符重复") }
            if let track = tracks.first {
                guard track.end - track.payload >= 4 else { throw Failure("Google 高清候选时间描述符被截断") }
                let count = try Binary.int(data,track.payload,4)
                let entries = try descriptors(data,track.payload + 4,track.end)
                guard entries.count == count, entries.allSatisfy({ $0.tag == 0xc3 && $0.end - $0.payload == 12 }) else {
                    throw Failure("Google 高清候选帧数与时间描述符不一致")
                }
                for entry in entries {
                    let value = Int64(bitPattern:try Binary.read(data,entry.payload + 4,8))
                    high.append(value >= 0 ? Int(value) : nil)
                }
                let known = high.compactMap { $0 }
                guard zip(known,known.dropFirst()).allSatisfy({ $0 <= $1 }) else { throw Failure("Google 高清候选时间未按顺序排列") }
            }
            times.append(ImageTimes(primary:signed >= 0 ? Int(signed) : nil,highResolution:high))
        }
        guard times.allSatisfy({ $0 == times.first }) else { throw Failure("视频中的封面时间描述符冲突") }
        return times.first
    }
    static func timestamp(_ data: [UInt8]) throws -> Int? { try imageTimes(data)?.primary }
}

// Vendor container facts are independently implemented from the public SEF
// directory layout and Huawei's fixed 3×20-byte LIVE_ tail. No byte scanning
// for ftyp inside compressed pixels and no inferred cover midpoint.
enum PhotoVendor {
    struct Identity { let name: String; let detail: String }
    static func identify(_ data: [UInt8], format: String) -> Identity {
        if PhotoHEIF.matches(data), let exif = try? PhotoHEIF.exif(data), let app = try? MLJPEG.app(225,exif) {
            return identify([255,216] + app + [255,217],format:format)
        }
        var make = "", model = ""
        do {
            for segment in try MLJPEG.segments(data) where segment.marker == 225 && segment.payload.starts(with:MLJPEG.exif) {
                let t = try MLTIFF(Array(segment.payload.dropFirst(6))), fields = try t.root().0
                func text(_ tag: Int) throws -> String {
                    guard let field = fields.first(where: { $0.tag == tag }), field.type == 2, field.count > 0, field.count <= 1024 else { return "" }
                    let bytes: [UInt8]
                    if field.count <= 4 { bytes = Array(field.value.prefix(field.count)) }
                    else {
                        let offset = try field.offset(t.little)
                        guard offset <= t.data.count, field.count <= t.data.count - offset else { return "" }
                        bytes = Array(t.data[offset..<offset + field.count])
                    }
                    return String(decoding:bytes.prefix(while: { $0 != 0 }),as:UTF8.self).trimmingCharacters(in:.whitespacesAndNewlines)
                }
                make = try text(0x010f); model = try text(0x0110)
                break
            }
        } catch { /* Vendor identification must not prevent media conversion. */ }
        let normalized = make.uppercased()
        let brands: [(String,[String])] = [
            ("荣耀",["HONOR","荣耀"]),("华为",["HUAWEI","华为"]),
            ("小米",["XIAOMI","REDMI","POCO","小米","红米"]),
            ("三星",["SAMSUNG","三星"]),("OPPO",["OPPO"]),
            ("OnePlus",["ONEPLUS","一加"]),("realme",["REALME","真我"]),
            ("vivo",["VIVO","IQOO"]),("Google",["GOOGLE"]),("Apple",["APPLE"])]
        let formatName = ["huawei-honor":"华为/荣耀 LIVE_","samsung-sef":"三星 SEF","oppo":"OPPO/OnePlus","vivo-v2":"vivo V2","google-compatible":"Google 兼容" ][format] ?? format
        let camera = [make,model].filter { !$0.isEmpty }.joined(separator:" / ")
        if let brand = brands.first(where: { $0.1.contains(where: { normalized.hasPrefix($0) }) }) {
            return Identity(name:brand.0,detail:"厂商依据：原片 EXIF Make；\(camera)\n动态格式：\(formatName)")
        }
        let fallback = ["huawei-honor":"华为/荣耀","samsung-sef":"三星","oppo":"OPPO/OnePlus","vivo-v2":"vivo"][format]
        if let name = fallback { return Identity(name:name,detail:"厂商依据：动态照片格式（非机身识别）\nEXIF：\(camera.isEmpty ? "未记录" : camera)\n动态格式：\(formatName)") }
        return Identity(name:"未知",detail:"未找到可确认的厂商字段\nEXIF：\(camera.isEmpty ? "未记录" : camera)\n动态格式：\(formatName.isEmpty ? "未知" : formatName)。Google 兼容格式不代表 Google 厂商。")
    }
}

enum VendorMotion {
    struct Location { let start: Int; let end: Int; let frame: Int?; let format: String }
    static let oppo = "http://ns.oplus.com/photos/1.0/camera/"
    static let vivo = "http://ns.vivo.com/photos/1.0/camera/"
    static func integer(_ packets: [XMPPacket], _ uri: String, _ name: String) throws -> Int? {
        let strings = packets.flatMap { MLXMP.values($0.document,uri,name) }
        let values = try strings.map { text -> Int in
            guard let value = Int(text.trimmingCharacters(in:.whitespacesAndNewlines)) else { throw Failure("厂商 XMP 数值无效：\(name)") }
            return value
        }
        guard Set(values).count <= 1 else { throw Failure("厂商 XMP 字段冲突：\(name)") }
        return values.first
    }
    static func locate(_ data: [UInt8], imageEnd: Int) throws -> Location? {
        if data.count >= 60 {
            let tail = Array(data.suffix(60))
            let last = String(decoding:tail[40..<60],as:UTF8.self).trimmingCharacters(in:.whitespacesAndNewlines)
            if last.hasPrefix("LIVE_") {
                let first = String(decoding:tail[..<20],as:UTF8.self).trimmingCharacters(in:.whitespacesAndNewlines)
                // Huawei v3 samples also use v3_f31_c; the frame index remains
                // the decimal field before the optional, exact `_c` suffix.
                let frameField = first.hasSuffix("_c") ? String(first.dropLast(2)) : first
                guard let regex = frameField.range(of:"^v[1-6]_f[0-9]+$",options:.regularExpression), regex == frameField.startIndex..<frameField.endIndex,
                      let split = frameField.range(of:"_f"), let frame = Int(frameField[split.upperBound...]),
                      let length = Int(last.dropFirst(5)), length >= 36, length <= data.count - 40 else { throw Failure("华为/荣耀 LIVE_ 尾标无效") }
                var end = data.count - 60
                let start = end - (length - 20)
                guard start >= imageEnd, start < end else { throw Failure("华为/荣耀视频范围与静态图片重叠") }
                if first.hasSuffix("_c") {
                    // v3 `_c` carries six length-prefixed private records and
                    // their total byte count (including that final UInt32).
                    let bytes = try Binary.int(data,end - 4,4)
                    guard bytes >= 4, bytes <= end - start else { throw Failure("华为/荣耀私有数据长度无效") }
                    let privateStart = end - bytes
                    var at = privateStart
                    for type in [5,1,0x0306,2,3,4] {
                        guard at <= end - 12 else { throw Failure("华为/荣耀私有数据记录无效") }
                        let size = try Binary.int(data,at,4)
                        guard size >= 8, size <= end - 4 - at,
                              try Binary.int(data,at + 4,4,little:true) == type else { throw Failure("华为/荣耀私有数据记录无效") }
                        at += size
                    }
                    guard at == end - 4 else { throw Failure("华为/荣耀私有数据记录无效") }
                    end = privateStart
                }
                return Location(start:start,end:end,frame:frame,format:"huawei-honor")
            }
        }
        guard data.count >= 20, Array(data.suffix(4)) == Binary.string("SEFT") else { return nil }
        let footer = data.count - 8, size = try Binary.int(data,footer,4,little:true)
        guard size >= 12, size <= footer else { throw Failure("三星 SEF 目录长度无效") }
        let head = footer - size
        guard Array(data[head..<head + 4]) == Binary.string("SEFH") else { throw Failure("三星 SEFH 目录头无效") }
        let count = try Binary.int(data,head + 8,4,little:true)
        guard count <= (size - 12) / 12, 12 + count * 12 == size else { throw Failure("三星 SEF 目录越界") }
        var found: Location?, owned: [Range<Int>] = []
        for i in 0..<count {
            let entry = head + 12 + i * 12
            let marker = try Binary.int(data,entry + 2,2,little:true)
            let back = try Binary.int(data,entry + 4,4,little:true), length = try Binary.int(data,entry + 8,4,little:true)
            guard back <= head, length >= 8, length <= back else { throw Failure("三星 SEF 数据范围无效") }
            let at = head - back, range = at..<at + length
            guard at >= imageEnd, !owned.contains(where: { $0.overlaps(range) }), Array(data[entry..<entry + 4]) == Array(data[at..<at + 4]) else { throw Failure("三星 SEF 记录重叠或头部不一致") }
            owned.append(range)
            let nameLength = try Binary.int(data,at + 4,4,little:true)
            guard nameLength <= length - 8 else { throw Failure("三星 SEF 名称越界") }
            if marker == 0x0a30 {
                guard found == nil, nameLength == 16, String(decoding:data[at + 8..<at + 24],as:UTF8.self) == "MotionPhoto_Data" else { throw Failure("三星动态视频条目不唯一或名称错误") }
                var start = at + 24, end = at + length
                if end - start == 12 && Array(data[start..<start + 4]) == Binary.string("mpv2") {
                    let offset = try Binary.int(data,start + 4,4), bytes = try Binary.int(data,start + 8,4)
                    guard offset >= imageEnd, offset <= at, bytes <= at - offset else { throw Failure("三星 mpv2 视频指针越界") }
                    start = offset; end = offset + bytes
                }
                found = Location(start:start,end:end,frame:nil,format:"samsung-sef")
            }
        }
        return found
    }
}

enum PhotoFormat {
    static func codingHash(_ data: [UInt8]) throws -> String {
#if MOTIONLIVE_SWIFT_REFERENCE
        try PhotoHEIF.matches(data) ? PhotoHEIF.codingHash(data) : MLJPEG.codingHash(data)
#else
        return try RustCore.codingHash(data)
#endif
    }
    static func auxiliaryHashes(_ data: [UInt8]) throws -> [String] {
#if MOTIONLIVE_SWIFT_REFERENCE
        if PhotoHEIF.matches(data) { return [] } // All HEIF image items are covered by codingHash.
        return try MLJPEG.auxiliary(data).dropFirst().map { sha(Data($0)) }
#else
        return try RustCore.auxiliaryHashes(data)
#endif
    }
    static let notMotion = "不是受支持的动态照片"
    static func parse(_ data: [UInt8]) throws -> NativeMotionPhoto {
#if MOTIONLIVE_SWIFT_REFERENCE
        if PhotoHEIF.matches(data) { return try PhotoHEIF.parse(data) }
        let packets = try MLXMP.packets(data)
        let motion = try MLXMP.integer(packets, "MotionPhoto")
        let micro = try MLXMP.integer(packets, "MicroVideo")
        let identified = motion == 1 || micro == 1
        guard motion != 0 && (identified || (motion == nil && micro == nil)) else { throw Failure(notMotion) }
        var lengths: [Int] = []
        for packet in packets {
            for element in MLXMP.all(packet.document) {
                if MLXMP.property(element, MLXMP.item, "Semantic") == "MotionPhoto" {
                    guard let text = MLXMP.property(element, MLXMP.item, "Length"), let length = Int(text) else { throw Failure("动态视频长度缺失") }
                    lengths.append(length)
                }
            }
        }
        guard Set(lengths).count <= 1 else { throw Failure("动态视频长度冲突") }
        var length = try lengths.first ?? MLXMP.integer(packets, "MicroVideoOffset")
        if length == nil { length = try MLXMP.integer(packets,"MediaDataOffset") }
        if length == nil { length = try VendorMotion.integer(packets,VendorMotion.oppo,"MediaDataOffset") }
        let imageEnd = try MLJPEG.imageRanges(data).map { $0.upperBound }.max()!
        let vendor = try VendorMotion.locate(data,imageEnd:imageEnd)
        let recovered = !identified && length == nil && vendor == nil
        if recovered {
            let end = imageEnd
            guard end + 8 <= data.count, Array(data[end + 4..<end + 8]) == Binary.string("ftyp") else { throw Failure(notMotion) }
            length = data.count - end
        }
        let start: Int, end: Int
        if let vendor = vendor { start = vendor.start; end = vendor.end }
        else {
            guard let count = length, count >= 16, count < data.count else { throw Failure("无法从 XMP 定位动态视频") }
            start = data.count - count
            if let bytes = try VendorMotion.integer(packets,VendorMotion.oppo,"VideoLength") {
                guard bytes >= 16, bytes <= data.count - start else { throw Failure("OPPO 视频长度无效") }
                end = start + bytes
            } else { end = data.count }
        }
        guard start >= imageEnd, end <= data.count, end - start >= 16 else { throw Failure("视频与 JPEG 图像范围重叠") }
        let still = try MLJPEG.staticOnly(Array(data[..<start])), video = Array(data[start..<end])
        guard Array(video[4..<8]) == Binary.string("ftyp") else { throw Failure("XMP 偏移没有指向 MP4 视频") }
        var position = 0, boxes: Set<String> = []
        while position < video.count {
            guard position + 8 <= video.count else { throw Failure("MP4 数据头被截断") }
            var size = try Binary.int(video, position, 4), header = 8
            if size == 1 { size = try Binary.int(video, position + 8, 8); header = 16 }
            else if size == 0 { size = video.count - position }
            guard size >= header, size <= video.count - position else { throw Failure("MP4 box 长度无效") }
            boxes.insert(String(decoding: video[position + 4..<position + 8], as: UTF8.self))
            position += size
        }
        guard boxes.contains("moov"), boxes.contains("mdat") else { throw Failure("视频缺少 moov/mdat") }
        var time = try MLXMP.integer(packets, "MotionPhotoPresentationTimestampUs")
        var tag = "MotionPhotoPresentationTimestampUs"
        if time == nil || time! < 0 {
            let old = try MLXMP.integer(packets, "MicroVideoPresentationTimestampUs")
            time = (old != nil && old! >= 0) ? old : nil
            tag = "MicroVideoPresentationTimestampUs"
        }
        if time == nil, let value = try VendorMotion.integer(packets,VendorMotion.oppo,"MotionPhotoPrimaryPresentationTimestampUs"), value >= 0 {
            time = value; tag = "OpCamera:MotionPhotoPrimaryPresentationTimestampUs"
        }
        if recovered {
            guard let original = try MotionVideoMetadata.timestamp(video) else {
                throw Failure("原片缺少主 XMP，内嵌视频也未记录有效封面时间，无法无损恢复封面位置")
            }
            time = original; tag = "MP4:motionphoto-image-meta"
        }
        var ultra = packets.contains { !MLXMP.values($0.document, MLXMP.hdr, "Version").isEmpty }
        let images = try MLJPEG.auxiliary(still)
        if !ultra {
            for image in images.dropFirst() {
                if try MLXMP.packets(image).contains(where: { !MLXMP.values($0.document,MLXMP.hdr,"Version").isEmpty }) { ultra = true }
            }
        }
        if ultra {
            guard images.count >= 2 else { throw Failure("声明为 Ultra HDR，但缺少 MPF 增益图") }
            for image in images.dropFirst() { _ = try MLJPEG.segments(image) }
        }
        let oppoLength = try VendorMotion.integer(packets,VendorMotion.oppo,"VideoLength")
        let isVivo = packets.contains { !MLXMP.values($0.document,VendorMotion.vivo,"VMotionPhotoVersion").isEmpty }
        return NativeMotionPhoto(still: still, video: video, timestamp: time, timestampTag: vendor?.frame == nil ? (time == nil ? nil : tag) : "Huawei:frame-index", ultra: ultra,
            coverFrame:vendor?.frame,format:vendor?.format ?? (oppoLength != nil ? "oppo" : isVivo ? "vivo-v2" : "google-compatible"))
#else
        return try RustCore.parse(data)
#endif
    }
    // Some GCam files carry one real compressed auxiliary sample but zero stts,
    // mdhd and tkhd duration. An explicit opt-in repairs only those fixed-size
    // fields using the primary track's shortest positive frame duration.
    static func repairAuxiliaryDuration(_ data: [UInt8]) throws -> ([UInt8], [String:Any])? {
        let top = try MotionVideoMetadata.boxes(data,0,data.count)
        guard let moov = top.first(where: { $0.type == "moov" }) else { return nil }
        var videos: [MotionVideoMetadata.Box] = []
        for track in try MotionVideoMetadata.boxes(data,moov.payload,moov.end) where track.type == "trak" {
            let mdia = try MotionVideoMetadata.child(data,track,"mdia"), h = try MotionVideoMetadata.child(data,mdia,"hdlr")
            if h.payload + 12 <= h.end && Array(data[h.payload + 8..<h.payload + 12]) == Binary.string("vide") { videos.append(track) }
        }
        guard videos.count == 2 else { return nil }
        func child(_ box: MotionVideoMetadata.Box, _ name: String) throws -> MotionVideoMetadata.Box { try MotionVideoMetadata.child(data,box,name) }
        func word(_ box: MotionVideoMetadata.Box, _ at: Int) throws -> Int { try MotionVideoMetadata.word(data,box,at) }
        let mainMedia = try child(videos[0],"mdia"), auxMedia = try child(videos[1],"mdia")
        let mainHeader = try child(mainMedia,"mdhd"), auxHeader = try child(auxMedia,"mdhd"), trackHeader = try child(videos[1],"tkhd")
        let movieHeader = try child(moov,"mvhd")
        guard [mainHeader,auxHeader,trackHeader,movieHeader].allSatisfy({ $0.payload < $0.end && data[$0.payload] == 0 }) else { return nil }
        let mainTable = try child(child(mainMedia,"minf"),"stbl"), auxTable = try child(child(auxMedia,"minf"),"stbl")
        let times = try child(auxTable,"stts"), sizes = try child(auxTable,"stsz"), chunks = try child(auxTable,"stsc")
        guard try word(times,4) == 1, try word(times,8) == 1, try word(times,12) == 0,
              try word(sizes,8) == 1, try word(auxHeader,16) == 0, try word(trackHeader,20) == 0 else { return nil }
        guard !(try MotionVideoMetadata.boxes(data,videos[1].payload,videos[1].end)).contains(where: { $0.type == "edts" }),
              !(try MotionVideoMetadata.boxes(data,auxTable.payload,auxTable.end)).contains(where: { $0.type == "ctts" }),
              try word(chunks,4) == 1, try word(chunks,8) == 1, try word(chunks,12) == 1, try word(chunks,16) == 1 else { throw Failure("零时长辅助轨的布局不能安全修复") }
        let offsets = try MotionVideoMetadata.boxes(data,auxTable.payload,auxTable.end).filter { ["stco","co64"].contains($0.type) }
        guard offsets.count == 1, try word(offsets[0],4) == 1 else { throw Failure("零时长辅助轨偏移无效") }
        let offset = try MotionVideoMetadata.word(data,offsets[0],8,offsets[0].type == "co64" ? 8 : 4)
        let fixed = try word(sizes,4), size = try fixed == 0 ? word(sizes,12) : fixed
        guard size > 0, offset <= data.count, size <= data.count - offset,
              top.contains(where: { $0.type == "mdat" && offset >= $0.payload && offset + size <= $0.end }) else { throw Failure("零时长辅助轨的压缩样本范围无效") }
        let mainTimes = try child(mainTable,"stts"), count = try word(mainTimes,4)
        guard count > 0, count <= (mainTimes.end - mainTimes.payload - 8) / 8 else { throw Failure("主轨帧长表无效") }
        var positive: [Int] = []
        for i in 0..<count { let delta = try word(mainTimes,12 + i * 8); if delta > 0 { positive.append(delta) } }
        let mainScale = try word(mainHeader,12), auxScale = try word(auxHeader,12), movieScale = try word(movieHeader,12)
        guard let step = positive.min(), mainScale > 0, auxScale > 0, movieScale > 0 else { throw Failure("无法从主轨确定有效帧长") }
        let seconds = Double(step) / Double(mainScale)
        let auxValue = ceil(seconds * Double(auxScale)), movieValue = ceil(seconds * Double(movieScale))
        guard auxValue.isFinite, movieValue.isFinite, auxValue <= Double(UInt32.max), movieValue <= Double(UInt32.max) else { throw Failure("修复辅助轨时长超出容器范围") }
        let auxTicks = max(1,Int(auxValue)), movieTicks = max(1,Int(movieValue))
        var repaired = data
        try Binary.put(auxTicks,into:&repaired,at:times.payload + 12)
        try Binary.put(auxTicks,into:&repaired,at:auxHeader.payload + 16)
        try Binary.put(movieTicks,into:&repaired,at:trackHeader.payload + 20)
        return (repaired,["method":"primary-shortest-positive-frame-duration","original_duration_seconds":0,
            "repaired_duration_seconds":Double(auxTicks) / Double(auxScale),"sample_pts_unchanged":true,
            "original_sample_sha256":sha(Data(data[offset..<offset + size]))])
    }
    static func liveStill(_ original: [UInt8], uuid: String, updateIdentifier: Bool = true) throws -> [UInt8] {
        #if MOTIONLIVE_SWIFT_REFERENCE
        if PhotoHEIF.matches(original) { return try PhotoHEIF.liveStill(original,uuid:uuid) }
        let data = try MLJPEG.staticOnly(original)
        var replacements: [Int:[UInt8]] = [:], insertion: [UInt8] = []
        let exifs = try MLJPEG.segments(data).filter { $0.marker == 225 && $0.payload.starts(with: MLJPEG.exif) }
        guard exifs.count <= 1 else { throw Failure("JPEG 包含多个 EXIF 数据段") }
        if updateIdentifier {
            if let s = exifs.first { replacements[s.start] = try MLJPEG.app(225, MLJPEG.exifIdentifier(s.payload, uuid: uuid)) }
            else { insertion = try MLJPEG.app(225, MLJPEG.exifIdentifier(nil, uuid: uuid)) }
        }
        let keys: Set<String> = ["MotionPhoto", "MotionPhotoVersion", "MotionPhotoPresentationTimestampUs", "MicroVideo", "MicroVideoVersion", "MicroVideoOffset", "MicroVideoPresentationTimestampUs", "MediaDataOffset"]
        let vendorKeys: Set<String> = ["MotionPhotoPrimaryPresentationTimestampUs","MotionPhotoOwner","OLivePhotoVersion","VideoLength","MediaDataOffset","VMotionPhotoVersion","VMotionPhotoSource","VMediaKitVersion"]
        func isMotion(_ node: XMLNode) -> Bool {
            let uri = MLXMP.uri(node), name = node.localName ?? ""
            return uri == MLXMP.camera && keys.contains(name) || [VendorMotion.oppo,VendorMotion.vivo].contains(uri) && vendorKeys.contains(name)
        }
        let packets = try MLXMP.packets(data)
        var changed: Set<Int> = []
        for (i,packet) in packets.enumerated() {
            for element in MLXMP.all(packet.document).reversed() {
                for attr in element.attributes ?? [] where isMotion(attr) { attr.detach(); changed.insert(i) }
                if isMotion(element) { element.detach(); changed.insert(i) }
                else if MLXMP.uri(element) == MLXMP.rdf && element.localName == "li",
                        MLXMP.values(element, MLXMP.item, "Semantic").contains("MotionPhoto") { element.detach(); changed.insert(i) }
            }
        }
        replacements.merge(try MLXMP.rewrites(packets,changed:changed)) { _,new in new }
        let result = try MLJPEG.edit(data, replacements: replacements, insert: insertion)
        let before = try MLJPEG.auxiliary(data).dropFirst().map { sha(Data($0)) }
        let after = try MLJPEG.auxiliary(result).dropFirst().map { sha(Data($0)) }
        guard before == after else { throw Failure("增益图字节校验失败") }
        return result
        #else
        // HEIC input always takes the HEIC path inside (identifier always
        // written), matching the old PhotoHEIF.liveStill behavior.
        return try RustCore.liveStill(original, uuid: uuid, updateIdentifier: updateIdentifier)
        #endif
    }
}
