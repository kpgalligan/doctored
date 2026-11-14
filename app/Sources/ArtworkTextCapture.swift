import AppKit
import CoreText

// TextKit owns shaping/layout; this file sends resolved outlines and superclass decoration paths to
// Rust.

/// TextKit supplies resolved glyphs, positions and decorations. Intrinsic
/// glyph assets retain their byte color/coverage; authored ink stays continuous.
enum ArtworkTextCapture {
    static func capture(payload: TextLayerPayload, anchor: CGPoint) throws -> NativeArtworkDrawing {
        guard Thread.isMainThread, !BackgroundWork.occupied,
              !payload.string.isEmpty, payload.string.utf16.count <= 4096 else {
            throw DoctoredCoreError(message: "Prepare at most 4096 UTF-16 text units on the main thread after background work finishes.")
        }
        _ = try RasterResources.shared.admit(scratch: TextArtworkLayout.platformAllowance)
        let manager = TextArtworkLayout()
        manager.ink = payload.ink
        guard let plan = TextLayer.drawingPlan(payload, anchor: anchor, color: nil, layoutManager: manager),
              plan.width > 0, plan.height > 0,
              plan.width <= RasterImage.maxResizePixels / plan.height else {
            throw DoctoredCoreError(message: "Text layout exceeds the supported artwork extent.")
        }
        try manager.prepare(width: plan.width, height: plan.height)
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
}

private final class TextArtworkLayout: NSLayoutManager {
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

    override init() { super.init() }
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

