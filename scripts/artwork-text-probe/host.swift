#if !ARTWORK_CAPTURE_STANDALONE
import AppKit
import CoreText

/// Compile with the real app sources and comparison.swift. TextKit's hinted
/// raster is a layout oracle, not the area-coverage oracle for native outlines.
/// Before running this corpus, the declared layout allowance is two pixels and
/// 2% alpha area. Intrinsic pixels and authored colors use exact equality.
func runArtworkTextCaptureProbe(directory: URL) throws -> [String: Any] {
    let names = ["natural", "wrap", "center", "tight_leading", "tracking_baseline", "transform", "rtl",
                 "color_runs", "intrinsic_opaque", "intrinsic_opacity", "decorations", "decorations_transform", "decorations_tight"]
    var results: [[String: Any]] = []
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    for name in names {
        var style = TextStyle(family: "Helvetica Neue", size: 48, underline: true, strikethrough: true)
        var string = "A😀B👨‍👩‍👧‍👦C e\u{0301} fi\n漢字 مرحبا"
        var box = TextBox.width(600)
        var rgba = [0.12345, 0.45678, 0.78901, 1.0]
        if name == "wrap" { box = .width(170) }
        if name == "center" { style.alignment = .center }
        if name == "tight_leading" { style.leading = 20 }
        if name == "tracking_baseline" { style.tracking = 2.25; style.baselineShift = 3.75 }
        if name == "rtl" { string = "مرحبا بالعالم\nשלום"; style.alignment = .right }
        if name.hasPrefix("intrinsic") {
            string = "😀"; style.underline = false; style.strikethrough = false
            rgba[3] = name == "intrinsic_opaque" ? 1 : 0.37
        }
        if name.hasPrefix("decorations") {
            string = "😀👨‍👩‍👧‍👦\n😀"; rgba[3] = 0.37
            if name == "decorations_tight" { style.leading = 20; style.baselineShift = 3.75 }
        }
        guard let color = ArtworkColor.color(rgba) else { throw DoctoredCoreError(message: "Invalid probe ink") }
        var payload = TextLayerPayload(string: string, style: style, color: color, box: box)
        if name.contains("transform") {
            guard let map = LinearMap(array: [0.91, -0.17, 0.23, 0.86]) else {
                throw DoctoredCoreError(message: "Invalid probe transform")
            }
            payload.transform = map
        }
        if name == "color_runs" {
            payload.artwork?.runs = [ArtworkColorRun(location: 0, length: 1, rgba: [0.76, 0.12, 0.31, 0.613])]
        }
        let anchor = CGPoint(x: 8.25, y: 60.125)
        let captured = try ArtworkTextCapture.capture(payload: payload, anchor: anchor)
        let decorations = name.hasPrefix("decorations")
        let reference = try textCaptureReference(payload: payload, anchor: anchor, decorationsOnly: decorations)
        let rendered = try textCaptureRendered(captured, pathsOnly: decorations)
        var report = textCaptureDifference(reference, rendered, width: captured.width)
        report["name"] = name
        report["parts"] = captured.parts.count
        if ["color_runs", "tight_leading", "decorations_transform", "decorations_tight"].contains(name) {
            try textCaptureSavePNG(reference, width: captured.width, height: captured.height,
                url: directory.appendingPathComponent("\(name)-reference.png"))
            try textCaptureSavePNG(rendered, width: captured.width, height: captured.height,
                url: directory.appendingPathComponent("\(name)-captured.png"))
        }
        if name.hasPrefix("intrinsic") {
            var opaque = payload
            opaque.artwork?.rgba[3] = 1
            let original = try textCaptureReference(payload: opaque, anchor: anchor)
            let intrinsic = captured.parts.compactMap { part -> ([UInt8], Double)? in
                if case let .intrinsic(pixels, opacity) = part { return (pixels, opacity) }
                return nil
            }
            guard intrinsic.count == 1, intrinsic[0].0 == original, intrinsic[0].1 == rgba[3] else {
                throw DoctoredCoreError(message: "Intrinsic superclass pixels or independent opacity changed: \(name)")
            }
            report["intrinsic_exact_opaque_pixels"] = true
            report["authored_opacity_exact"] = true
        } else {
            let a = report["reference_alpha_area"] as? Int ?? 0
            let b = report["captured_alpha_area"] as? Int ?? 0
            let beyond = report["ink_pixels_outside_two_pixel_reference_neighborhood"] as? Int ?? -1
            guard a > 0, abs(Double(a - b)) / Double(a) <= 0.02, beyond == 0 else {
                throw DoctoredCoreError(message: "TextKit outline/decoration layout gate failed: \(name); \(report)")
            }
            if decorations {
                guard (report["reference_bounds"] as? [Int]) == (report["captured_bounds"] as? [Int]) else {
                    throw DoctoredCoreError(message: "Superclass decoration extent changed: \(name)")
                }
                guard (report["max_alpha"] as? Int ?? 256) <= 2 else {
                    throw DoctoredCoreError(message: "Superclass decoration coverage differs beyond two byte alpha codes: \(name)")
                }
            }
            if name == "color_runs" {
                let inks = captured.parts.compactMap { part -> [Double]? in
                    switch part { case let .path(_, rgba, _), let .coverage(_, rgba), let .fill(rgba, _): return rgba; case .intrinsic, .gradient, .pattern: return nil }
                }
                guard inks.contains([0.76, 0.12, 0.31, 0.613]), inks.contains(rgba) else {
                    throw DoctoredCoreError(message: "Continuous attributed ink was rounded during capture")
                }
                report["continuous_attributed_ink_exact"] = true
            }
        }
        results.append(report)
    }
    results.append(try outlineLessNoInk())
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    results += try editorIntrinsicParity(directory: directory)
    results.append(try artworkDeniedReadIntegrity())
    let report: [String: Any] = ["passed": true, "groups": results, "group_count": results.count,
        "layout_tolerance": "Two pixels and 2% alpha area against hinted TextKit; intrinsic RGBA8/opacity and attributed ink exact"]
    let data = try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys])
    try data.write(to: directory.appendingPathComponent("artwork-text-capture.json"))
    return report
}

