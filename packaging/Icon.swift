import AppKit

// Matches docs/branding/icon.svg. Coordinates use the SVG's top-left origin.
func color(_ rgb: UInt32, alpha: CGFloat = 1) -> NSColor {
    NSColor(calibratedRed: CGFloat((rgb >> 16) & 255) / 255,
            green: CGFloat((rgb >> 8) & 255) / 255,
            blue: CGFloat(rgb & 255) / 255, alpha: alpha)
}

let folder = URL(fileURLWithPath: CommandLine.arguments[1])
try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
for size in [16, 32, 128, 256, 512] {
    for scale in [1, 2] {
        let px = size * scale
        let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: px, pixelsHigh: px,
            bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
            colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
        NSGraphicsContext.saveGraphicsState()
        let context = NSGraphicsContext(bitmapImageRep: bitmap)!
        NSGraphicsContext.current = context
        context.cgContext.translateBy(x: 0, y: CGFloat(px))
        context.cgContext.scaleBy(x: CGFloat(px) / 1024, y: -CGFloat(px) / 1024)

        color(0x0D1B27).setFill()
        NSBezierPath(roundedRect: NSRect(x: 64, y: 64, width: 896, height: 896),
                     xRadius: 208, yRadius: 208).fill()
        color(0xFFFFFF, alpha: 0.08).setStroke()
        let border = NSBezierPath(roundedRect: NSRect(x: 65, y: 65, width: 894, height: 894),
                                  xRadius: 207, yRadius: 207)
        border.lineWidth = 2
        border.stroke()

        color(0x65DCC5).setStroke()
        let motion = NSBezierPath()
        motion.move(to: NSPoint(x: 512, y: 224))
        motion.appendArc(withCenter: NSPoint(x: 512, y: 512), radius: 288,
                         startAngle: 270, endAngle: 0, clockwise: true)
        motion.lineWidth = 48
        motion.lineCapStyle = .round
        motion.stroke()

        // Playhead dot where the arc begins.
        color(0x65DCC5).setFill()
        NSBezierPath(ovalIn: NSRect(x: 482, y: 194, width: 60, height: 60)).fill()

        color(0xF0F6F5).setStroke()
        let frame = NSBezierPath(roundedRect: NSRect(x: 336, y: 368, width: 352, height: 288),
                                 xRadius: 48, yRadius: 48)
        frame.lineWidth = 28
        frame.stroke()

        color(0x65DCC5, alpha: 0.45).setStroke()
        let timeline = NSBezierPath()
        timeline.move(to: NSPoint(x: 404, y: 512))
        timeline.line(to: NSPoint(x: 620, y: 512))
        timeline.lineWidth = 20
        timeline.lineCapStyle = .round
        timeline.stroke()

        // Soft glow behind the moment that stays (approximates the SVG radial gradient).
        for (r, a) in [(CGFloat(118), CGFloat(0.10)), (CGFloat(84), CGFloat(0.14)), (CGFloat(56), CGFloat(0.20))] {
            color(0xF3BE72, alpha: a).setFill()
            NSBezierPath(ovalIn: NSRect(x: 512 - r, y: 512 - r, width: 2 * r, height: 2 * r)).fill()
        }

        color(0xF3BE72).setFill()
        let moment = NSBezierPath()
        moment.move(to: NSPoint(x: 512, y: 466))
        moment.line(to: NSPoint(x: 558, y: 512))
        moment.line(to: NSPoint(x: 512, y: 558))
        moment.line(to: NSPoint(x: 466, y: 512))
        moment.close()
        moment.fill()

        NSGraphicsContext.restoreGraphicsState()
        let suffix = scale == 2 ? "@2x" : ""
        try bitmap.representation(using: .png, properties: [:])!.write(
            to: folder.appendingPathComponent("icon_\(size)x\(size)\(suffix).png"))
    }
}
