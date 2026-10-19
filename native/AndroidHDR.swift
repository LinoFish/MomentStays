import Foundation
import ImageIO
import CoreGraphics
import CoreVideo

// Monterey supports extracting legacy Apple HDR auxiliary pixels. Newer ISO /
// tmap variants are retained in HEIC, never guessed into a legacy gain model.
enum AndroidHDR {
    static func jpeg(_ image:CGImage, quality:Double, properties:[String:Any] = [:]) throws -> [UInt8] {
        let bytes = NSMutableData()
        guard let dest = CGImageDestinationCreateWithData(bytes,"public.jpeg" as CFString,1,nil) else { throw Failure("无法创建 JPEG 编码器") }
        var values = properties; values[kCGImageDestinationLossyCompressionQuality as String] = quality
        CGImageDestinationAddImage(dest,image,values as CFDictionary)
        guard CGImageDestinationFinalize(dest) else { throw Failure("JPEG 编码失败") }
        return Array(bytes as Data)
    }
    static func stops(_ properties:[String:Any]) -> Double? {
        guard let maker = properties[kCGImagePropertyMakerAppleDictionary as String] as? [String:Any],
              let a = (maker["33"] as? NSNumber)?.doubleValue, let b = (maker["48"] as? NSNumber)?.doubleValue,
              a.isFinite, b.isFinite else { return nil }
        let value = max(a < 1 ? (b <= 0.01 ? -20*b + 1.8 : -0.101*b + 1.601) : (b <= 0.01 ? -70*b + 3 : -0.303*b + 2.303),0)
        return value > 0 && value <= 16 ? value : nil
    }
    static func prepare(_ url:URL, options:AndroidOptions) throws -> ([UInt8],[String:Any]) {
        guard options.imageQuality > 0 && options.imageQuality <= 1 && options.gainQuality > 0 && options.gainQuality <= 1 else { throw Failure("JPEG 质量需在 1–100% 之间") }
        let original = Array(try Data(contentsOf:url)), heic = PhotoHEIF.matches(original)
        if options.container == "heic" {
            guard heic else { throw Failure("保留原编码的 HEIC 输出需要 HEIC 原图；未将 JPEG 再编码成 HEIC") }
            _ = try PhotoHEIF.layout(original)
            return (original,["status":"preserved","input":"heic","reason":"HEIC 全部原始图像/增益图编码及属性保留；Apple 私有 HDR 的安卓渲染兼容性未保证"])
        }
        guard let source = CGImageSourceCreateWithURL(url as CFURL,nil),
              let properties = CGImageSourceCopyPropertiesAtIndex(source,0,nil) as? [String:Any] else { throw Failure("无法解码原始静态图") }
        var base: [UInt8]
        if !heic && options.preserveJPEG { base = try MLJPEG.staticOnly(original) }
        else {
            guard let image = CGImageSourceCreateImageAtIndex(source,0,nil) else { throw Failure("ImageIO 无法解码静态图") }
            base = try jpeg(image,quality:options.imageQuality,properties:properties)
        }
        if !heic {
            let maps = try MLJPEG.auxiliary(original)
            let ultra = try MLXMP.packets(original).contains { !MLXMP.values($0.document,MLXMP.hdr,"Version").isEmpty } || maps.dropFirst().contains { image in
                ((try? MLXMP.packets(image)) ?? []).contains { !MLXMP.values($0.document,MLXMP.hdr,"Version").isEmpty }
            }
            if ultra && options.preserveJPEG { return (base,["status":"preserved","input":"ultra-hdr","reason":"已有 Ultra HDR 原始 JPEG 和增益图保留；质量设置仅在重新编码时生效"]) }
            if ultra && !options.preserveJPEG {
                guard maps.count == 2 else { throw Failure("Ultra HDR 增益图布局不受支持") }
                // Keep calibrated map bytes and metadata; only the SDR base is re-encoded.
                base = try PhotoHDR.packGainMap(base,gain:maps[1])
                return (base,["status":"preserved-gainmap","reason":"主图按指定质量重编码，已有 Ultra HDR 增益图与参数保留"])
            }
            let (legacy,_) = try PhotoHDR.legacy(original)
            if let legacy = legacy {
                let temp = FileManager.default.temporaryDirectory.appendingPathComponent("motionlive-apple-gain-" + UUID().uuidString + ".jpg")
                defer { try? FileManager.default.removeItem(at:temp) }
                try Data(legacy.image).write(to:temp)
                guard let mapSource = CGImageSourceCreateWithURL(temp as CFURL,nil), let image = CGImageSourceCreateImageAtIndex(mapSource,0,nil) else { throw Failure("无法解码 Apple JPEG 增益图") }
                return (try convert(base:base,image:image,stops:legacy.stops,quality:options.gainQuality),["status":"converted","input":"apple-hdr","output":"ultra-hdr","reason":"已有 Apple 增益图按 Rec.709 模型转为 Ultra HDR"])
            }
            guard maps.count <= 1 else { throw Failure("原始 JPEG 含未知辅助图/HDR 参数，未丢弃或推测") }
            return (base,["status":"sdr","reason":"SDR 不生成增益图"])
        }
        let auxiliary = CGImageSourceCopyAuxiliaryDataInfoAtIndex(source,0,kCGImageAuxiliaryDataTypeHDRGainMap) as? [String:Any]
        guard let auxiliary = auxiliary else {
            let layout = try PhotoHEIF.layout(original)
            let props = layout.children.first { $0.type == "iprp" }
            let marker = props.map { String(decoding:original[$0.payload..<$0.end],as:UTF8.self) } ?? ""
            guard !marker.contains("hdrgainmap"), !layout.items.contains(where: { $0.type == "tmap" }), !highDynamicRange(original,layout:layout) else { throw Failure("此 HEIC HDR 不能在 macOS 12 安全提取；请保留 HEIC 原编码，或提供支持的 Apple HDR 增益图") }
            return (base,["status":"sdr","reason":"未检测到可恢复 HDR；不生成增益图"])
        }
        guard let metadataValue = auxiliary[kCGImageAuxiliaryDataInfoMetadata as String],
              CFGetTypeID(metadataValue as CFTypeRef) == CGImageMetadataGetTypeID() else { throw Failure("HEIC 增益图缺少可验证的 Apple HDR 版本；请保留原编码") }
        let metadata = metadataValue as! CGImageMetadata
        let tags = CGImageMetadataCopyTags(metadata) as? [CGImageMetadataTag] ?? []
        let versions = tags.filter { (CGImageMetadataTagCopyNamespace($0) as String?) == MLXMP.apple && (CGImageMetadataTagCopyName($0) as String?) == "HDRGainMapVersion" }.compactMap { CGImageMetadataTagCopyValue($0).map { String(describing:$0) } }
        guard versions == ["65536"] || versions == ["131072"] else { throw Failure("此 HEIC 增益图不是支持的 Apple HDR；请保留 HEIC 原编码") }
        let headrooms = tags.filter { (CGImageMetadataTagCopyNamespace($0) as String?) == MLXMP.apple && (CGImageMetadataTagCopyName($0) as String?) == "HDRGainMapHeadroom" }.compactMap { CGImageMetadataTagCopyValue($0).flatMap { Double(String(describing:$0)) } }
        // Monterey may expose a legacy HEIC map as 131072 with a computed
        // headroom. Accept only when it agrees with the documented Maker model.
        if versions == ["131072"] {
            guard headrooms.count == 1, headrooms[0].isFinite, headrooms[0] > 1,
                  let makerStops = stops(properties), abs(log2(headrooms[0]) - makerStops) < 0.002,
                  !(try PhotoHEIF.layout(original)).items.contains(where: { $0.type == "tmap" }) else { throw Failure("新版 Apple HDR 参数不能验证为已支持模型；请保留 HEIC 原编码") }
        }
        guard let stops = stops(properties), let desc = auxiliary[kCGImageAuxiliaryDataInfoDataDescription as String] as? [String:Any],
              let raw = auxiliary[kCGImageAuxiliaryDataInfoData as String] as? Data,
              let width = (desc["Width"] as? NSNumber)?.intValue, let height = (desc["Height"] as? NSNumber)?.intValue,
              let stride = (desc["BytesPerRow"] as? NSNumber)?.intValue,
              (desc["PixelFormat"] as? NSNumber)?.uint32Value == kCVPixelFormatType_OneComponent8,
              width > 0, height > 0, width <= 16384, height <= 16384, stride >= width, raw.count / stride >= height,
              let provider = CGDataProvider(data:raw as CFData),
              let image = CGImage(width:width,height:height,bitsPerComponent:8,bitsPerPixel:8,bytesPerRow:stride,space:CGColorSpaceCreateDeviceGray(),bitmapInfo:[],provider:provider,decode:nil,shouldInterpolate:false,intent:.defaultIntent) else { throw Failure("Apple HEIC HDR 增益图/亮度参数不完整，未降为 SDR 或猜测 HDR") }
        return (try convert(base:base,image:image,stops:stops,quality:options.gainQuality),["status":"converted","input":"apple-hdr","output":"ultra-hdr","reason":"Apple HEIC 原始增益图转 Ultra HDR；主 JPEG 和增益 JPEG 使用独立质量"])
    }
    static func highDynamicRange(_ bytes:[UInt8],layout:PhotoHEIF.Layout) -> Bool {
        guard let properties = layout.children.first(where: { $0.type == "iprp" }),
              let boxes = try? MotionVideoMetadata.boxes(bytes,properties.payload,properties.end),
              let ipco = boxes.first(where: { $0.type == "ipco" }),
              let values = try? MotionVideoMetadata.boxes(bytes,ipco.payload,ipco.end) else { return true }
        for value in values {
            // Conservative: a 10-bit stream may contain HDR even without a map.
            if value.type == "hvcC", value.end - value.payload >= 19, bytes[value.payload + 17] & 7 > 0 { return true }
            if value.type == "colr", value.end - value.payload >= 10,
               String(decoding:bytes[value.payload..<value.payload + 4],as:UTF8.self) == "nclx",
               let transfer = try? Binary.int(bytes,value.payload + 6,2), [16,18].contains(transfer) { return true }
        }
        return false
    }
    static func convert(base:[UInt8],image:CGImage,stops:Double,quality:Double) throws -> [UInt8] {
        guard image.bitsPerComponent == 8, image.bitsPerPixel == 8, let raw = image.dataProvider?.data as Data? else { throw Failure("Apple HDR 需要 8 位单通道增益图") }
        let width = image.width, height = image.height, stride = image.bytesPerRow
        guard raw.count / stride >= height, stride >= width else { throw Failure("增益图数据被截断") }
        let headroom = pow(2,stops)
        let lookup: [UInt8] = (0...255).map { i in
            let v = Double(i) / 255, linear = v < 0.081 ? v / 4.5 : pow((v + 0.099) / 1.099,1 / 0.45)
            return UInt8(max(0,min(255,(log2(1 + (headroom - 1) * linear) / stops * 255).rounded())))
        }
        var pixels = [UInt8](repeating:0,count:width * height)
        for y in 0..<height { for x in 0..<width { pixels[y * width + x] = lookup[Int(raw[y * stride + x])] } }
        guard let provider = CGDataProvider(data:Data(pixels) as CFData), let gain = CGImage(width:width,height:height,bitsPerComponent:8,bitsPerPixel:8,bytesPerRow:width,space:CGColorSpaceCreateDeviceGray(),bitmapInfo:[],provider:provider,decode:nil,shouldInterpolate:false,intent:.defaultIntent) else { throw Failure("无法生成增益图") }
        return try PhotoHDR.ultra(base,gainJPEG:jpeg(gain,quality:quality),stops:stops)
    }
}
