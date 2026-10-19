import Foundation

struct LegacyMap {
    let image: [UInt8]
    let stops: Double
}

enum PhotoHDR {
    // Honor's legacy EHDR is not an Adobe/ISO gain map. Preserve the bounded
    // static extension verbatim; its rendering curve has no verified decoder.
    // Offsets below describe only the observed ExtInfo v1 / 128-byte record.
    static func honorPrivate(_ data: [UInt8], imageEnd: Int) throws -> [UInt8]? {
        let footer = Binary.string("HiHonor_ExtInfo") + [UInt8](repeating:0,count:5)
        guard data.count >= footer.count, Array(data.suffix(footer.count)) == footer else { return nil }
        let count = data.count - imageEnd
        guard imageEnd >= 0, count >= 193, count <= 1_048_576 else { throw Failure("荣耀私有 HDR 静态扩展范围无效，未丢弃原数据") }
        let tail = Array(data[imageEnd...]), name = count - 45, sizeAt = name - 4
        guard Array(tail[name..<name + 20]) == Binary.string("HiHonor_EhdrEn") + [UInt8](repeating:0,count:6),
              try Binary.int(tail,count - 24,4,little:true) == 1,
              tail[count - 25] == 0,
              try Binary.int(tail,sizeAt,4,little:true) == 132,
              try Binary.int(tail,sizeAt - 4,4,little:true) == 128 else { throw Failure("荣耀私有 HDR 扩展版本尚不支持，未丢弃原数据") }
        let payload = sizeAt - 132, prefixSize = payload - 12
        guard prefixSize >= 4,
              Array(tail[payload - 8..<payload]) == Binary.string("CDRDATA\0"),
              try Binary.int(tail,prefixSize,4,little:true) == prefixSize,
              try Binary.int(tail,0,4,little:true) <= prefixSize else { throw Failure("荣耀私有 HDR 扩展长度无效，未丢弃原数据") }
        return tail
    }
    static func honorPrivate(_ data: [UInt8]) throws -> [UInt8]? {
        if PhotoHEIF.matches(data) { return nil }
        return try honorPrivate(data,imageEnd:MLJPEG.imageRanges(data).map { $0.upperBound }.max()!)
    }
    static let iso = Binary.string("urn:iso:std:iso:ts:21496:-1\0")
    static func hasISO(_ still: [UInt8]) throws -> Bool {
        for image in [still] + (try MLJPEG.auxiliary(still).dropFirst().map { $0 }) {
            if try MLJPEG.segments(image).contains(where: { $0.marker == 226 && $0.payload.starts(with:iso) }) { return true }
        }
        return false
    }
    // Add interoperable Adobe metadata to an existing ISO gain map. Both JPEG
    // codestreams remain untouched. Unsupported/private math is never guessed.
    static func upgradeISO(_ still: [UInt8]) throws -> [UInt8]? {
        let images = try MLJPEG.auxiliary(still)
        guard images.count == 2 else { return nil }
        let packets = try MLJPEG.segments(images[1]).filter { $0.marker == 226 && $0.payload.starts(with:iso) }
        guard packets.count == 1 else { return nil }
        let bytes = Array(packets[0].payload.dropFirst(iso.count))
        guard bytes.count >= 5, try Binary.int(bytes,0,2) == 0 else { throw Failure("ISO HDR 增益图版本尚不支持，原数据已保留") }
        let flags = bytes[4], channels = flags & 128 == 0 ? 1 : 3
        guard flags & 64 != 0, flags & 4 == 0, flags & 0x33 == 0 else { throw Failure("ISO HDR 色彩空间或反向增益不适合无损补充 Ultra HDR 元数据，原格式已保留") }
        var position = 5
        func word(_ signed: Bool = false) throws -> Double {
            let raw = UInt32(try Binary.read(bytes,position,4)); position += 4
            return signed ? Double(Int32(bitPattern:raw)) : Double(raw)
        }
        let common: Double? = flags & 8 != 0 ? try word() : nil
        if let common = common, common == 0 { throw Failure("ISO HDR 参数分母为零") }
        func ratio(_ signed: Bool = false) throws -> Double {
            let n = try word(signed), d = try common ?? word()
            guard d > 0 else { throw Failure("ISO HDR 参数分母为零") }
            return n / d
        }
        let capacityMin = try ratio(), capacityMax = try ratio()
        var values = [[Double]]()
        for _ in 0..<channels { values.append(try [ratio(true),ratio(true),ratio(),ratio(true),ratio(true)]) }
        guard position == bytes.count, capacityMin >= 0, capacityMax > capacityMin, capacityMax <= 16,
              values.allSatisfy({ $0.allSatisfy { $0.isFinite } && $0[0] >= -16 && $0[1] <= 16 && $0[1] >= $0[0] && $0[2] > 0 && $0[2] <= 16 && abs($0[3]) <= 1 && abs($0[4]) <= 1 }) else { throw Failure("ISO HDR 增益参数无效或超出支持范围，原数据已保留") }
        func field(_ name: String, _ values: [Double]) -> String {
            if values.count == 1 { return "<hdrgm:\(name)>\(values[0])</hdrgm:\(name)>" }
            return "<hdrgm:\(name)><rdf:Seq>" + values.map { "<rdf:li>\($0)</rdf:li>" }.joined() + "</rdf:Seq></hdrgm:\(name)>"
        }
        var fields = ""
        for (i,name) in ["GainMapMin","GainMapMax","Gamma","OffsetSDR","OffsetHDR"].enumerated() { fields += field(name,values.map { $0[i] }) }
        let xml = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"\(MLXMP.rdf)\"><rdf:Description xmlns:hdrgm=\"\(MLXMP.hdr)\" hdrgm:Version=\"1.0\" hdrgm:BaseRenditionIsHDR=\"False\" hdrgm:HDRCapacityMin=\"\(capacityMin)\" hdrgm:HDRCapacityMax=\"\(capacityMax)\">\(fields)</rdf:Description></rdf:RDF></x:xmpmeta>"
        let gain = try MLJPEG.edit(images[1],replacements:[:],insert:MLJPEG.app(225,MLXMP.header + Binary.string(xml)))
        let output = try packGainMap(still,gain:gain)
        guard try MLJPEG.codingHash(images[1]) == MLJPEG.codingHash(MLJPEG.auxiliary(output)[1]) else { throw Failure("ISO HDR 增益图压缩数据发生变化") }
        return output
    }
    static func legacy(_ still: [UInt8]) throws -> (LegacyMap?, String) {
        var maps: [[UInt8]] = []
        for image in try MLJPEG.auxiliary(still).dropFirst() {
            for packet in try MLXMP.packets(image) {
                if MLXMP.values(packet.document, MLXMP.pixel, "AuxiliaryImageType").contains("urn:com:apple:photo:2020:aux:hdrgainmap") {
                    guard MLXMP.values(packet.document, MLXMP.apple, "HDRGainMapVersion") == ["65536"] else {
                        return (nil, "旧版增益图版本尚不支持，已保留原始数据")
                    }
                    maps.append(image)
                }
            }
        }
        guard maps.count == 1 else {
            let hasAux = try MLJPEG.auxiliary(still).count > 1
            return (nil, maps.isEmpty ? (hasAux ? "已有辅助图，但缺少可验证的 HDR 亮度参数；原样保留，不推测增益" : "没有可恢复的 HDR 增益数据；SDR 保持原样") : "存在多个旧增益图，未推测其含义")
        }
        for segment in try MLJPEG.segments(still) where segment.marker == 225 && segment.payload.starts(with: MLJPEG.exif) {
            let t = try MLTIFF(Array(segment.payload.dropFirst(6)))
            let (fields, _) = try t.exif()
            guard let f = fields.first(where: { $0.tag == 0x927c }) else { continue }
            let offset = try f.offset(t.little)
            guard f.type == 7, offset + f.count <= t.data.count else { throw Failure("旧 MakerNote 无效") }
            let note = Array(t.data[offset..<offset + f.count])
            guard note.starts(with: Binary.string("Apple iOS\0\0\u{1}")), note.count >= 16 else { continue }
            guard Array(note[12..<14]) == [77,77] || Array(note[12..<14]) == [73,73] else { throw Failure("Apple MakerNote 字节序无效") }
            let little = note[12] == 73
            let (entries, _) = try MLTIFF.directory(note, at: 14, little: little)
            var values: [Int:Double] = [:]
            for f in entries where (f.tag == 33 || f.tag == 48) && f.count == 1 {
                if f.type == 11 {
                    values[f.tag] = Double(Float(bitPattern: UInt32(try Binary.read(f.value, 0, 4, little: little))))
                } else if [5,10,12].contains(f.type) {
                    let at = try f.offset(little)
                    guard at >= 16 + entries.count * 12 + 4, at + 8 <= note.count else { throw Failure("旧 HDR 亮度参数偏移无效") }
                    if f.type == 12 { values[f.tag] = Double(bitPattern: try Binary.read(note, at, 8, little: little)) }
                    else {
                        let num = UInt32(try Binary.read(note, at, 4, little: little))
                        let den = UInt32(try Binary.read(note, at + 4, 4, little: little))
                        guard den != 0 else { throw Failure("旧 HDR 亮度参数分母为零") }
                        values[f.tag] = f.type == 10 ? Double(Int32(bitPattern: num)) / Double(Int32(bitPattern: den)) : Double(num) / Double(den)
                    }
                }
            }
            if let a = values[33], let b = values[48], a.isFinite, b.isFinite {
                let raw: Double
                if a < 1 { raw = b <= 0.01 ? -20*b + 1.8 : -0.101*b + 1.601 }
                else { raw = b <= 0.01 ? -70*b + 3 : -0.303*b + 2.303 }
                let stops = max(raw, 0)
                if stops > 0 && stops <= 16 { return (LegacyMap(image: maps[0], stops: stops), "检测到可恢复的旧 Apple HDR") }
            }
        }
        return (nil, "旧增益图亮度参数不完整，已保留原始数据")
    }
    static func colorProfile(_ still: [UInt8], srgb: [UInt8]) throws -> [UInt8] {
        let segs = try MLJPEG.segments(still)
        if segs.contains(where: { $0.marker == 226 && $0.payload.starts(with: MLJPEG.icc) }) { return still }
        for s in segs where s.marker == 225 && s.payload.starts(with: MLJPEG.exif) {
            let t = try MLTIFF(Array(s.payload.dropFirst(6)))
            let (fields, _) = try t.exif()
            if let f = fields.first(where: { $0.tag == 0xa001 }), f.type == 3, f.count == 1,
               try Binary.int(f.value, 0, 2, little: t.little) == 1 {
                return try MLJPEG.edit(still, replacements: [:], insert: MLJPEG.app(226, MLJPEG.icc + [1,1] + srgb))
            }
        }
        throw Failure("旧 HDR 缺少 ICC 或明确的 sRGB 色彩声明，保留原始格式")
    }
    static func ultra(_ still: [UInt8], gainJPEG: [UInt8], stops: Double) throws -> [UInt8] {
        let gainXML = """
        <x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="\(MLXMP.rdf)"><rdf:Description xmlns:hdrgm="\(MLXMP.hdr)" hdrgm:Version="1.0" hdrgm:GainMapMin="0" hdrgm:GainMapMax="\(stops)" hdrgm:Gamma="1" hdrgm:OffsetSDR="0" hdrgm:OffsetHDR="0" hdrgm:HDRCapacityMin="0" hdrgm:HDRCapacityMax="\(stops)" hdrgm:BaseRenditionIsHDR="False"/></rdf:RDF></x:xmpmeta>
        """
        let gain = Array(gainJPEG[..<2]) + (try MLJPEG.app(225, MLXMP.header + Binary.string(gainXML))) + Array(gainJPEG.dropFirst(2))
        return try packGainMap(still,gain:gain)
    }
    static func packGainMap(_ still: [UInt8], gain: [UInt8]) throws -> [UInt8] {
        let end = try MLJPEG.segments(still).last!.end
        let primary = Array(still[..<end])
        var replacements: [Int:[UInt8]] = [:]
        for s in try MLJPEG.segments(primary) where s.marker == 226 && s.payload.starts(with: MLJPEG.mpf) { replacements[s.start] = [] }
        let newXML = """
        <x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="\(MLXMP.rdf)"><rdf:Description xmlns:rdf="\(MLXMP.rdf)" xmlns:hdrgm="\(MLXMP.hdr)" xmlns:Container="\(MLXMP.container)" xmlns:Item="\(MLXMP.item)" hdrgm:Version="1.0"><Container:Directory><rdf:Seq><rdf:li rdf:parseType="Resource"><Container:Item Item:Semantic="Primary" Item:Mime="image/jpeg" Item:Length="0"/></rdf:li><rdf:li rdf:parseType="Resource"><Container:Item Item:Semantic="GainMap" Item:Mime="image/jpeg" Item:Length="\(gain.count)"/></rdf:li></rdf:Seq></Container:Directory></rdf:Description></rdf:RDF></x:xmpmeta>
        """
        let newDoc = try XMLDocument(xmlString: newXML, options: [.nodeLoadExternalEntitiesNever])
        let packets = try MLXMP.packets(primary)
        var insert: [UInt8] = []
        if let first = packets.first {
            for element in MLXMP.all(first.document).reversed() where element.localName == "Directory" && MLXMP.uri(element) == MLXMP.container { element.detach() }
            guard let rdf = MLXMP.all(first.document).first(where: { $0.localName == "RDF" && MLXMP.uri($0) == MLXMP.rdf }),
                  let description = MLXMP.all(newDoc).first(where: { $0.localName == "Description" && MLXMP.uri($0) == MLXMP.rdf }) else { throw Failure("XMP RDF 结构无效") }
            description.detach(); rdf.addChild(description)
            replacements[first.segment.start] = try MLXMP.segment(first.document)
        } else { insert = try MLXMP.segment(newDoc) }
        let cleaned = try MLJPEG.edit(primary, replacements: replacements, insert: insert)
        var tiff: [UInt8] = [77,77,0,42,0,0,0,8]
        let fields = [IFDField(tag: 0xb000, type: 7, count: 4, value: Binary.string("0100")),
                      IFDField(tag: 0xb001, type: 4, count: 1, value: Binary.bytes(2,4)),
                      IFDField(tag: 0xb002, type: 7, count: 32, value: Binary.bytes(50,4))]
        tiff += MLTIFF.serialize(fields, little: false)
        let finalSize = cleaned.count + 90
        for (flags,size,offset) in [(0x20030000, finalSize, 0), (0, gain.count, finalSize - 10)] {
            tiff += Binary.bytes(flags,4) + Binary.bytes(size,4) + Binary.bytes(offset,4) + [0,0,0,0]
        }
        let output = Array(cleaned[..<2]) + (try MLJPEG.app(226, MLJPEG.mpf + tiff)) + Array(cleaned.dropFirst(2)) + gain
        guard try MLJPEG.codingHash(still) == MLJPEG.codingHash(output), try MLJPEG.auxiliary(output).count == 2 else { throw Failure("旧 HDR 转换后的图像校验失败") }
        return output
    }
}
