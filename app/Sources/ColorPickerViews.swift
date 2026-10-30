import AppKit

/// The Color Picker sheet's three drawn controls. They are
/// display chrome: every gradient is drawn in `ColorProfile.sRGB`, because
/// the colour they show is an authored sRGB colour, not document pixels.

/// A gradient in the app's sRGB from encoded RGB stops.
private func srgbGradient(_ stops: [(r: Double, g: Double, b: Double, a: Double)],
                          locations: [CGFloat]) -> CGGradient?
{
    let colors = stops.compactMap {
        CGColor(colorSpace: ColorProfile.sRGB,
                components: [CGFloat($0.r), CGFloat($0.g), CGFloat($0.b), CGFloat($0.a)])
    }
    guard colors.count == stops.count else { return nil }
    return CGGradient(colorsSpace: ColorProfile.sRGB, colors: colors as CFArray,
                      locations: locations)
}

/// The saturation/brightness square for one hue: x is saturation,
/// y is brightness (bottom black). A white → pure-hue horizontal gradient
/// under a clear → black vertical one, and a ring at the current position.
final class SaturationBrightnessField: NSView {
    static let side: CGFloat = 256

    var hue: Double = 0 { didSet { needsDisplay = true } }
    var saturation: Double = 0 { didSet { needsDisplay = true } }
    var brightness: Double = 0 { didSet { needsDisplay = true } }
    /// Mouse down / drag, with the new saturation and brightness.
    var onChange: ((Double, Double) -> Void)?

    init() {
        super.init(frame: NSRect(x: 0, y: 0, width: Self.side, height: Self.side))
        translatesAutoresizingMaskIntoConstraints = false
        widthAnchor.constraint(equalToConstant: Self.side).isActive = true
        heightAnchor.constraint(equalToConstant: Self.side).isActive = true
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("SaturationBrightnessField does not support NSCoder")
    }

    override func draw(_ dirtyRect: NSRect) {
        guard let context = NSGraphicsContext.current?.cgContext else { return }
        let pure = Hexcone.rgb(h: hue, s: 1, b: 1)
        if let across = srgbGradient([(1, 1, 1, 1), (pure.r, pure.g, pure.b, 1)],
                                     locations: [0, 1]) {
            context.drawLinearGradient(
                across, start: CGPoint(x: bounds.minX, y: 0), end: CGPoint(x: bounds.maxX, y: 0),
                options: [])
        }
        if let down = srgbGradient([(0, 0, 0, 1), (0, 0, 0, 0)], locations: [0, 1]) {
            context.drawLinearGradient(
                down, start: CGPoint(x: 0, y: bounds.minY), end: CGPoint(x: 0, y: bounds.maxY),
                options: [])
        }
        DS.border.setStroke()
        NSBezierPath(rect: bounds.insetBy(dx: 0.5, dy: 0.5)).stroke()

        // The marker must read over any colour beneath it, so it is a white
        // ring inside a black one rather than a theme colour.
        let center = NSPoint(x: bounds.minX + CGFloat(saturation) * bounds.width,
                             y: bounds.minY + CGFloat(brightness) * bounds.height)
        let outer = NSBezierPath(ovalIn: NSRect(x: center.x - 6, y: center.y - 6,
                                                width: 12, height: 12))
        outer.lineWidth = 1
        NSColor.black.setStroke()
        outer.stroke()
        let inner = NSBezierPath(ovalIn: NSRect(x: center.x - 5, y: center.y - 5,
                                                width: 10, height: 10))
        inner.lineWidth = 1.5
        NSColor.white.setStroke()
        inner.stroke()
    }

    override func mouseDown(with event: NSEvent) { track(event) }

    override func mouseDragged(with event: NSEvent) { track(event) }

    private func track(_ event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        let s = Double(min(max((point.x - bounds.minX) / bounds.width, 0), 1))
        let b = Double(min(max((point.y - bounds.minY) / bounds.height, 0), 1))
        saturation = s
        brightness = b
        onChange?(s, b)
    }
}

/// The vertical hue strip beside the field: 0° at the bottom to 360° at the
/// top, with a triangle on each side at the current hue.
final class HueStrip: NSView {
    static let size = NSSize(width: 20, height: 256)
    /// Room on each side for the triangle markers.
    private static let margin: CGFloat = 5

    var hue: Double = 0 { didSet { needsDisplay = true } }
    /// Mouse down / drag, with the new hue in 0…360.
    var onChange: ((Double) -> Void)?