/// Counts, before drawing them, the glyphs capture sends down its
/// outline-less monochrome branch: a non-color font, no outline, non-empty
/// bounds (`TextArtworkLayout.showCGGlyphs`'s own predicate).
private final class OutlineLessGlyphCounter: NSLayoutManager {
    var fallbackGlyphs = 0
    override init() { super.init() }
    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("OutlineLessGlyphCounter does not support NSCoder") }
    override func showCGGlyphs(_ glyphs: UnsafePointer<CGGlyph>, positions: UnsafePointer<CGPoint>, count: Int,
        font: NSFont, textMatrix: CGAffineTransform, attributes: [NSAttributedString.Key: Any], in context: CGContext) {
        let ctFont = font as CTFont
        if !CTFontGetSymbolicTraits(ctFont).contains(.traitColorGlyphs) {
            for i in 0..<count {
                var glyph = glyphs[i]
                if CTFontCreatePathForGlyph(ctFont, glyph, nil) == nil,
                   !CTFontGetBoundingRectsForGlyphs(ctFont, .default, &glyph, nil, 1).isEmpty {
                    fallbackGlyphs += 1
                }
            }
        }
        super.showCGGlyphs(glyphs, positions: positions, count: count, font: font,
            textMatrix: textMatrix, attributes: attributes, in: context)
    }
}

