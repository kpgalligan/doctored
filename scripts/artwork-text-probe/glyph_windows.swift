import AppKit
import CoreText

// Colour-glyph and bitmap-glyph parts at their own box (`ArtworkTextCapture`'s
// glyph windows): the evidence that a glyph's reported bounds hold the ink
// TextKit draws for it, the exact gates of the windowed planes against the
// same glyphs drawn whole by an independent recorder, the edge guard, the
// charge of a large text box with emoji runs, and the windows placed by the
// core in a 16-bit document.

private typealias GlyphWindow = NativeArtworkEvaluation.Window

/// One `showCGGlyphs` call (a run) as the recorder sees it: the raster rect
/// each of its glyphs' reported bounds covers (`ArtworkTextCapture.glyphRect`,
/// empty bounds left out), whether a rect was not finite (capture then takes
/// the whole raster), and the run drawn ALONE over the whole raster by the
/// superclass, straight RGBA8.
private struct GlyphInkRun {
    let color: Bool
    let rects: [CGRect]
    let unbounded: Bool
    let plane: [UInt8]
}

/// An independent recorder: the ordinary superclass draws each run once more,
/// alone, into a whole-raster plane of its own, as capture drew every glyph
/// part before glyph windows. With `drawsCoverage`, each colour glyph that
/// reports bounds is also drawn alone into a whole-raster A8 plane and kept
/// when it inks, which is what capture's bitmap-glyph branch makes of it
/// under `Windowing.colorAsCoverage`. It also counts the glyphs capture
/// sends down that branch in an ordinary run (no outline, non-empty bounds,
/// not a colour font) and how many of them ink.
private final class GlyphInkRecorder: NSLayoutManager {
    let drawsCoverage: Bool
    private(set) var runs: [GlyphInkRun] = []
    private(set) var coverage: [[UInt8]] = []
    private(set) var outlineLess = 0
    private(set) var outlineLessInked = 0
    init(drawsCoverage: Bool = false) {
        self.drawsCoverage = drawsCoverage
        super.init()
    }
    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("GlyphInkRecorder does not support NSCoder") }

    override func showCGGlyphs(_ glyphs: UnsafePointer<CGGlyph>, positions: UnsafePointer<CGPoint>, count: Int,
        font: NSFont, textMatrix: CGAffineTransform, attributes: [NSAttributedString.Key: Any], in context: CGContext) {
        let ctFont = font as CTFont
        var white = attributes
        white[.foregroundColor] = NSColor.white
        if CTFontGetSymbolicTraits(ctFont).contains(.traitColorGlyphs) {
            let mapped = (0..<count).map {
                ArtworkTextCapture.glyphRect(glyphs[$0], at: positions[$0], font: ctFont, textMatrix: textMatrix,
                                             ctm: context.ctm, height: context.height)
            }
            let ink = attributes[.foregroundColor] as? NSColor ?? .black
            let plane = alone(glyphs, positions: positions, count: count, font: font, textMatrix: textMatrix,
                              attributes: attributes, source: context, planes: 4, ink: ink)
            runs.append(GlyphInkRun(color: true, rects: mapped.compactMap { $0 }.filter { !$0.isNull },
                                    unbounded: mapped.contains { $0 == nil }, plane: plane))
            if drawsCoverage {
                for i in 0..<count {
                    var glyph = glyphs[i]
                    guard !CTFontGetBoundingRectsForGlyphs(ctFont, .default, &glyph, nil, 1).isEmpty else { continue }
                    let a8 = alone(glyphs.advanced(by: i), positions: positions.advanced(by: i), count: 1,
                                   font: font, textMatrix: textMatrix, attributes: white, source: context,
                                   planes: 1, ink: .white)
                    if a8.contains(where: { $0 != 0 }) { coverage.append(a8) }
                }
            }
        } else {
            for i in 0..<count {
                var glyph = glyphs[i]
                guard CTFontCreatePathForGlyph(ctFont, glyph, nil) == nil,
                      !CTFontGetBoundingRectsForGlyphs(ctFont, .default, &glyph, nil, 1).isEmpty else { continue }
                outlineLess += 1
                let a8 = alone(glyphs.advanced(by: i), positions: positions.advanced(by: i), count: 1,
                               font: font, textMatrix: textMatrix, attributes: white, source: context,
                               planes: 1, ink: .white)
                if a8.contains(where: { $0 != 0 }) { outlineLessInked += 1 }
            }
        }
        super.showCGGlyphs(glyphs, positions: positions, count: count, font: font,
            textMatrix: textMatrix, attributes: attributes, in: context)
    }

    /// The glyphs drawn alone by the superclass over the whole raster of
    /// `source`, `planes` bytes a pixel: straight sRGB RGBA8 (4) or A8 (1).
    private func alone(_ glyphs: UnsafePointer<CGGlyph>, positions: UnsafePointer<CGPoint>, count: Int,
        font: NSFont, textMatrix: CGAffineTransform, attributes: [NSAttributedString.Key: Any],
        source: CGContext, planes: Int, ink: NSColor) -> [UInt8] {
        let width = source.width, height = source.height
        var bytes = [UInt8](repeating: 0, count: width * height * planes)
        bytes.withUnsafeMutableBytes { buffer in
            let space = planes == 4 ? CGColorSpace(name: CGColorSpace.sRGB) : CGColorSpaceCreateDeviceGray()
            let info = planes == 4 ? CGImageAlphaInfo.premultipliedLast : .alphaOnly
            guard let space, let context = CGContext(data: buffer.baseAddress, width: width, height: height,
                bitsPerComponent: 8, bytesPerRow: width * planes, space: space, bitmapInfo: info.rawValue) else { return }
            context.concatenate(source.ctm)
            context.textMatrix = source.textMatrix
            context.textPosition = source.textPosition
            context.setShouldAntialias(true)
            context.setAllowsFontSmoothing(false)
            context.setFillColor(ink.cgColor)
            context.setStrokeColor(ink.cgColor)
            drawSuper(glyphs, positions: positions, count: count, font: font, textMatrix: textMatrix,
                      attributes: attributes, in: context)
        }
        if planes == 4 { Bitmap.unpremultiply(&bytes) }
        return bytes
    }

    private func drawSuper(_ glyphs: UnsafePointer<CGGlyph>, positions: UnsafePointer<CGPoint>, count: Int,
        font: NSFont, textMatrix: CGAffineTransform, attributes: [NSAttributedString.Key: Any], in context: CGContext) {
        NSGraphicsContext.saveGraphicsState()
        defer { NSGraphicsContext.restoreGraphicsState() }
        NSGraphicsContext.current = NSGraphicsContext(cgContext: context, flipped: true)
        super.showCGGlyphs(glyphs, positions: positions, count: count, font: font,
            textMatrix: textMatrix, attributes: attributes, in: context)
    }
}

