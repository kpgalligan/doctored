import AppKit
import CoreText

// TextKit owns shaping/layout; this file sends resolved outlines and superclass decoration paths to
// Rust.

/// TextKit supplies resolved glyphs, positions and decorations. Intrinsic
/// glyph assets retain their byte color/coverage; authored ink stays continuous.
enum ArtworkTextCapture {
    /// How far a colour or bitmap glyph part's window reaches past its
    /// glyphs' reported bounds, in pixels (`glyphRect`). The text probe
    /// measures how far TextKit's ink reaches past those bounds
    /// (`glyph_box_containment` in `scripts/artwork-text-probe`): at most 1 px,
    /// over Apple Color Emoji from 9 to 512 pt, scaled 0.5 to 3 and sheared.
    /// So the window's outer ring is clear of ink, which is what the capture's
    /// edge guard checks before it keeps a window.
    static let glyphMargin = 2

    /// How capture windows its glyph parts. The app always takes the
    /// default. The text probe narrows `margin` to reach the edge guard, and
    /// sets `colorAsCoverage` to draw colour glyphs through the bitmap-glyph
    /// (A8) branch, which no installed font reaches with ink.
    struct Windowing {
        var margin = ArtworkTextCapture.glyphMargin
        var colorAsCoverage = false
    }

    static func capture(payload: TextLayerPayload, anchor: CGPoint,
                        windowing: Windowing = Windowing()) throws -> NativeArtworkDrawing {
        guard Thread.isMainThread, !BackgroundWork.occupied,
              !payload.string.isEmpty, payload.string.utf16.count <= 4096 else {
            throw DoctoredCoreError(message: "Prepare at most 4096 UTF-16 text units on the main thread after background work finishes.")
        }
        _ = try RasterResources.shared.admit(scratch: TextArtworkLayout.platformAllowance)
        let manager = TextArtworkLayout(windowing: windowing)
        manager.ink = payload.ink
        guard let plan = TextLayer.drawingPlan(payload, anchor: anchor, color: nil, layoutManager: manager),
              plan.width > 0, plan.height > 0,
              plan.width <= RasterImage.maxResizePixels / plan.height else {
            throw DoctoredCoreError(message: "Text layout exceeds the supported artwork extent.")
        }
        try manager.prepare(width: plan.width, height: plan.height)
        // Layout → raster for a placed or warped block: the plan's own map, never read from a
        // CTM.
        func layoutToRaster() -> CGAffineTransform {
            let frac = DescribedLayer.anchorFraction(anchor)
            let origin = CGPoint(x: plan.offsetX - Int(anchor.x.rounded(.down)),
                                 y: plan.offsetY - Int(anchor.y.rounded(.down)))
            return payload.transform.cgAffine.concatenating(
                CGAffineTransform(translationX: frac.x - origin.x, y: frac.y - origin.y))
        }
        if payload.onPath != nil {
            // Text on a path (ArtworkTextOnPath.swift): the plan laid the line out straight
            // in this manager.
            guard let container = manager.textContainers.first, let storage = manager.textStorage
            else { throw DoctoredCoreError(message: "Text on a path needs a laid-out line.") }
            manager.placement = try TextPathPlacement(
                payload: payload,
                stack: (manager: manager as NSLayoutManager, container: container, storage: storage),
                layoutToRaster: layoutToRaster())
        }
        if payload.warp != nil {
            // Warped text (ArtworkTextWarp.swift): the plan laid the block out straight in this
            // manager; every outline goes through the envelope, over the plan's own layout rect.
            // Never with a path (the decoder refuses both).
            guard let layoutRect = plan.layoutRect else {
                throw DoctoredCoreError(message: "Warped text needs a laid-out block.")
            }
            manager.warp = try TextWarpPlacement(
                payload: payload, layoutRect: layoutRect, layoutToRaster: layoutToRaster())
        }
        // This scalar context only drives TextKit's callbacks. Its raster is
        // discarded; it never supplies native text coverage or source pixels.
        guard let context = CGContext(data: nil, width: plan.width, height: plan.height,
            bitsPerComponent: 8, bytesPerRow: plan.width, space: CGColorSpaceCreateDeviceGray(),
            bitmapInfo: CGImageAlphaInfo.alphaOnly.rawValue) else {
            throw DoctoredCoreError(message: "Could not prepare the text capture context.")
        }
        guard Bitmap.drawFlipped(context, height: plan.height, appKit: true, plan.draw) else {
            throw DoctoredCoreError(message: "TextKit could not draw the captured layout.")
        }
        let parts = try manager.finish()
        return NativeArtworkDrawing(width: plan.width, height: plan.height,
            offsetX: plan.offsetX, offsetY: plan.offsetY, parts: parts)
    }