/// Apple Chancery's U+0338 is an outline-less, non-color glyph
/// that draws no ink. Capture must drop it rather than add an empty A8 part
/// per mark — 150 of them used to exceed the 256 ordered parts — so the
/// capture of "a̸" × 150 has exactly the parts of "a" × 150 and passes the
/// standard layout gate.
private func outlineLessNoInk() throws -> [String: Any] {
    let style = TextStyle(family: "Apple Chancery", size: 24)
    guard style.isInstalled else { throw DoctoredCoreError(message: "Apple Chancery is not installed") }
    guard let color = ArtworkColor.color([0.12345, 0.45678, 0.78901, 1.0]) else {
        throw DoctoredCoreError(message: "Invalid probe ink")
    }
    let anchor = CGPoint(x: 8.25, y: 60.125)
    let payload = TextLayerPayload(string: String(repeating: "a\u{0338}", count: 150), style: style,
        color: color, box: .width(300))
    let counter = OutlineLessGlyphCounter()
    _ = try textCaptureWithLayoutManager(counter, payload: payload, anchor: anchor)
    guard counter.fallbackGlyphs >= 1 else {
        throw DoctoredCoreError(message: "No outline-less non-color glyph reached the fallback branch")
    }
    let captured = try ArtworkTextCapture.capture(payload: payload, anchor: anchor)
    let coverage = captured.parts.filter { part in
        if case .coverage = part { return true }
        return false
    }
    guard coverage.isEmpty else {
        throw DoctoredCoreError(message: "An outline-less glyph that draws no ink kept \(coverage.count) A8 part(s)")
    }
    let plain = TextLayerPayload(string: String(repeating: "a", count: 150), style: style,
        color: color, box: .width(300))
    let withoutMarks = try ArtworkTextCapture.capture(payload: plain, anchor: anchor)
    guard captured.parts.count == withoutMarks.parts.count else {
        throw DoctoredCoreError(message: "Dropped marks changed the part count: "
            + "\(captured.parts.count) against \(withoutMarks.parts.count)")
    }
    let reference = try textCaptureReference(payload: payload, anchor: anchor)
    let rendered = try textCaptureRendered(captured)
    var report = textCaptureDifference(reference, rendered, width: captured.width)
    let a = report["reference_alpha_area"] as? Int ?? 0
    let b = report["captured_alpha_area"] as? Int ?? 0
    let beyond = report["ink_pixels_outside_two_pixel_reference_neighborhood"] as? Int ?? -1
    guard a > 0, abs(Double(a - b)) / Double(a) <= 0.02, beyond == 0 else {
        throw DoctoredCoreError(message: "TextKit outline layout gate failed: outline_less_no_ink; \(report)")
    }
    report["name"] = "outline_less_no_ink"
    report["fallback_glyphs"] = counter.fallbackGlyphs
    report["parts"] = captured.parts.count
    report["parts_without_marks"] = withoutMarks.parts.count
    return report
}