/// How far ink in `plane` (`planes` bytes a pixel, alpha last) reaches past
/// the nearest of `rects`, each made integral, in whole pixels (the larger
/// of the x and y distances): 0 when every inked pixel lies in one of them.
/// Also the inked pixel count.
private func inkOvershoot(_ plane: [UInt8], planes: Int, width: Int, rects: [CGRect]) -> (overshoot: Int, inked: Int) {
    let boxes = rects.map { r -> (Int, Int, Int, Int) in
        (Int(r.minX.rounded(.down)), Int(r.minY.rounded(.down)), Int(r.maxX.rounded(.up)), Int(r.maxY.rounded(.up)))
    }
    var overshoot = 0, inked = 0
    var p = planes - 1, pixel = 0
    while p < plane.count {
        if plane[p] != 0 {
            inked += 1
            let x = pixel % width, y = pixel / width
            var nearest = Int.max
            for (x0, y0, x1, y1) in boxes {
                let dx = max(x0 - x, x - (x1 - 1), 0)
                let dy = max(y0 - y, y - (y1 - 1), 0)
                nearest = min(nearest, max(dx, dy))
                if nearest == 0 { break }
            }
            overshoot = max(overshoot, nearest)
        }
        p += planes
        pixel += 1
    }
    return (overshoot, inked)
}