    /// The map from glyph space to the raster (pixels, y down) of a glyph
    /// TextKit draws at `position` under `textMatrix`, in a context whose CTM
    /// is `ctm` over a raster `height` rows tall.
    static func glyphMap(textMatrix: CGAffineTransform, position: CGPoint, ctm: CGAffineTransform,
                         height: Int) -> CGAffineTransform {
        var map = textMatrix
        map.tx += position.x
        map.ty += position.y
        return map.concatenating(ctm)
            .concatenating(CGAffineTransform(a: 1, b: 0, c: 0, d: -1, tx: 0, ty: CGFloat(height)))
    }

    /// The raster rect `glyph`'s reported bounds cover under `glyphMap`:
    /// `.null` for empty bounds, nil when a number is not finite. The text
    /// probe measures ink against this same rect.
    static func glyphRect(_ glyph: CGGlyph, at position: CGPoint, font: CTFont,
                          textMatrix: CGAffineTransform, ctm: CGAffineTransform, height: Int) -> CGRect? {
        var glyph = glyph
        let bounds = CTFontGetBoundingRectsForGlyphs(font, .default, &glyph, nil, 1)
        guard !bounds.isEmpty else { return .null }
        let map = glyphMap(textMatrix: textMatrix, position: position, ctm: ctm, height: height)
        let rect = bounds.applying(map)
        guard [map.a, map.b, map.c, map.d, map.tx, map.ty, rect.minX, rect.minY, rect.maxX, rect.maxY]
            .allSatisfy({ $0.isFinite }) else { return nil }
        return rect
    }
}

private final class TextArtworkLayout: NSLayoutManager {
    private typealias Window = NativeArtworkEvaluation.Window
    static let platformAllowance: UInt64 = 32 * 1024 * 1024
    private static let maximumParts = 256
    private static let maximumSamples = 64_000_000
    private static let maximumPoints = 262_144
    private var width = 0
    private var height = 0
    private var parts: [NativeArtworkPart] = []
    private var failure: Error?
    private var pointCount = 0
    private var intrinsicBytes: UInt64 = 0
    private var pending = CGMutablePath()
    private var pendingColor: [Double]?
    private var pendingBounds = CGRect.null
    /// The ink the payload's numbers are in: attribute colours
    /// are read back in it.
    var ink: ArtworkInk = .srgb
    /// Text on a path: where each glyph and decoration goes (ArtworkTextOnPath.swift).
    var placement: TextPathPlacement?
    /// Warped text: the envelope every outline goes through (ArtworkTextWarp.swift).
    var warp: TextWarpPlacement?
    private let windowing: ArtworkTextCapture.Windowing