    /// One more part of `planes` samples a pixel over the text box. The bound
    /// is the core's, per component (`artwork_window`): there is no total
    /// across parts, and a path part's plane is windowed to its own glyphs
    /// downstream (`NativeArtworkEvaluation.Window`).
    private func admitPart(planes: Int) throws {
        let count = width * height
        guard parts.count + (pendingColor == nil ? 0 : 1) < Self.maximumParts,
              count <= Self.maximumSamples / planes else {
            throw error("Text artwork exceeds 256 ordered parts or 64 million samples in one component.")
        }
        try admit(extra: UInt64(count * planes) * 2)
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
            try admitPart(planes: 1)
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

    private var pageToOutput: CGAffineTransform {
        CGAffineTransform(a: 1, b: 0, c: 0, d: -1, tx: 0, ty: CGFloat(height))
    }

    override func showCGGlyphs(_ glyphs: UnsafePointer<CGGlyph>, positions: UnsafePointer<CGPoint>,
        count: Int, font: NSFont, textMatrix: CGAffineTransform,
        attributes: [NSAttributedString.Key: Any], in context: CGContext) {
        guard failure == nil else { return }
        do {
            guard count >= 0, count <= 4096 else { throw error("TextKit returned too many glyphs.") }
            let rgba = try color(attributes)
            let ctFont = font as CTFont
            if CTFontGetSymbolicTraits(ctFont).contains(.traitColorGlyphs) {
                try intrinsic(glyphs, positions: positions, count: count, font: font,
                    textMatrix: textMatrix, attributes: attributes, context: context, rgba: rgba)
                return
            }
            for i in 0..<count {
                guard let path = CTFontCreatePathForGlyph(ctFont, glyphs[i], nil) else {
                    var glyph = glyphs[i]
                    let bounds = CTFontGetBoundingRectsForGlyphs(ctFont, .default, &glyph, nil, 1)
                    if !bounds.isEmpty {
                        try intrinsicCoverage(glyphs.advanced(by: i), positions: positions.advanced(by: i), count: 1,
                            font: font, textMatrix: textMatrix, attributes: attributes, context: context, rgba: rgba)
                    }
                    continue
                }
                var map = textMatrix
                map.tx += positions[i].x
                map.ty += positions[i].y
                map = map.concatenating(context.ctm).concatenating(pageToOutput)
                guard [map.a, map.b, map.c, map.d, map.tx, map.ty].allSatisfy({ $0.isFinite && abs($0) <= 10_000_000 }) else {
                    throw error("TextKit produced an unsupported glyph transform.")
                }
                guard let output = path.copy(using: &map) else { throw error("Could not place the resolved glyph outline.") }
                try append(output, color: rgba)
            }
        } catch { failure = error }
    }

    private func intrinsic(_ glyphs: UnsafePointer<CGGlyph>, positions: UnsafePointer<CGPoint>, count: Int,
        font: NSFont, textMatrix: CGAffineTransform, attributes: [NSAttributedString.Key: Any],
        context original: CGContext, rgba: [Double]) throws {
        flush()
        try admitPart(planes: 4)
        let countBytes = width * height * 4
        try admit(extra: UInt64(countBytes))
        var pixels = [UInt8](repeating: 0, count: countBytes)
        var opaqueAttributes = attributes
        // The colour's own numbers with alpha 1 — all four inks in a CMYK
        // document (#1), where alpha is the fifth (`ArtworkColor.alpha`).
        guard let opaque = ArtworkColor.color(Array(rgba.dropLast()) + [1], in: ink) else {
            throw error("TextKit returned unsupported text ink.")
        }
        opaqueAttributes[.foregroundColor] = opaque
        let drawn = pixels.withUnsafeMutableBytes { buffer -> Bool in
            guard let space = CGColorSpace(name: CGColorSpace.sRGB),
                  let context = CGContext(data: buffer.baseAddress, width: width, height: height,
                    bitsPerComponent: 8, bytesPerRow: width * 4, space: space,
                    bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return false }
            drawSuperclass(glyphs, positions: positions, count: count, font: font, textMatrix: textMatrix,
                attributes: opaqueAttributes, original: original, context: context, ink: opaque)
            return true
        }
        guard drawn else { throw error("Could not capture intrinsic glyph pixels.") }
        Bitmap.unpremultiply(&pixels)
        intrinsicBytes += UInt64(countBytes)
        parts.append(.intrinsic(pixels: pixels, opacity: ArtworkColor.alpha(rgba)))
    }

    private func intrinsicCoverage(_ glyphs: UnsafePointer<CGGlyph>, positions: UnsafePointer<CGPoint>, count: Int,
        font: NSFont, textMatrix: CGAffineTransform, attributes: [NSAttributedString.Key: Any],
        context original: CGContext, rgba: [Double]) throws {
        var opaqueAttributes = attributes
        opaqueAttributes[.foregroundColor] = NSColor.white
        // An outline-less glyph that draws no ink (Apple Chancery's
        // U+0338 reports bounds but has neither outline nor bitmap) adds no
        // part: its glyph box is probed first, in a box-sized A8 plane placed
        // by the path branch's map. Only a glyph that inks keeps the full
        // A8 coverage part below.
        var glyph = glyphs[0]
        let bounds = CTFontGetBoundingRectsForGlyphs(font as CTFont, .default, &glyph, nil, 1)
        var map = textMatrix
        map.tx += positions[0].x
        map.ty += positions[0].y
        map = map.concatenating(original.ctm).concatenating(pageToOutput)
        let mapped = bounds.applying(map)
        if [map.a, map.b, map.c, map.d, map.tx, map.ty].allSatisfy({ $0.isFinite }),
           [mapped.minX, mapped.minY, mapped.maxX, mapped.maxY].allSatisfy({ $0.isFinite }) {
            let box = mapped.integral.insetBy(dx: -2, dy: -2)
                .intersection(CGRect(x: 0, y: 0, width: width, height: height))
            guard !box.isEmpty else { return }
            let boxWidth = Int(box.width)
            let boxHeight = Int(box.height)
            try admit(extra: UInt64(boxWidth * boxHeight))
            var probe = [UInt8](repeating: 0, count: boxWidth * boxHeight)
            let probed = probe.withUnsafeMutableBytes { buffer -> Bool in
                guard let context = CGContext(data: buffer.baseAddress, width: boxWidth,
                    height: boxHeight, bitsPerComponent: 8, bytesPerRow: boxWidth,
                    space: CGColorSpaceCreateDeviceGray(),
                    bitmapInfo: CGImageAlphaInfo.alphaOnly.rawValue) else { return false }
                drawSuperclass(glyphs, positions: positions, count: count, font: font,
                    textMatrix: textMatrix, attributes: opaqueAttributes, original: original,
                    context: context, ink: .white,
                    deviceOrigin: CGPoint(x: box.minX, y: CGFloat(height) - box.maxY))
                return true
            }
            guard probed else { throw error("Could not capture intrinsic monochrome glyph coverage.") }
            guard probe.contains(where: { $0 != 0 }) else { return }
        }
        flush()
        try admitPart(planes: 1)
        let countBytes = width * height
        try admit(extra: UInt64(countBytes))
        var pixels = [UInt8](repeating: 0, count: countBytes)
        let drawn = pixels.withUnsafeMutableBytes { buffer -> Bool in
            guard let context = CGContext(data: buffer.baseAddress, width: width, height: height,
                bitsPerComponent: 8, bytesPerRow: width, space: CGColorSpaceCreateDeviceGray(),
                bitmapInfo: CGImageAlphaInfo.alphaOnly.rawValue) else { return false }
            drawSuperclass(glyphs, positions: positions, count: count, font: font, textMatrix: textMatrix,
                attributes: opaqueAttributes, original: original, context: context, ink: .white)
            return true
        }
        guard drawn else { throw error("Could not capture intrinsic monochrome glyph coverage.") }
        intrinsicBytes += UInt64(countBytes)
        parts.append(.coverage(pixels: pixels, rgba: rgba))
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
            for path in paths { try append(path, color: rgba) }
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