private func sameBytes(_ a: [UInt8], _ b: [UInt8]) -> Bool {
    guard a.count == b.count else { return false }
    return a.withUnsafeBytes { left in
        b.withUnsafeBytes { right in
            guard let l = left.baseAddress, let r = right.baseAddress else { return left.count == right.count }
            return memcmp(l, r, left.count) == 0
        }
    }
}

private func intrinsicParts(_ drawing: NativeArtworkDrawing) -> [(bytes: [UInt8], window: GlyphWindow)] {
    drawing.parts.compactMap { part in
        guard case let .intrinsic(bytes, _, window) = part else { return nil }
        return (bytes, window)
    }
}

private func coverageParts(_ drawing: NativeArtworkDrawing) -> [(bytes: [UInt8], window: GlyphWindow)] {
    drawing.parts.compactMap { part in
        guard case let .coverage(bytes, _, window) = part else { return nil }
        return (bytes, window)
    }
}

private func probeFailure(_ message: String) -> DoctoredCoreError {
    DoctoredCoreError(message: "Glyph-window probe: \(message)")
}

/// Each part's window is smaller than the raster (no silent fallback to the
/// whole box) and its plane, embedded in zeros, equals `expected` exactly.
private func checkWindowed(_ parts: [(bytes: [UInt8], window: GlyphWindow)], expected: [[UInt8]], planes: Int,
                           drawing: NativeArtworkDrawing, _ what: String) throws {
    guard parts.count == expected.count, !parts.isEmpty else {
        throw probeFailure("\(what): \(parts.count) parts against \(expected.count) recorded")
    }
    for (k, part) in parts.enumerated() {
        guard part.window.pixels < drawing.width * drawing.height else {
            throw probeFailure("\(what): part \(k) fell back to the whole \(drawing.width) x \(drawing.height) raster")
        }
        let embedded = textCaptureEmbedded(part.bytes, window: part.window, planes: planes,
                                           width: drawing.width, height: drawing.height)
        guard sameBytes(embedded, expected[k]) else {
            throw probeFailure("\(what): part \(k) over \(part.window.json) differs from the glyphs drawn whole")
        }
    }
}

private let glyphCorpus = "😀👨‍👩‍👧‍👦🇫🇷👍🏽#️⃣"
private let identity: [Double] = [1, 0, 0, 1]
private let rotateShear: [Double] = [0.91, -0.17, 0.23, 0.86]
private let glyphTransforms: [(String, [Double])] = [
    ("identity", identity), ("scale_0.5", [0.5, 0, 0, 0.5]), ("scale_1.37", [1.37, 0, 0, 1.37]),
    ("scale_3", [3, 0, 0, 3]), ("rotate_shear", rotateShear),
]
private let fractionalAnchor = CGPoint(x: 8.25, y: 60.125)
private let glyphAnchors: [(String, CGPoint)] = [("fractional", fractionalAnchor), ("whole", CGPoint(x: 8, y: 60))]

private func glyphPayload(_ string: String, size: Double, map: [Double], family: String = "Helvetica Neue",
                          rgba: [Double] = [0.12345, 0.45678, 0.78901, 1.0]) throws -> TextLayerPayload {
    guard let color = ArtworkColor.color(rgba), let transform = LinearMap(array: map) else {
        throw probeFailure("invalid ink or transform")
    }
    var payload = TextLayerPayload(string: string, style: TextStyle(family: family, size: size), color: color,
                                   box: .point)
    payload.transform = transform
    return payload
}

