import Foundation
import ImageIO
import CoreGraphics
import CryptoKit

for path in CommandLine.arguments.dropFirst() {
    guard let source = CGImageSourceCreateWithURL(URL(fileURLWithPath:path) as CFURL,nil),
          let image = CGImageSourceCreateImageAtIndex(source,0,[kCGImageSourceShouldCacheImmediately:true] as CFDictionary),
          let properties = CGImageSourceCopyPropertiesAtIndex(source,0,nil) as? [String:Any] else { fatalError("ImageIO cannot decode \(path)") }
    let width = image.width, height = image.height
    var bytes = [UInt8](repeating:0,count:width * height * 4)
    let ok = bytes.withUnsafeMutableBytes { raw -> Bool in
        guard let context = CGContext(data:raw.baseAddress,width:width,height:height,bitsPerComponent:8,
            bytesPerRow:width * 4,space:CGColorSpace(name:CGColorSpace.sRGB)!,bitmapInfo:CGImageAlphaInfo.premultipliedLast.rawValue) else { return false }
        context.draw(image,in:CGRect(x:0,y:0,width:width,height:height)); return true
    }
    guard ok else { fatalError("Cannot render decoded image") }
    let maker = properties[kCGImagePropertyMakerAppleDictionary as String] as? [String:Any]
    let result: [String:Any] = ["path":path,"width":width,"height":height,"bits":image.bitsPerComponent,
        "orientation":properties[kCGImagePropertyOrientation as String] ?? NSNull(),
        "identifier":maker?["17"] ?? NSNull(),
        "pixel_sha256":SHA256.hash(data:Data(bytes)).map { String(format:"%02x",$0) }.joined()]
    let data = try JSONSerialization.data(withJSONObject:result,options:[.sortedKeys]); print(String(decoding:data,as:UTF8.self))
}