private func editorIntrinsicParity(directory: URL) throws -> [[String: Any]] {
    let view = ArtworkTextEditing.makeView(frame: CGRect(x: 0, y: 0, width: 400, height: 200))
    guard view.layoutManager is ArtworkEditingLayoutManager, let storage = view.textStorage else {
        throw DoctoredCoreError(message: "The actual editor factory did not install intrinsic-opacity drawing")
    }
    var results: [[String: Any]] = []
    for (name, opacity) in [("editor_intrinsic_translucent", 0.37), ("editor_intrinsic_transparent", 0.0),
                            ("editor_intrinsic_runs", 0.37), ("editor_intrinsic_transform", 0.61)] {
        let style = TextStyle(family: "Helvetica Neue", size: 48)
        guard let ink = ArtworkColor.color([0.12, 0.34, 0.56, opacity]) else { throw DoctoredCoreError(message: "Invalid editor probe color") }
        var payload = TextLayerPayload(string: "😀😀", style: style, color: ink, box: .point)
        if name == "editor_intrinsic_runs" {
            payload.artwork?.runs = [ArtworkColorRun(location: 2, length: 2, rgba: [0.8, 0.1, 0.2, 0.823])]
        }
        if name == "editor_intrinsic_transform", let map = LinearMap(array: [0.93, -0.16, 0.21, 0.87]) { payload.transform = map }
        let anchor = CGPoint(x: 8.25, y: 60.125)
        let drawing = try ArtworkTextCapture.capture(payload: payload, anchor: anchor)
        let rendered = try textCaptureRendered(drawing)
        let editor = try textCaptureWithLayoutManager(ArtworkEditingLayoutManager(), payload: payload, anchor: anchor)
        var report = textCaptureDifference(editor, rendered, width: drawing.width)
        guard (report["max_alpha"] as? Int ?? 256) <= 2,
              (report["max_premultiplied_rgb"] as? Int ?? 256) <= 2 else {
            throw DoctoredCoreError(message: "Actual editor intrinsic opacity differs from final capture: \(name); \(report)")
        }
        report["name"] = name
        results.append(report)
        if name == "editor_intrinsic_runs" {
            try textCaptureSavePNG(editor, width: drawing.width, height: drawing.height,
                url: directory.appendingPathComponent("editor-intrinsic-runs.png"))
            try textCaptureSavePNG(rendered, width: drawing.width, height: drawing.height,
                url: directory.appendingPathComponent("final-intrinsic-runs.png"))
        }
    }
    var style = TextStyle(family: "Helvetica Neue", size: 48, tracking: 1.25, underline: true)
    let base = NSColor(srgbRed: 0.12, green: 0.34, blue: 0.56, alpha: 0.37)
    var payload = TextLayerPayload(string: "A😀B", style: style, color: base, box: .width(400))
    payload.artwork?.runs = [ArtworkColorRun(location: 1, length: 2, rgba: [0.8, 0.1, 0.2, 0.823])]
    storage.setAttributedString(payload.attributedString(style: style))
    view.typingAttributes = style.attributes(color: base)
    view.setSelectedRange(NSRange(location: 1, length: 2))
    let selectedTypingInk = (view.typingAttributes[.foregroundColor] as? NSColor).flatMap(ArtworkColor.rgba)
    style.tracking = 0; style.underline = false; style.baselineShift = 2.5
    ArtworkTextEditing.restyle(view, style: style, ink: base)
    let preserved = ArtworkTextEditing.payload(view: view, style: style, ink: base, width: 400)
    guard preserved.artwork?.runs == payload.artwork?.runs,
          storage.attribute(.kern, at: 0, effectiveRange: nil) == nil,
          storage.attribute(.underlineStyle, at: 0, effectiveRange: nil) == nil else {
        throw DoctoredCoreError(message: "Editor typography changed attributed colors or retained removed attributes; "
            + "before runs=\(String(describing: payload.artwork?.runs)); after runs=\(String(describing: preserved.artwork?.runs)); "
            + "selected typing ink=\(String(describing: selectedTypingInk)); "
            + "kern=\(String(describing: storage.attribute(.kern, at: 0, effectiveRange: nil))); "
            + "underline=\(String(describing: storage.attribute(.underlineStyle, at: 0, effectiveRange: nil)))")
    }
    let changed = NSColor(srgbRed: 0.17, green: 0.27, blue: 0.47, alpha: 0.617)
    ArtworkTextEditing.restyle(view, style: style, ink: changed)
    guard let first = storage.attribute(.foregroundColor, at: 0, effectiveRange: nil) as? NSColor,
          let selected = storage.attribute(.foregroundColor, at: 1, effectiveRange: nil) as? NSColor,
          let last = storage.attribute(.foregroundColor, at: 3, effectiveRange: nil) as? NSColor,
          ArtworkColor.rgba(first) == ArtworkColor.rgba(base), ArtworkColor.rgba(last) == ArtworkColor.rgba(base),
          ArtworkColor.rgba(selected) == ArtworkColor.rgba(changed) else {
        throw DoctoredCoreError(message: "Editor color change escaped the selected composed run")
    }
    results.append(["name": "actual_editor_factory_typography_and_selected_run_color", "passed": true])
    return results
}