    init(windowing: ArtworkTextCapture.Windowing) {
        self.windowing = windowing
        super.init()
    }
    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("TextArtworkLayout does not support NSCoder") }

    func prepare(width: Int, height: Int) throws {
        self.width = width
        self.height = height
        guard width <= Self.maximumSamples / height else {
            throw error("Text artwork exceeds the 64-million-sample limit of one component.")
        }
        try admit()
    }

    func finish() throws -> [NativeArtworkPart] {
        if let failure { throw failure }
        flush()
        return parts
    }

    private func error(_ message: String) -> DoctoredCoreError { DoctoredCoreError(message: message) }

    private func admit(extra: UInt64 = 0) throws {
        _ = try RasterResources.shared.admit(scratch: Self.platformAllowance
            + UInt64(width * height) + intrinsicBytes + UInt64(pointCount) * 64 + extra)
    }

    private var partRefusal: DoctoredCoreError {
        error("Text artwork exceeds 256 ordered parts or 64 million samples in one component.")
    }

    /// One more part holding `samples` samples: a path part is charged the
    /// text box here (its plane is windowed to its own glyphs downstream,
    /// `NativeArtworkEvaluation.Window`), a glyph part its plane's bytes over
    /// its own window (`glyphPlane`). The bound is the core's, per component
    /// (`artwork_window`); there is no total across parts.
    private func admitPart(samples: Int) throws {
        guard parts.count + (pendingColor == nil ? 0 : 1) < Self.maximumParts,
              samples <= Self.maximumSamples else { throw partRefusal }
        try admit(extra: UInt64(samples) * 2)
    }

    private func flush() {
        guard let color = pendingColor else { return }
        parts.append(.path(pending, rgba: color))
        pending = CGMutablePath()
        pendingColor = nil
        pendingBounds = .null
    }

    private func append(_ path: CGPath, color: [Double]) throws {
        guard !path.isEmpty else { return }
        let bounds = path.boundingBoxOfPath
        guard bounds.minX.isFinite, bounds.minY.isFinite, bounds.maxX.isFinite, bounds.maxY.isFinite,
              [bounds.minX, bounds.minY, bounds.maxX, bounds.maxY].allSatisfy({ abs($0) <= 10_000_000 }) else {
            throw error("TextKit produced an unsupported artwork path extent.")
        }
        var points = 0
        path.applyWithBlock { element in
            switch element.pointee.type {
            case .moveToPoint, .addLineToPoint: points += 1
            case .addQuadCurveToPoint: points += 2
            case .addCurveToPoint: points += 3
            case .closeSubpath: break
            @unknown default: points = Self.maximumPoints + 1
            }
        }
        guard points <= Self.maximumPoints - pointCount else {
            throw error("Text outlines exceed the bounded native path-point limit.")
        }
        // Merge only disjoint ink with identical color. Overlapping glyphs or
        // decorations retain their original drawing order and opacity layers.
        if pendingColor != color || pendingBounds.intersects(bounds) { flush() }
        if pendingColor == nil {
            try admitPart(samples: width * height)
            pendingColor = color
        }
        try admit(extra: UInt64(points) * 64)
        pointCount += points
        pending.addPath(path)
        pendingBounds = pendingBounds.union(bounds)
    }

    private func color(_ attributes: [NSAttributedString.Key: Any], decoration: NSAttributedString.Key? = nil) throws -> [Double] {
        let ink = decoration.flatMap { attributes[$0] as? NSColor }
            ?? attributes[.foregroundColor] as? NSColor ?? .black
        guard let values = ArtworkColor.rgba(ink, in: self.ink) else {
            throw error("TextKit returned unsupported text ink.")
        }
        return values
    }

    override func showCGGlyphs(_ glyphs: UnsafePointer<CGGlyph>, positions: UnsafePointer<CGPoint>,
        count: Int, font: NSFont, textMatrix: CGAffineTransform,
        attributes: [NSAttributedString.Key: Any], in context: CGContext) {
        guard failure == nil else { return }
        do {
            guard count >= 0, count <= 4096 else { throw error("TextKit returned too many glyphs.") }
            let rgba = try color(attributes)
            let ctFont = font as CTFont
            let colorGlyphs = CTFontGetSymbolicTraits(ctFont).contains(.traitColorGlyphs)
            // On a path only an outline can be placed (TP-5), and under a warp bent (WT-3).
            if placement != nil, colorGlyphs { throw error(TextPathPlacement.outlineGlyphsRefusal) }
            if warp != nil, colorGlyphs { throw error(TextWarp.outlineGlyphsRefusal) }
            if colorGlyphs, !windowing.colorAsCoverage {
                try intrinsic(glyphs, positions: positions, count: count, font: font,
                    textMatrix: textMatrix, attributes: attributes, context: context, rgba: rgba)
                return
            }
            let placed = try placement.map { placement -> [CGAffineTransform?] in
                let straight = (0..<count).map {
                    ArtworkTextCapture.glyphMap(textMatrix: textMatrix, position: positions[$0],
                                                ctm: context.ctm, height: height)
                }
                var advances = [CGSize](repeating: .zero, count: count)
                CTFontGetAdvancesForGlyphs(ctFont, .horizontal, glyphs, &advances, count)
                return try placement.glyphMaps(straight, advances: advances.map(\.width))
            }
            for i in 0..<count {
                // A colour glyph comes here only under the probe's `colorAsCoverage`.
                guard !colorGlyphs, let path = CTFontCreatePathForGlyph(ctFont, glyphs[i], nil) else {
                    var glyph = glyphs[i]
                    let bounds = CTFontGetBoundingRectsForGlyphs(ctFont, .default, &glyph, nil, 1)
                    if !bounds.isEmpty {
                        if placement != nil { throw error(TextPathPlacement.outlineGlyphsRefusal) }
                        if warp != nil { throw error(TextWarp.outlineGlyphsRefusal) }
                        try intrinsicCoverage(glyphs.advanced(by: i), positions: positions.advanced(by: i), count: 1,
                            font: font, textMatrix: textMatrix, attributes: attributes, context: context, rgba: rgba)
                    }
                    continue
                }
                var map = ArtworkTextCapture.glyphMap(textMatrix: textMatrix, position: positions[i],
                                                      ctm: context.ctm, height: height)
                if let placed {
                    // Past an open subpath's end: the glyph is not drawn (TP-3).
                    guard let onPath = placed[i] else { continue }
                    map = onPath
                }
                guard [map.a, map.b, map.c, map.d, map.tx, map.ty].allSatisfy({ $0.isFinite && abs($0) <= 10_000_000 }) else {
                    throw error("TextKit produced an unsupported glyph transform.")
                }
                guard let output = path.copy(using: &map) else { throw error("Could not place the resolved glyph outline.") }
                // Under a warp each glyph is bent on its own, so overlapping glyphs keep their
                // order (WT-3).
                try append(warp.map { try $0.outline(output) } ?? output, color: rgba)
            }
        } catch { failure = error }
    }

    private func intrinsic(_ glyphs: UnsafePointer<CGGlyph>, positions: UnsafePointer<CGPoint>, count: Int,
        font: NSFont, textMatrix: CGAffineTransform, attributes: [NSAttributedString.Key: Any],
        context original: CGContext, rgba: [Double]) throws {
        flush()
        var opaqueAttributes = attributes
        // The colour's own numbers with alpha 1 — all four inks in a CMYK
        // document (#1), where alpha is the fifth (`ArtworkColor.alpha`).
        guard let opaque = ArtworkColor.color(Array(rgba.dropLast()) + [1], in: ink) else {
            throw error("TextKit returned unsupported text ink.")
        }
        opaqueAttributes[.foregroundColor] = opaque
        guard var plane = try glyphPlane(glyphs, positions: positions, count: count, font: font,
            textMatrix: textMatrix, attributes: opaqueAttributes, original: original, planes: 4,
            ink: opaque, dropsInkless: false) else { return }
        Bitmap.unpremultiply(&plane.bytes)
        parts.append(.intrinsic(pixels: plane.bytes, opacity: ArtworkColor.alpha(rgba),
                                window: plane.window))
    }

    private func intrinsicCoverage(_ glyphs: UnsafePointer<CGGlyph>, positions: UnsafePointer<CGPoint>, count: Int,
        font: NSFont, textMatrix: CGAffineTransform, attributes: [NSAttributedString.Key: Any],
        context original: CGContext, rgba: [Double]) throws {
        var opaqueAttributes = attributes
        opaqueAttributes[.foregroundColor] = NSColor.white
        // An outline-less glyph that draws no ink in its window (Apple
        // Chancery's U+0338 reports bounds but has neither outline nor
        // bitmap) adds no part.
        guard let plane = try glyphPlane(glyphs, positions: positions, count: count, font: font,
            textMatrix: textMatrix, attributes: opaqueAttributes, original: original, planes: 1,
            ink: .white, dropsInkless: true) else { return }
        flush()
        parts.append(.coverage(pixels: plane.bytes, rgba: rgba, window: plane.window))
    }

    /// The window a call's glyphs ink: the union of their reported bounds in
    /// the raster (`ArtworkTextCapture.glyphRect`), grown by the margin and
    /// clipped to it. The whole raster when a number is not finite or no
    /// glyph reports bounds.
    private func glyphWindow(_ glyphs: UnsafePointer<CGGlyph>, positions: UnsafePointer<CGPoint>, count: Int,
        font: NSFont, textMatrix: CGAffineTransform, ctm: CGAffineTransform) -> Window {
        var union = CGRect.null
        for i in 0..<count {
            guard let rect = ArtworkTextCapture.glyphRect(glyphs[i], at: positions[i], font: font as CTFont,
                textMatrix: textMatrix, ctm: ctm, height: height) else {
                return Window(x: 0, y: 0, width: width, height: height)
            }
            union = union.union(rect)
        }
        return .covering(union, margin: windowing.margin, width: width, height: height)
    }

    /// The superclass's drawing of `glyphs` as one part's plane, `planes`
    /// bytes a pixel (4: premultiplied sRGB RGBA8; 1: A8), over the window
    /// they ink (`glyphWindow`), admitted as a part. Ink is never clipped:
    /// when ink touches the window's outer ring on a side the raster does not
    /// bound, it may continue past the window, so the plane is drawn again
    /// over the whole raster. nil when `dropsInkless` and the plane holds no
    /// ink: that glyph adds no part.
    private func glyphPlane(_ glyphs: UnsafePointer<CGGlyph>, positions: UnsafePointer<CGPoint>, count: Int,
        font: NSFont, textMatrix: CGAffineTransform, attributes: [NSAttributedString.Key: Any],
        original: CGContext, planes: Int, ink: NSColor,
        dropsInkless: Bool) throws -> (window: Window, bytes: [UInt8])? {
        let whole = Window(x: 0, y: 0, width: width, height: height)
        var window = glyphWindow(glyphs, positions: positions, count: count, font: font,
                                 textMatrix: textMatrix, ctm: original.ctm)
        func draw(_ window: Window) throws -> [UInt8] {
            try drawPlane(glyphs, positions: positions, count: count, font: font, textMatrix: textMatrix,
                attributes: attributes, original: original, window: window, planes: planes, ink: ink)
        }
        var bytes = try draw(window)
        if window != whole, inksRing(bytes, window: window, planes: planes) {
            window = whole
            bytes = try draw(whole)
        }
        let alphas = stride(from: planes - 1, to: bytes.count, by: planes)
        if dropsInkless, !alphas.contains(where: { bytes[$0] != 0 }) { return nil }
        try admitPart(samples: bytes.count)
        intrinsicBytes += UInt64(bytes.count)
        return (window, bytes)
    }

    /// `glyphs` drawn by the superclass into a plane of `window`'s pixels,
    /// row 0 at the window's top (`glyphPlane`).
    private func drawPlane(_ glyphs: UnsafePointer<CGGlyph>, positions: UnsafePointer<CGPoint>, count: Int,
        font: NSFont, textMatrix: CGAffineTransform, attributes: [NSAttributedString.Key: Any],
        original: CGContext, window: Window, planes: Int, ink: NSColor) throws -> [UInt8] {
        let samples = window.pixels * planes
        guard samples <= Self.maximumSamples else { throw partRefusal }
        try admit(extra: UInt64(samples))
        var bytes = [UInt8](repeating: 0, count: samples)
        let drawn = bytes.withUnsafeMutableBytes { buffer -> Bool in
            let space = planes == 4 ? CGColorSpace(name: CGColorSpace.sRGB) : CGColorSpaceCreateDeviceGray()
            let alpha = planes == 4 ? CGImageAlphaInfo.premultipliedLast : .alphaOnly
            guard let space, let context = CGContext(data: buffer.baseAddress, width: window.width,
                height: window.height, bitsPerComponent: 8, bytesPerRow: window.width * planes,
                space: space, bitmapInfo: alpha.rawValue) else { return false }
            // The window's lower-left corner in the raster's device space (y up).
            drawSuperclass(glyphs, positions: positions, count: count, font: font, textMatrix: textMatrix,
                attributes: attributes, original: original, context: context, ink: ink,
                deviceOrigin: CGPoint(x: window.x, y: height - window.y - window.height))
            return true
        }
        guard drawn else {
            throw error(planes == 4 ? "Could not capture intrinsic glyph pixels."
                : "Could not capture intrinsic monochrome glyph coverage.")
        }
        return bytes
    }

    /// Whether `bytes` (`planes` a pixel, alpha last) inks `window`'s outer
    /// ring on a side the raster does not bound.
    private func inksRing(_ bytes: [UInt8], window: Window, planes: Int) -> Bool {
        func inked(_ x: Int, _ y: Int) -> Bool { bytes[(y * window.width + x) * planes + planes - 1] != 0 }
        let rows = 0..<window.height, columns = 0..<window.width
        return (window.x > 0 && rows.contains { inked(0, $0) })
            || (window.x + window.width < width && rows.contains { inked(window.width - 1, $0) })
            || (window.y > 0 && columns.contains { inked($0, 0) })
            || (window.y + window.height < height && columns.contains { inked($0, window.height - 1) })
    }

    private func drawSuperclass(_ glyphs: UnsafePointer<CGGlyph>, positions: UnsafePointer<CGPoint>, count: Int,
        font: NSFont, textMatrix: CGAffineTransform, attributes: [NSAttributedString.Key: Any],
        original: CGContext, context: CGContext, ink: NSColor,
        deviceOrigin: CGPoint = .zero) {
        context.translateBy(x: -deviceOrigin.x, y: -deviceOrigin.y)
        context.concatenate(original.ctm)
        context.textMatrix = original.textMatrix
        context.textPosition = original.textPosition
        context.setShouldAntialias(true)
        context.setAllowsFontSmoothing(false)
        context.setFillColor(ink.cgColor)
        context.setStrokeColor(ink.cgColor)
        NSGraphicsContext.saveGraphicsState()
        defer { NSGraphicsContext.restoreGraphicsState() }
        NSGraphicsContext.current = NSGraphicsContext(cgContext: context, flipped: true)
        super.showCGGlyphs(glyphs, positions: positions, count: count, font: font,
            textMatrix: textMatrix, attributes: attributes, in: context)
    }

    private func decoration(_ range: NSRange, key: NSAttributedString.Key, draw: () -> Void) {
        guard failure == nil else { return }
        do {
            guard let storage = textStorage, range.length > 0,
                  range.location < numberOfGlyphs,
                  let source = NSGraphicsContext.current?.cgContext else {
                throw error("TextKit returned an invalid decoration range.")
            }
            let index = characterIndexForGlyph(at: range.location)
            guard index < storage.length else { throw error("Text decoration has no character attributes.") }
            let rgba = try color(storage.attributes(at: index, effectiveRange: nil), decoration: key)
            try admit(extra: 2 * 1024 * 1024)
            let paths = try TextDecorationPDF.capture(width: width, height: height, transform: source.ctm, draw: draw)
            // On a path a decoration bends with the glyphs (TP-4), and under a warp (WT-3).
            for path in paths {
                let bent = try warp.map { try $0.outline(path) } ?? path
                try append(placement.map { try $0.decoration(bent) } ?? bent, color: rgba)
            }
        } catch { failure = error }
    }

    override func drawUnderline(forGlyphRange range: NSRange, underlineType: NSUnderlineStyle,
        baselineOffset: CGFloat, lineFragmentRect: CGRect, lineFragmentGlyphRange: NSRange, containerOrigin: CGPoint) {
        decoration(range, key: .underlineColor) {
            super.drawUnderline(forGlyphRange: range, underlineType: underlineType, baselineOffset: baselineOffset,
                lineFragmentRect: lineFragmentRect, lineFragmentGlyphRange: lineFragmentGlyphRange, containerOrigin: containerOrigin)
        }
    }

    override func drawStrikethrough(forGlyphRange range: NSRange, strikethroughType: NSUnderlineStyle,
        baselineOffset: CGFloat, lineFragmentRect: CGRect, lineFragmentGlyphRange: NSRange, containerOrigin: CGPoint) {
        decoration(range, key: .strikethroughColor) {
            super.drawStrikethrough(forGlyphRange: range, strikethroughType: strikethroughType, baselineOffset: baselineOffset,
                lineFragmentRect: lineFragmentRect, lineFragmentGlyphRange: lineFragmentGlyphRange, containerOrigin: containerOrigin)
        }
    }
}