/// The containment evidence and the exact gate. Over emoji (a plain one, a
/// ZWJ family, a flag, a skin-tone modifier, a keycap) at eight sizes, five
/// transforms (scale 3 up to 200 pt) and two anchors: how far any colour
/// run's ink reaches past its glyphs' reported rects, written to
/// `glyph-containment.json` before the gate runs; then each captured colour
/// part, embedded, equals its run drawn whole, from a window smaller than
/// the raster. Also, over the installed fonts that could reach capture's A8
/// branch, how many such glyphs exist and how many ink.
private func glyphContainment(directory: URL) throws -> [String: Any] {
    var rows: [[String: Any]] = []
    var cases: [(TextLayerPayload, CGPoint, GlyphInkRecorder)] = []
    var worst = 0
    for size in [9.0, 12, 17.5, 24, 48, 96, 200, 512] {
        for (transformName, map) in glyphTransforms where !(transformName == "scale_3" && size > 200) {
            for (anchorName, anchor) in glyphAnchors {
                let payload = try glyphPayload(glyphCorpus, size: size, map: map)
                let recorder = GlyphInkRecorder()
                _ = try textCaptureWithLayoutManager(recorder, payload: payload, anchor: anchor)
                guard let plan = TextLayer.drawingPlan(payload, anchor: anchor) else {
                    throw probeFailure("layout failed at \(size) pt \(transformName)")
                }
                let colour = recorder.runs.filter(\.color)
                guard !colour.isEmpty else { throw probeFailure("no colour run at \(size) pt \(transformName)") }
                var overshoot = 0, inked = 0, boxless = 0
                for run in colour {
                    let measured = inkOvershoot(run.plane, planes: 4, width: plan.width, rects: run.rects)
                    inked += measured.inked
                    // A run with a rect that is not finite, or with none, is
                    // captured over the whole raster: its ink is counted
                    // apart, never as an overshoot.
                    if run.unbounded || run.rects.isEmpty {
                        if measured.inked > 0 { boxless += 1 }
                        continue
                    }
                    overshoot = max(overshoot, measured.overshoot)
                }
                worst = max(worst, overshoot)
                rows.append(["size": size, "transform": transformName, "anchor": anchorName,
                             "colour_runs": colour.count, "glyph_rects": colour.reduce(0) { $0 + $1.rects.count },
                             "inked_pixels": inked, "max_ink_overshoot_px": overshoot,
                             "inked_runs_without_bounds": boxless, "raster": [plan.width, plan.height]])
                cases.append((payload, anchor, recorder))
            }
        }
    }
    var census: [[String: Any]] = []
    let families = Set(["Apple Chancery", "GB18030 Bitmap"]).union(
        NSFontManager.shared.availableFontFamilies.filter { $0.localizedCaseInsensitiveContains("bitmap") })
    for family in families.sorted() where TextStyle(family: family, size: 24).isInstalled {
        let payload = try glyphPayload("a\u{0338}漢字A\u{0338}", size: 24, map: identity, family: family)
        let recorder = GlyphInkRecorder()
        _ = try textCaptureWithLayoutManager(recorder, payload: payload, anchor: fractionalAnchor)
        census.append(["family": family, "outline_less_glyphs": recorder.outlineLess,
                       "outline_less_glyphs_that_ink": recorder.outlineLessInked])
    }
    var report: [String: Any] = ["name": "glyph_box_containment", "rows": rows, "row_count": rows.count,
        "max_ink_overshoot_px": worst, "glyph_margin": ArtworkTextCapture.glyphMargin,
        "margin_rule": "max(2, max_ink_overshoot_px + 1)", "a8_branch_census": census]
    try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys])
        .write(to: directory.appendingPathComponent("glyph-containment.json"))
    guard ArtworkTextCapture.glyphMargin >= max(2, worst + 1) else {
        throw probeFailure("ink reaches \(worst) px past its glyph rects; the margin is \(ArtworkTextCapture.glyphMargin)")
    }
    var windowPixels = 0, rasterPixels = 0
    for (payload, anchor, recorder) in cases {
        let captured = try ArtworkTextCapture.capture(payload: payload, anchor: anchor)
        let parts = intrinsicParts(captured)
        try checkWindowed(parts, expected: recorder.runs.filter(\.color).map(\.plane), planes: 4, drawing: captured,
                          "\(payload.size) pt \(payload.transform.array)")
        windowPixels += parts.reduce(0) { $0 + $1.window.pixels }
        rasterPixels += captured.width * captured.height * parts.count
    }
    report["exact_rows"] = cases.count
    report["window_pixels_over_raster_pixels"] = Double(windowPixels) / Double(max(rasterPixels, 1))
    report.removeValue(forKey: "rows")
    return report
}