/// A reservation consumes accounting headroom without allocating a giant
/// fixture. Presence must survive a refused metadata read, and operations
/// that need that metadata must refuse before publishing a raster-only result.
private func artworkDeniedReadIntegrity() throws -> [String: Any] {
    func check(_ condition: @autoclosure () -> Bool, _ message: String) throws {
        guard condition() else { throw DoctoredCoreError(message: "Artwork denied-read probe: \(message)") }
    }
    func refuses(_ name: String, _ operation: () throws -> Void) throws {
        do { try operation() }
        catch { return }
        throw DoctoredCoreError(message: "Artwork denied-read probe: \(name) did not refuse")
    }
    let width = 64, height = 64
    guard let image = RasterImage.from(rgba: [UInt8](repeating: 0, count: width * height * 4),
        width: width, height: height), let byte = RasterDocument.from(image: image),
        let profile = byte.iccProfile else {
        throw DoctoredCoreError(message: "Could not create the small artwork admission fixture")
    }
    let base = try RasterDocument.fromRawRGBA16([UInt16](repeating: 0, count: width * height * 4),
        width: width, height: height, profile: profile)
    let payload = ShapeLayerPayload(kind: "rect", w: 12.25, h: 9.5,
        fill: NSColor(srgbRed: 0.1234567, green: 0.3456789, blue: 0.5678912, alpha: 0.6123456),
        stroke: nil, strokeWidth: 0)
    guard let source = try base.nativeArtwork(.shape(payload), anchor: CGPoint(x: 8, y: 8),
        layer: 0, replacing: false, name: "Admission fixture"),
        let metadata = source.layerMeta(1), let withoutDescription = source.withLayerMeta(1, nil) else {
        throw DoctoredCoreError(message: "Could not prepare the retained artwork admission fixture")
    }
    let fingerprint = source.layerFingerprint(1)
    let restored = try source.restoringArtworkDescription(metadata, layer: 1)
    try check(restored === source, "restoring an identical readable description must preserve identity")
    let document = ImageDocument()
    document.fileType = ImageDocument.nativeTypeIdentifier
    document.installEditingState(EditingState(document: source, selection: SelectionHistoryState(),
        layerSelection: .single(1), paintTarget: .layer, jpegExportQuality: 90,
        embedColorProfile: true, stripMetadata: false, exportBitDepth: 16,
        exportDither: "none"), markSaved: true)
    defer { document.close() }
    let available = try RasterResources.shared.admit(scratch: 0)
    try check(available > 1, "fixture has no initial working allowance")
    var reservation: PrecisionEditPayload? = try PrecisionEditPayload.admitted(bytes: available - 1)
    try withExtendedLifetime(reservation) {
        try check(source.layerMeta(1) == nil, "metadata read must actually be denied")
        try check(source.hasArtworkDescription(1) && source.artworkIndices == [1]
            && document.layerDescribesSource(1), "denied reading erased retained-artwork presence")
        func namesUnreadable(_ name: String, _ operation: () throws -> RasterDocument?) throws {
            do {
                _ = try operation()
            } catch {
                try check(error.localizedDescription.contains("could not be read"),
                    "\(name) must name the unreadable retained description")
                return
            }
            throw DoctoredCoreError(message: "Artwork denied-read probe: \(name) did not refuse")
        }
        try namesUnreadable("crop") { try source.cropped(x: 0, y: 0, w: 48, h: 48) }
        try namesUnreadable("canvas size") {
            try source.canvasResized(w: 80, h: 80, originX: 0, originY: 0)
        }
        try check(document.doc === source, "a named geometry refusal changed the document")
        try refuses("integral affine") {
            _ = try source.artworkAffine(1, matrix: CGAffineTransform(translationX: 1, y: 0),
                sampler: DR_FILTER_NEAREST)
        }
        try refuses("profile assignment") { _ = try source.artworkProfile(profile, convert: false) }
        try refuses("missing-description restoration") {
            _ = try withoutDescription.restoringArtworkDescription(metadata, layer: 1)
        }
        try check(document.doc === source && !document.isDocumentEdited,
            "refused work changed the installed document or dirty state")
    }
    reservation = nil
    try check(source.layerMeta(1) == metadata && source.layerFingerprint(1) == fingerprint,
        "refused work changed the original pixels or description")
    try check(withoutDescription.layerMeta(1) == nil && !withoutDescription.hasArtworkDescription(1),
        "refused restoration mutated its input")
    return ["name": "typed_artwork_presence_and_atomic_refusal_under_denied_metadata_read", "passed": true,
        "accounting_reservation_bytes": available - 1, "fixture_canvas_pixels": width * height,
        "denied_operations": ["metadata_read", "crop", "integral_affine", "profile_assignment", "metadata_restoration"]]
}
#endif