    init() {
        super.init(frame: NSRect(origin: .zero, size: Self.size))
        translatesAutoresizingMaskIntoConstraints = false
        widthAnchor.constraint(equalToConstant: Self.size.width).isActive = true
        heightAnchor.constraint(equalToConstant: Self.size.height).isActive = true
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("HueStrip does not support NSCoder")
    }

    private var strip: NSRect { bounds.insetBy(dx: Self.margin, dy: 0) }

    override func draw(_ dirtyRect: NSRect) {
        guard let context = NSGraphicsContext.current?.cgContext else { return }
        let degrees: [Double] = [0, 60, 120, 180, 240, 300, 360]
        let stops = degrees.map { degree -> (r: Double, g: Double, b: Double, a: Double) in
            let rgb = Hexcone.rgb(h: degree, s: 1, b: 1)
            return (rgb.r, rgb.g, rgb.b, 1)
        }
        context.saveGState()
        context.clip(to: strip)
        if let gradient = srgbGradient(stops, locations: degrees.map { CGFloat($0 / 360) }) {
            context.drawLinearGradient(
                gradient, start: CGPoint(x: 0, y: strip.minY), end: CGPoint(x: 0, y: strip.maxY),
                options: [])
        }
        context.restoreGState()
        DS.border.setStroke()
        NSBezierPath(rect: strip.insetBy(dx: 0.5, dy: 0.5)).stroke()

        let y = strip.minY + CGFloat(min(max(hue, 0), 360) / 360) * strip.height
        let half = Self.margin - 1
        let left = NSBezierPath()
        left.move(to: NSPoint(x: bounds.minX, y: y - half))
        left.line(to: NSPoint(x: strip.minX, y: y))
        left.line(to: NSPoint(x: bounds.minX, y: y + half))
        left.close()
        let right = NSBezierPath()
        right.move(to: NSPoint(x: bounds.maxX, y: y - half))
        right.line(to: NSPoint(x: strip.maxX, y: y))
        right.line(to: NSPoint(x: bounds.maxX, y: y + half))
        right.close()
        DS.textStrong.setFill()
        left.fill()
        right.fill()
    }

    override func mouseDown(with event: NSEvent) { track(event) }

    override func mouseDragged(with event: NSEvent) { track(event) }

    private func track(_ event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        let h = Double(min(max((point.y - strip.minY) / strip.height, 0), 1)) * 360
        hue = h
        onChange?(h)
    }
}

/// The new/current pair: the colour being picked on top, the colour
/// the sheet opened with below, each over a checkerboard so alpha shows.
final class ColorCompareSwatch: NSView {
    static let size = NSSize(width: 64, height: 72)

    var newColor: NSColor { didSet { needsDisplay = true } }
    let currentColor: NSColor

    init(newColor: NSColor, currentColor: NSColor) {
        self.newColor = newColor
        self.currentColor = currentColor
        super.init(frame: NSRect(origin: .zero, size: Self.size))
        translatesAutoresizingMaskIntoConstraints = false
        widthAnchor.constraint(equalToConstant: Self.size.width).isActive = true
        heightAnchor.constraint(equalToConstant: Self.size.height).isActive = true
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("ColorCompareSwatch does not support NSCoder")
    }

    override func draw(_ dirtyRect: NSRect) {
        let half = bounds.height / 2
        let top = NSRect(x: bounds.minX, y: bounds.minY + half, width: bounds.width, height: half)
        let bottom = NSRect(x: bounds.minX, y: bounds.minY, width: bounds.width, height: half)
        drawChecker(in: bounds)
        newColor.setFill()
        top.fill(using: .sourceOver)
        currentColor.setFill()
        bottom.fill(using: .sourceOver)
        DS.border.setStroke()
        NSBezierPath(rect: bounds.insetBy(dx: 0.5, dy: 0.5)).stroke()
    }

    private func drawChecker(in rect: NSRect) {
        let cell: CGFloat = 8
        DS.checkerA.setFill()
        rect.fill()
        DS.checkerB.setFill()
        var row = 0
        var y = rect.minY
        while y < rect.maxY {
            var x = rect.minX + (row % 2 == 0 ? 0 : cell)
            while x < rect.maxX {
                NSRect(x: x, y: y, width: min(cell, rect.maxX - x), height: min(cell, rect.maxY - y))
                    .fill()
                x += cell * 2
            }
            y += cell
            row += 1
        }
    }
}