/// Capture's bitmap-glyph (A8) branch, reached by drawing colour glyphs
/// through it (`Windowing.colorAsCoverage`): each glyph's coverage part,
/// embedded, equals that glyph drawn whole into A8, from a window smaller
/// than the raster.
private func coverageWindows() throws -> [String: Any] {
    var rows = 0, parts = 0
    for size in [12.0, 48, 200] {
        for (name, map) in [("identity", identity), ("rotate_shear", rotateShear)] {
            let payload = try glyphPayload(glyphCorpus, size: size, map: map)
            let recorder = GlyphInkRecorder(drawsCoverage: true)
            _ = try textCaptureWithLayoutManager(recorder, payload: payload, anchor: fractionalAnchor)
            let captured = try ArtworkTextCapture.capture(payload: payload, anchor: fractionalAnchor,
                windowing: ArtworkTextCapture.Windowing(colorAsCoverage: true))
            let coverage = coverageParts(captured)
            try checkWindowed(coverage, expected: recorder.coverage, planes: 1, drawing: captured,
                              "A8 \(size) pt \(name)")
            rows += 1
            parts += coverage.count
        }
    }
    return ["name": "coverage_glyph_windows", "rows": rows, "parts": parts]
}

/// The edge guard: with the margin narrowed to -4 px, "😀"'s window lies
/// inside its ink, which touches the window's ring, so capture draws the part
/// again over the whole raster, equal to the glyph drawn whole: ink is never
/// clipped. The colour part and the A8 part alike.
private func edgeGuard() throws -> [String: Any] {
    let payload = try glyphPayload("😀", size: 48, map: identity)
    let recorder = GlyphInkRecorder(drawsCoverage: true)
    _ = try textCaptureWithLayoutManager(recorder, payload: payload, anchor: fractionalAnchor)
    var results: [String: Any] = ["name": "glyph_window_edge_guard"]
    for (planes, windowing) in [(4, ArtworkTextCapture.Windowing(margin: -4)),
                                (1, ArtworkTextCapture.Windowing(margin: -4, colorAsCoverage: true))] {
        let captured = try ArtworkTextCapture.capture(payload: payload, anchor: fractionalAnchor, windowing: windowing)
        let parts = planes == 4 ? intrinsicParts(captured) : coverageParts(captured)
        let expected = planes == 4 ? recorder.runs.filter(\.color).map(\.plane) : recorder.coverage
        let whole = GlyphWindow(x: 0, y: 0, width: captured.width, height: captured.height)
        guard parts.count == 1, expected.count == 1, parts[0].window == whole,
              sameBytes(parts[0].bytes, expected[0]) else {
            throw probeFailure("edge guard (\(planes) planes): \(parts.map { $0.window.json }) is not the whole "
                + "\(captured.width) x \(captured.height) raster holding the glyph drawn whole")
        }
        results[planes == 4 ? "rgba_window" : "a8_window"] = parts[0].window.json
    }
    return results
}

