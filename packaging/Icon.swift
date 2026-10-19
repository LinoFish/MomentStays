import AppKit

let folder = URL(fileURLWithPath:CommandLine.arguments[1])
try FileManager.default.createDirectory(at:folder,withIntermediateDirectories:true)
for size in [16,32,128,256,512] {
    for scale in [1,2] {
        let px = size * scale
        let bitmap = NSBitmapImageRep(bitmapDataPlanes:nil,pixelsWide:px,pixelsHigh:px,bitsPerSample:8,samplesPerPixel:4,hasAlpha:true,isPlanar:false,colorSpaceName:.deviceRGB,bytesPerRow:0,bitsPerPixel:0)!
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep:bitmap)
        let factor = CGFloat(px) / 1024
        let transform = NSAffineTransform(); transform.scale(by:factor); transform.concat()
        let outer = NSBezierPath(roundedRect:NSRect(x:66,y:66,width:892,height:892),xRadius:206,yRadius:206)
        NSGradient(starting:NSColor(calibratedRed:0.20,green:0.20,blue:0.62,alpha:1),ending:NSColor(calibratedRed:0.63,green:0.33,blue:0.95,alpha:1))!.draw(in:outer,angle:55)
        NSColor.white.withAlphaComponent(0.17).setStroke()
        let rim = NSBezierPath(ovalIn:NSRect(x:199,y:199,width:626,height:626)); rim.lineWidth = 28; rim.stroke()
        NSColor.white.withAlphaComponent(0.56).setStroke()
        let mid = NSBezierPath(ovalIn:NSRect(x:277,y:277,width:470,height:470)); mid.lineWidth = 25; mid.stroke()
        NSColor.white.setStroke()
        let inner = NSBezierPath(ovalIn:NSRect(x:350,y:350,width:324,height:324)); inner.lineWidth = 22; inner.stroke()
        let play = NSBezierPath(); play.move(to:NSPoint(x:474,y:420)); play.line(to:NSPoint(x:474,y:604)); play.line(to:NSPoint(x:618,y:512)); play.close()
        NSColor.white.setFill(); play.fill()
        NSGraphicsContext.restoreGraphicsState()
        let suffix = scale == 2 ? "@2x" : ""
        try bitmap.representation(using:.png,properties:[:])!.write(to:folder.appendingPathComponent("icon_\(size)x\(size)\(suffix).png"))
    }
}