/// Superclass decoration drawing emits a tiny vector-only PDF. Its color
/// literals are ignored; the original attributed RGBA remains authoritative.
private final class TextDecorationPDF {
    private final class Bytes {
        let data = NSMutableData()
        var refused = false
    }
    private struct State {
        var transform = CGAffineTransform.identity
        var width: CGFloat = 1
        var cap = CGLineCap.butt
        var join = CGLineJoin.miter
        var miter: CGFloat = 10
    }
    private var state = State()
    private var stack: [State] = []
    private var path = CGMutablePath()
    private var paths: [CGPath] = []
    private var failure: String?
    private var operators = 0
    private var points = 0
    private let flip: CGAffineTransform

    private init(height: Int) { flip = CGAffineTransform(a: 1, b: 0, c: 0, d: -1, tx: 0, ty: CGFloat(height)) }

    static func capture(width: Int, height: Int, transform: CGAffineTransform, draw: () -> Void) throws -> [CGPath] {
        let output = Bytes()
        var callbacks = CGDataConsumerCallbacks(putBytes: { info, buffer, count in
            guard let info else { return 0 }
            let sink = Unmanaged<Bytes>.fromOpaque(info).takeUnretainedValue()
            guard count <= 256 * 1024 - sink.data.length else { sink.refused = true; return 0 }
            sink.data.append(buffer, length: count)
            return count
        }, releaseConsumer: nil)
        var box = CGRect(x: 0, y: 0, width: width, height: height)
        guard let consumer = CGDataConsumer(info: Unmanaged.passUnretained(output).toOpaque(), cbks: &callbacks),
              let context = CGContext(consumer: consumer, mediaBox: &box, nil) else {
            throw DoctoredCoreError(message: "Could not capture TextKit decoration geometry.")
        }
        context.beginPDFPage(nil)
        context.concatenate(transform)
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(cgContext: context, flipped: true)
        draw()
        NSGraphicsContext.restoreGraphicsState()
        context.endPDFPage()
        context.closePDF()
        guard !output.refused, let provider = CGDataProvider(data: output.data),
              let document = CGPDFDocument(provider), document.numberOfPages == 1,
              let page = document.page(at: 1), let table = CGPDFOperatorTableCreate() else {
            throw DoctoredCoreError(message: "TextKit decoration exceeds the bounded vector document.")
        }
        defer { CGPDFOperatorTableRelease(table) }
        let capture = TextDecorationPDF(height: height)
        let stream = CGPDFContentStreamCreateWithPage(page)
        defer { CGPDFContentStreamRelease(stream) }
        let operators: [(String, CGPDFOperatorCallback)] = [
            ("q", { scan, info in TextDecorationPDF.operate("q", scan, info) }), ("Q", { scan, info in TextDecorationPDF.operate("Q", scan, info) }),
            ("cm", { scan, info in TextDecorationPDF.operate("cm", scan, info) }), ("m", { scan, info in TextDecorationPDF.operate("m", scan, info) }),
            ("l", { scan, info in TextDecorationPDF.operate("l", scan, info) }), ("c", { scan, info in TextDecorationPDF.operate("c", scan, info) }),
            ("v", { scan, info in TextDecorationPDF.operate("v", scan, info) }), ("y", { scan, info in TextDecorationPDF.operate("y", scan, info) }),
            ("re", { scan, info in TextDecorationPDF.operate("re", scan, info) }), ("h", { scan, info in TextDecorationPDF.operate("h", scan, info) }),
            ("f", { scan, info in TextDecorationPDF.operate("f", scan, info) }), ("F", { scan, info in TextDecorationPDF.operate("f", scan, info) }),
            ("S", { scan, info in TextDecorationPDF.operate("S", scan, info) }), ("s", { scan, info in TextDecorationPDF.operate("s", scan, info) }),
            ("n", { scan, info in TextDecorationPDF.operate("n", scan, info) }), ("w", { scan, info in TextDecorationPDF.operate("w", scan, info) }),
            ("J", { scan, info in TextDecorationPDF.operate("J", scan, info) }), ("j", { scan, info in TextDecorationPDF.operate("j", scan, info) }),
            ("M", { scan, info in TextDecorationPDF.operate("M", scan, info) }),
            ("gs", { scan, info in TextDecorationPDF.operate("gs", scan, info) })
        ]
        for (name, callback) in operators { CGPDFOperatorTableSetCallback(table, name, callback) }
        for name in ["g", "G", "rg", "RG", "k", "K", "cs", "CS", "sc", "SC", "scn", "SCN", "ri", "i"] {
            CGPDFOperatorTableSetCallback(table, name) { scan, info in TextDecorationPDF.operate("color", scan, info) }
        }
        for name in ["f*", "B", "B*", "b", "b*", "W", "W*", "d", "Do", "sh", "BI", "BT", "ET", "Tj", "TJ", "'", "\""] {
            CGPDFOperatorTableSetCallback(table, name) { scan, info in TextDecorationPDF.operate("unsupported", scan, info) }
        }
        let scanner = CGPDFScannerCreate(stream, table, Unmanaged.passUnretained(capture).toOpaque())
        defer { CGPDFScannerRelease(scanner) }
        guard CGPDFScannerScan(scanner), capture.failure == nil, capture.stack.isEmpty else {
            throw DoctoredCoreError(message: capture.failure ?? "Unsupported TextKit decoration graphics state.")
        }
        return capture.paths
    }