/// Three lines of two emoji runs each, far apart under large tracking and a
/// fixed leading, so the text box is between 16 and 64 million pixels. Each
/// run's plane is charged its own window, not the box (a part over the box
/// would be 4 bytes a pixel, past the 64-million-sample component bound),
/// and the parts, placed in the raster, are the text drawn whole.
private func intrinsicLargeBox() throws -> [String: Any] {
    guard let color = ArtworkColor.color([0.12345, 0.45678, 0.78901, 1.0]) else {
        throw probeFailure("invalid ink")
    }
    let style = TextStyle(family: "Helvetica Neue", size: 48, tracking: 3000, leading: 1500)
    let payload = TextLayerPayload(string: "😀 😀\n😀 😀\n😀 😀", style: style, color: color, box: .point)
    let anchor = fractionalAnchor
    guard let plan = TextLayer.drawingPlan(payload, anchor: anchor) else { throw probeFailure("large-box layout failed") }
    let box = plan.width * plan.height
    guard box > 16_000_000, box <= 64_000_000 else {
        throw probeFailure("large-box raster is \(plan.width) x \(plan.height), not 16-64 MP")
    }
    let captured = try ArtworkTextCapture.capture(payload: payload, anchor: anchor)
    let parts = intrinsicParts(captured)
    guard parts.count == 6, parts.allSatisfy({ $0.bytes.count == $0.window.pixels * 4 }) else {
        throw probeFailure("large box: \(parts.count) colour parts, or a plane not its window's size")
    }
    let total = parts.reduce(0) { $0 + $1.bytes.count }
    guard total * 100 < box * 4 else {
        throw probeFailure("large box: the colour parts hold \(total) bytes against a \(box * 4)-byte box")
    }
    let evaluation = try NativeArtworkEvaluation(captured, float: false)
    guard evaluation.intrinsicCount == total else {
        throw probeFailure("large box: the evaluation charges \(evaluation.intrinsicCount) intrinsic bytes, not \(total)")
    }
    var composite = [UInt8](repeating: 0, count: box * 4)
    for (k, part) in parts.enumerated() {
        for other in parts[(k + 1)...] {
            let a = CGRect(x: part.window.x, y: part.window.y, width: part.window.width, height: part.window.height)
            let b = CGRect(x: other.window.x, y: other.window.y, width: other.window.width, height: other.window.height)
            guard !a.intersects(b) else { throw probeFailure("large box: two colour windows overlap") }
        }
        let row = part.window.width * 4
        for y in 0..<part.window.height {
            let target = ((part.window.y + y) * captured.width + part.window.x) * 4
            composite.replaceSubrange(target..<target + row, with: part.bytes[y * row..<(y + 1) * row])
        }
    }
    let reference = try textCaptureReference(payload: payload, anchor: anchor)
    guard sameBytes(composite, reference) else {
        throw probeFailure("large box: the placed colour parts differ from the text drawn whole")
    }
    return ["name": "intrinsic_large_box", "raster": [captured.width, captured.height], "box_bytes": box * 4,
            "colour_parts": parts.count, "colour_part_bytes": total, "windows": parts.map { $0.window.json }]
}

/// A small 8-bit document's ICC profile (the fixture's sRGB).
private func fixtureProfile() throws -> Data {
    guard let image = RasterImage.from(rgba: [UInt8](repeating: 0, count: 64 * 64 * 4), width: 64, height: 64),
          let byte = RasterDocument.from(image: image), let profile = byte.iccProfile else {
        throw probeFailure("could not create the document fixture")
    }
    return profile
}

/// The colour windows through the core: a 16-bit document's emoji text layer
/// (`RasterDocument.nativeArtwork`), read back as 8-bit codes, matches the
/// text drawn by TextKit within two codes of alpha and premultiplied colour,
/// so each colour part's `window` puts its plane where TextKit drew it.
private func documentWindows() throws -> [String: Any] {
    let width = 256, height = 160
    let profile = try fixtureProfile()
    let base = try RasterDocument.fromRawRGBA16([UInt16](repeating: 0, count: width * height * 4),
        width: width, height: height, profile: profile)
    let payload = try glyphPayload("😀👍🏽", size: 48, map: rotateShear)
    guard let plan = TextLayer.drawingPlan(payload, anchor: fractionalAnchor),
          let document = try base.nativeArtwork(.text(payload), anchor: fractionalAnchor, layer: 0,
              replacing: false, name: "Glyph windows"),
          let info = document.layerInfo(1), let source = document.precision?.layerSource(1) else {
        throw probeFailure("the emoji text layer was not made")
    }
    let samples = try source.withRGBA16 { Array($0) }
    var rendered = [UInt8](repeating: 0, count: plan.width * plan.height * 4)
    var outside = 0
    for ly in 0..<source.height {
        for lx in 0..<source.width {
            let s = (ly * source.width + lx) * 4
            let x = info.offsetX + lx - plan.offsetX, y = info.offsetY + ly - plan.offsetY
            guard x >= 0, y >= 0, x < plan.width, y < plan.height else {
                if samples[s + 3] != 0 { outside += 1 }
                continue
            }
            for c in 0..<4 { rendered[(y * plan.width + x) * 4 + c] = UInt8((Double(samples[s + c]) / 257).rounded()) }
        }
    }
    let reference = try textCaptureReference(payload: payload, anchor: fractionalAnchor)
    let difference = textCaptureDifference(reference, rendered, width: plan.width)
    let alpha = difference["max_alpha"] as? Int ?? 256
    let colour = difference["max_premultiplied_rgb"] as? Int ?? 256
    guard outside == 0, alpha <= 2, colour <= 2 else {
        throw probeFailure("the emoji layer differs from TextKit's draw: \(alpha) alpha, \(colour) colour codes, "
            + "\(outside) inked pixels outside the text box")
    }
    return ["name": "glyph_windows_in_a_document", "emoji_max_alpha": alpha, "emoji_max_premultiplied_rgb": colour]
}

