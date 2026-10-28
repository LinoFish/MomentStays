import AppKit

// Text-free Finder background; the installer uses native icons and one arrow.
let width = 720, height = 300, scale = 2
guard CommandLine.arguments.count == 2,
      let bitmap = NSBitmapImageRep(bitmapDataPlanes:nil,pixelsWide:width * scale,pixelsHigh:height * scale,
        bitsPerSample:8,samplesPerPixel:4,hasAlpha:true,isPlanar:false,colorSpaceName:.deviceRGB,bytesPerRow:0,bitsPerPixel:0),
      let context = NSGraphicsContext(bitmapImageRep:bitmap) else { fatalError("Usage: dmg-background output.png") }

func color(_ hex: UInt32) -> NSColor {
    NSColor(srgbRed:CGFloat((hex >> 16) & 255) / 255,green:CGFloat((hex >> 8) & 255) / 255,
        blue:CGFloat(hex & 255) / 255,alpha:1)
}
let amber = color(0xC47E13)
NSGraphicsContext.saveGraphicsState()
context.cgContext.translateBy(x:0,y:CGFloat(height * scale))
context.cgContext.scaleBy(x:CGFloat(scale),y:-CGFloat(scale))
NSGraphicsContext.current = NSGraphicsContext(cgContext:context.cgContext,flipped:true)
color(0xF4F6F8).setFill(); NSRect(x:0,y:0,width:width,height:height).fill()
let arrow = NSBezierPath(); arrow.move(to:NSPoint(x:309,y:155)); arrow.line(to:NSPoint(x:411,y:155))
arrow.move(to:NSPoint(x:399,y:144)); arrow.line(to:NSPoint(x:411,y:155)); arrow.line(to:NSPoint(x:399,y:166))
arrow.lineWidth = 2.5; arrow.lineCapStyle = .round; arrow.lineJoinStyle = .round
amber.setStroke(); arrow.stroke()
NSGraphicsContext.restoreGraphicsState()
bitmap.size = NSSize(width:width,height:height)
guard let data = bitmap.representation(using:.png,properties:[:]) else { fatalError("Cannot encode background") }
try data.write(to:URL(fileURLWithPath:CommandLine.arguments[1]))