    private static func operate(_ name: String, _ scanner: CGPDFScannerRef?, _ info: UnsafeMutableRawPointer?) {
        guard let scanner, let info else { return }
        let capture = Unmanaged<TextDecorationPDF>.fromOpaque(info).takeUnretainedValue()
        guard capture.failure == nil else { return }
        capture.operators += 1
        guard capture.operators <= 4096 else { capture.failure = "Text decoration exceeds its operator limit."; return }
        func numbers(_ count: Int) -> [CGFloat]? {
            var values = [CGFloat](repeating: 0, count: count)
            for index in (0..<count).reversed() {
                var value = CGPDFReal()
                guard CGPDFScannerPopNumber(scanner, &value), value.isFinite, abs(value) <= 10_000_000 else { return nil }
                values[index] = CGFloat(value)
            }
            return values
        }
        let count: Int
        switch name {
        case "cm", "c": count = 6
        case "re", "v", "y": count = 4
        case "m", "l": count = 2
        case "w", "J", "j", "M": count = 1
        default: count = 0
        }
        guard let v = numbers(count) else { capture.failure = "Invalid TextKit decoration operands."; return }
        capture.points += count / 2
        guard capture.points <= 16_384 else { capture.failure = "Text decoration exceeds its path limit."; return }
        switch name {
        case "color": break
        case "gs":
            var name: UnsafePointer<CChar>?
            var dictionary: CGPDFDictionaryRef?
            guard CGPDFScannerPopName(scanner, &name), let name,
                  let object = CGPDFContentStreamGetResource(CGPDFScannerGetContentStream(scanner), "ExtGState", name),
                  CGPDFObjectGetValue(object, .dictionary, &dictionary), let dictionary else {
                capture.failure = "Invalid TextKit decoration extended state."; return
            }
            var recognized = 0
            for key in ["ca", "CA"] {
                var object: CGPDFObjectRef?
                if CGPDFDictionaryGetObject(dictionary, key, &object) {
                    var alpha = CGPDFReal()
                    guard CGPDFDictionaryGetNumber(dictionary, key, &alpha), alpha.isFinite, (0...1).contains(alpha) else {
                        capture.failure = "Unsupported TextKit decoration alpha state."; return
                    }
                    recognized += 1
                }
            }
            var typeObject: CGPDFObjectRef?
            if CGPDFDictionaryGetObject(dictionary, "Type", &typeObject) {
                var type: UnsafePointer<CChar>?
                guard CGPDFDictionaryGetName(dictionary, "Type", &type), let type, String(cString: type) == "ExtGState" else {
                    capture.failure = "Unsupported TextKit decoration state type."; return
                }
                recognized += 1
            }
            guard recognized == CGPDFDictionaryGetCount(dictionary) else {
                capture.failure = "TextKit decoration uses unsupported blend, mask or line state."; return
            }
        case "q":
            guard capture.stack.count < 32 else { capture.failure = "Text decoration graphics-state nesting exceeds its limit."; return }
            capture.stack.append(capture.state)
        case "Q":
            guard capture.path.isEmpty else { capture.failure = "Text decoration restores state inside a path."; return }
            guard let prior = capture.stack.popLast() else { capture.failure = "Unbalanced TextKit decoration state."; return }
            capture.state = prior
        case "cm":
            guard capture.path.isEmpty else { capture.failure = "Text decoration changes its transform inside a path."; return }
            capture.state.transform = CGAffineTransform(a: v[0], b: v[1], c: v[2], d: v[3], tx: v[4], ty: v[5]).concatenating(capture.state.transform)
            let t = capture.state.transform
            guard [t.a, t.b, t.c, t.d, t.tx, t.ty].allSatisfy({ $0.isFinite && abs($0) <= 10_000_000 }) else {
                capture.failure = "Text decoration exceeds its transform limit."; return
            }
        case "m": capture.path.move(to: CGPoint(x: v[0], y: v[1]))
        case "l": capture.path.addLine(to: CGPoint(x: v[0], y: v[1]))
        case "c": capture.path.addCurve(to: CGPoint(x: v[4], y: v[5]), control1: CGPoint(x: v[0], y: v[1]), control2: CGPoint(x: v[2], y: v[3]))
        case "v": capture.path.addCurve(to: CGPoint(x: v[2], y: v[3]), control1: capture.path.currentPoint, control2: CGPoint(x: v[0], y: v[1]))
        case "y": capture.path.addCurve(to: CGPoint(x: v[2], y: v[3]), control1: CGPoint(x: v[0], y: v[1]), control2: CGPoint(x: v[2], y: v[3]))
        case "re": capture.path.addRect(CGRect(x: v[0], y: v[1], width: v[2], height: v[3]))
        case "h": capture.path.closeSubpath()
        case "n": capture.path = CGMutablePath()
        case "w": capture.state.width = v[0]
        case "M": capture.state.miter = v[0]
        case "J":
            guard let cap = CGLineCap(rawValue: Int32(v[0])), v[0].rounded() == v[0] else { capture.failure = "Unsupported decoration line cap."; return }
            capture.state.cap = cap
        case "j":
            guard let join = CGLineJoin(rawValue: Int32(v[0])), v[0].rounded() == v[0] else { capture.failure = "Unsupported decoration line join."; return }
            capture.state.join = join
        case "f", "S", "s":
            guard capture.paths.count < 256 else { capture.failure = "Too many TextKit decoration paths."; return }
            if name == "s" { capture.path.closeSubpath() }
            let path: CGPath
            if name != "f" {
                guard capture.state.width > 0, capture.state.miter > 0 else { capture.failure = "Unsupported decoration stroke width."; return }
                path = capture.path.copy(strokingWithWidth: capture.state.width, lineCap: capture.state.cap,
                    lineJoin: capture.state.join, miterLimit: capture.state.miter)
            } else { path = capture.path }
            var transform = capture.state.transform.concatenating(capture.flip)
            guard let output = path.copy(using: &transform) else { capture.failure = "Could not transform TextKit decoration."; return }
            capture.paths.append(output)
            capture.path = CGMutablePath()
        default: capture.failure = "TextKit decoration requires unsupported raster, text, clipping or fill semantics."
        }
    }
}