/// The A8 window through the core: a drawing of one windowed coverage part
/// renders (`NativeArtworkEvaluation`) that coverage at its window, in its
/// ink, and nothing outside it.
private func coverageWindowRender() throws -> [String: Any] {
    let profile = try fixtureProfile()
    // An 8 x 6 ramp at (5, 3) of a 32 x 24 drawing.
    let window = GlyphWindow(x: 5, y: 3, width: 8, height: 6)
    let ramp = (0..<window.pixels).map { UInt8(5 + $0 * 5) }
    let ink = [0.2, 0.4, 0.6, 1.0]
    let drawing = NativeArtworkDrawing(width: 32, height: 24, offsetX: 0, offsetY: 0,
        parts: [.coverage(pixels: ramp, rgba: ink, window: window)])
    let evaluation = try NativeArtworkEvaluation(drawing, float: false)
    let rasterized = try PrecisionCapturedWork.run(scratchEstimate: evaluation.scratchBytes + UInt64(profile.count) * 64,
                                                   title: "Glyph window probe") {
        try evaluation.compute(profile: profile, control: $0)
    }
    let plane = try rasterized.withRGBA16 { Array($0) }
    var worst = 0
    for y in 0..<drawing.height {
        for x in 0..<drawing.width {
            let p = (y * drawing.width + x) * 4
            let inside = x >= window.x && x < window.x + window.width && y >= window.y && y < window.y + window.height
            let coverage = inside ? Int(ramp[(y - window.y) * window.width + x - window.x]) : 0
            worst = max(worst, abs(Int((Double(plane[p + 3]) / 257).rounded()) - coverage))
            if coverage > 0 {
                for c in 0..<3 {
                    worst = max(worst, abs(Int((Double(plane[p + c]) / 257).rounded()) - Int((ink[c] * 255).rounded())))
                }
            }
        }
    }
    guard worst <= 1 else { throw probeFailure("the A8 part renders \(worst) codes from its coverage at \(window.json)") }
    return ["name": "coverage_window_through_the_core", "a8_max_code_difference": worst]
}

/// Every glyph-window case; a failing one does not stop the others, and the
/// run fails listing each.
func runGlyphWindowProbe(directory: URL) throws -> [[String: Any]] {
    var results: [[String: Any]] = []
    var failures: [String] = []
    let cases: [(String, () throws -> [String: Any])] = [
        ("glyph_box_containment", { try glyphContainment(directory: directory) }),
        ("coverage_glyph_windows", coverageWindows), ("glyph_window_edge_guard", edgeGuard),
        ("intrinsic_large_box", intrinsicLargeBox), ("glyph_windows_in_a_document", documentWindows),
        ("coverage_window_through_the_core", coverageWindowRender),
    ]
    for (name, run) in cases {
        do { results.append(try run()) } catch { failures.append("\(name): \(error.localizedDescription)") }
    }
    guard failures.isEmpty else { throw DoctoredCoreError(message: failures.joined(separator: "\n")) }
    return results
}
