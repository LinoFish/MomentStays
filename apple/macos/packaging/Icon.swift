import AppKit

// Motion-Live icon (minimal Live Photo rings). Matches docs/branding/icon-light.svg and icon-dark.svg (icon.svg follows the system appearance).
// Usage: make-icon <output.iconset> [light|dark]
// Coordinates use the SVG's top-left origin on a 1024 canvas; angles are SVG angles (y down).
func color(_ rgb: UInt32, alpha: CGFloat = 1) -> NSColor {
    NSColor(srgbRed: CGFloat((rgb >> 16) & 255) / 255,
            green: CGFloat((rgb >> 8) & 255) / 255,
            blue: CGFloat(rgb & 255) / 255, alpha: alpha)
}

struct Palette { let top, bottom, ink, core: UInt32; let edge: NSColor }
let dark = CommandLine.arguments.count > 2 && CommandLine.arguments[2] == "dark"
let p = dark
    ? Palette(top: 0x2A3442, bottom: 0x141A22, ink: 0xF2F4F5, core: 0xF4B860, edge: color(0xFFFFFF, alpha: 0.08))
    : Palette(top: 0xFFFFFF, bottom: 0xECEEF1, ink: 0x1F2933, core: 0xF0A43C, edge: color(0x000000, alpha: 0.10))

let c = NSPoint(x: 512, y: 512)
let R: CGFloat = 250
func onRing(_ deg: CGFloat) -> NSPoint {
    let a = deg * .pi / 180
    return NSPoint(x: c.x + R * cos(a), y: c.y + R * sin(a))
}

func draw() {
    // Tile: macOS 824 grid
    let tileRect = NSRect(x: 100, y: 100, width: 824, height: 824)
    let tile = NSBezierPath(roundedRect: tileRect, xRadius: 185, yRadius: 185)
    NSGraphicsContext.saveGraphicsState()
    tile.addClip()
    NSGradient(starting: color(p.top), ending: color(p.bottom))!.draw(in: tileRect, angle: 90)
    NSGraphicsContext.restoreGraphicsState()
    p.edge.setStroke()
    let edge = NSBezierPath(roundedRect: tileRect.insetBy(dx: 0.5, dy: 0.5), xRadius: 184.5, yRadius: 184.5)
    edge.lineWidth = 1.5
    edge.stroke()

    // Outer dotted ring: 36 positions, the last quarter (9 o'clock -> 12) becomes a motion trail.
    color(p.ink).setFill()
    for i in 0..<27 {
        let pt = onRing(-90 + CGFloat(i) * 10)
        NSBezierPath(ovalIn: NSRect(x: pt.x - 11, y: pt.y - 11, width: 22, height: 22)).fill()
    }
    let start: CGFloat = 180, end: CGFloat = 261, steps = 80
    for s in 0..<steps {
        let t0 = CGFloat(s) / CGFloat(steps), t1 = CGFloat(s + 1) / CGFloat(steps)
        let seg = NSBezierPath()
        seg.move(to: onRing(start + (end - start) * t0))
        seg.line(to: onRing(start + (end - start) * t1))
        seg.lineWidth = 22
        seg.lineCapStyle = .butt
        color(p.ink, alpha: t1).setStroke()
        seg.stroke()
    }
    let head = onRing(end)
    color(p.ink).setFill()
    NSBezierPath(ovalIn: NSRect(x: head.x - 11, y: head.y - 11, width: 22, height: 22)).fill()

    // Inner ring and the moment that stays.
    color(p.ink).setStroke()
    let ring = NSBezierPath(ovalIn: NSRect(x: c.x - 150, y: c.y - 150, width: 300, height: 300))
    ring.lineWidth = 24
    ring.stroke()
    color(p.core).setFill()
    NSBezierPath(ovalIn: NSRect(x: c.x - 58, y: c.y - 58, width: 116, height: 116)).fill()
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
        draw()
        NSGraphicsContext.restoreGraphicsState()
        let suffix = scale == 2 ? "@2x" : ""
        try bitmap.representation(using: .png, properties: [:])!.write(
            to: folder.appendingPathComponent("icon_\(size)x\(size)\(suffix).png"))
    }
}
