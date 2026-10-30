import AppKit

// Type, shape and fill layers on a Grayscale document (#547 phase 6a-i). A Gray document stores
// each colour of a description as its gray by Image ▸ Mode ▸ Grayscale's rule — a run's, a shape's
// fill and stroke, a fill layer's colour and its wide `linear`, a gradient stop's hex and wide
// `stop_linears`, a custom pattern tile's texels — as its layer effects do (`gray_style`), so the
// stored description, `get_document` and the render agree. The core decides each gray
// (`dr_precision_doc_gray_artwork_colors`, `core/src/gray_artwork.rs`); the RGB renderer then runs
// unchanged in the document's twin profile and lands on R = G = B, and the core folds any pixel
// that still carries colour (a colour glyph) at publication. `nativeArtwork`'s Gray arm
// (NativeArtworkEvaluation.swift) is the one entry: the Type and shape tools, the Fill Layer and
// Artwork Colors sheets, MCP add/edit, geometry re-renders and `reinkingArtwork` all reach it.

extension RasterDocument {
    /// `description` with every colour as its gray (module comment); unchanged off a Gray
    /// document. One core call per description.
    func grayArtwork(_ description: LayerDescription) throws -> LayerDescription {
        guard isGray, let precision, description.artworkInk != .cmyk else { return description }
        let palette = description.palette
        let linearInk = description.artworkInk == .linearSRGB
        var fillLinear: [Double]?
        var stopLinears: [[Double]]?
        var pattern: PatternRef?
        if case let .fill(payload) = description {
            fillLinear = payload.linear
            stopLinears = payload.stopLinears
            pattern = payload.patternFill?.pattern.flatMap { $0.isCustom ? $0 : nil }
        }
        // The `linear` list: a linear-ink palette, then a fill's `linear`, then its stop linears.
        var linear: [[Double]] = linearInk ? palette.colors : []
        let fillLinearAt = linear.count
        if let fillLinear { linear.append(fillLinear) }
        let stopLinearsAt = linear.count
        linear += stopLinears ?? []
        // sRGB numbers outside 0…1 render clamped at 8 and 16 bits; the request takes them so.
        let srgb = linearInk ? [] : palette.colors.map { $0.map { min(max($0, 0), 1) } }
        var request: [String: Any] = [:]
        if !srgb.isEmpty { request["srgb"] = srgb }
        if !linear.isEmpty { request["linear"] = linear }
        if let pattern { request["pattern"] = Self.grayPatternObject(pattern) }
        guard !request.isEmpty else { return description }
        let answer = try precision.grayArtworkColors(request)
        let greyedLinear = (answer["linear"] as? [[Double]]) ?? []
        let greyedSRGB = (answer["srgb"] as? [[Double]]) ?? []
        guard greyedLinear.count == linear.count, greyedSRGB.count == srgb.count else {
            throw DoctoredCoreError(message: "The Grayscale artwork colours were incomplete.")
        }
        let greyed = linearInk ? Array(greyedLinear.prefix(palette.colors.count)) : greyedSRGB
        var result = palette.colors.isEmpty
            ? description
            : description.repainted(greyed, appearance: greyed, ink: description.artworkInk)
        if case var .fill(payload) = result {
            // `repainted` clears the wide slots a changed colour or stop drops; they are written
            // back greyed, after it.
            if fillLinear != nil { payload.linear = greyedLinear[fillLinearAt] }
            if let stopLinears {
                payload.stopLinears = Array(greyedLinear[stopLinearsAt..<(stopLinearsAt + stopLinears.count)])
            }
            if pattern != nil {
                guard let object = answer["pattern"] as? [String: Any] else {
                    throw DoctoredCoreError(message: "The Grayscale artwork colours were incomplete.")
                }
                payload.patternFill?.pattern = Self.grayPattern(object)
            }
            result = .fill(payload)
        }
        return result
    }

    /// `color` as its gray by the Mode rule (`grayArtworkColors`' `srgb` list), alpha kept: the
    /// Text session's live preview ink on a Gray document (#547 phase 6b). `color` unchanged off
    /// a Gray document or when the core cannot answer.
    func grayDisplayColor(_ color: NSColor) -> NSColor {
        guard isGray, let precision, let srgb = color.usingColorSpace(.sRGB) else { return color }
        let rgb = [srgb.redComponent, srgb.greenComponent, srgb.blueComponent]
            .map { min(max(Double($0), 0), 1) }
        guard let answer = try? precision.grayArtworkColors(["srgb": [rgb]]),
              let gray = (answer["srgb"] as? [[Double]])?.first, gray.count == 3 else { return color }
        return NSColor(srgbRed: CGFloat(gray[0]), green: CGFloat(gray[1]), blue: CGFloat(gray[2]),
                       alpha: srgb.alphaComponent)
    }

    /// A custom `PatternRef` as the core's pattern object.
    private static func grayPatternObject(_ pattern: PatternRef) -> [String: Any] {
        var object: [String: Any] = [:]
        if let name = pattern.name { object["name"] = name }
        if let width = pattern.width { object["width"] = width }
        if let height = pattern.height { object["height"] = height }
        if let rgba = pattern.rgba { object["rgba"] = rgba }
        return object
    }

    /// The core's pattern object as a `PatternRef`.
    private static func grayPattern(_ object: [String: Any]) -> PatternRef {
        PatternRef(preset: object["preset"] as? String, name: object["name"] as? String,
                   width: object["width"] as? Int, height: object["height"] as? Int,
                   rgba: object["rgba"] as? String)
    }
}
