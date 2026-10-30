import AppKit

/// The parameters a SHAPE LAYER's pixels were rendered from: the description
/// is the source of truth, the raster only its cache. Like a text layer's
/// payload it is serialized as JSON into the core's opaque per-layer metadata
/// slot — the core stores, copies and persists those bytes but never parses
/// them, so the schema and its versioning belong entirely to this side of the
/// FFI.
///
/// Geometry is layer-LOCAL, deliberately: the payload stores only the shape
/// box's SIZE (`w` × `h`), its styling and its 2×2 `transform`, never a
/// canvas position. Where the shape sits on the canvas is the layer's offset
/// (the anchor rule in DescribedLayer.swift: the box's top-left is the
/// source origin), like every other layer — which is what lets the Move tool
/// move a shape layer without invalidating its description, and keeps the
/// description true after any offset-only edit.
///
/// JSON shape, version 1: `{"type":"shape","version":1,"kind":"rect"|
/// "ellipse"|"line","w":…,"h":…,"flipped":bool?,"fill":"#RRGGBBAA"|"",
/// "stroke":"#RRGGBBAA"|"","strokeWidth":…,"radius":…}`. `flipped` is
/// additive and optional (missing = false). Version 2 adds `transform`
/// (`[a, b, c, d]`, row-major: x′ = a·x + b·y, y′ = c·x + d·y — see
/// `LinearMap`) and
/// `origin_frac` (`[fx, fy]`), and is written only when either is off its
/// default; a version-1 payload decodes as the identity, so untransformed
/// layers keep opening as shapes in older builds.
///
/// Version 3 adds `artwork` (continuous fill/stroke, `ArtworkShapeStyle`).
/// Version 4 is version 3 whose `artwork` carries
/// `"ink":"linear_srgb"`: the numbers, and the legacy hex `fill`/`stroke`
/// (the numbers ×255, clamped to 0–1), are linear light, written in a 32-bit
/// float document; red, green and blue may be above white, up to 65504.
/// Version 3 has no `ink`; any other `ink` is "not a shape layer".
///
/// Version 5 is version 3 or 4 plus the stroke styles `strokeJoin`
/// (`miter`|`round`|`bevel`), `strokeDash` (`none`|`dashed`|`dotted`) and
/// `strokeAlign` (`inside`|`center`|`outside`), each written only when off its
/// default. It always carries `artwork`, whose `ink` says what the numbers are
/// (absent = sRGB, `linear_srgb` = linear light), and is written only when a
/// style is off its default: a default-style shape keeps encoding as 1–4.
/// Below version 5 the three keys are ignored and the styles are the defaults.
///
/// Version 6 (#547) is version 5 whose `artwork` carries `"ink":"cmyk"`: its
/// `fill` and `stroke` are `[c, m, y, k, alpha]` in the CMYK document's
/// profile, and the hex `fill`/`stroke` are that ink's sRGB APPEARANCE through
/// the profile (CmykArtwork.swift). An ink payload is always version 6.
struct ShapeLayerPayload: Codable, Equatable {
    /// The only `type` this app understands; anything else (or nothing) means
    /// the layer's metadata was written by something that is not a shape
    /// layer, and the layer is a plain raster layer.
    static let typeName = "shape"
    /// The newest schema this app writes; older builds read layers at it as
    /// plain rasters — the graceful degradation the format is designed for.
    static let currentVersion = 6
    /// The stroke-style schema and the ink one (#547).
    static let styledVersion = 5
    static let cmykVersion = 6
    /// The schema a linear-ink payload with default stroke styles is written
    /// at (version 3 for sRGB ink): what version 4 has always meant.
    static let linearVersion = 4
    /// The schema written when the transform is the identity and the anchor
    /// is whole-pixel.
    static let legacyVersion = 1
    /// The values `kind` may take.
    static let kinds = ["rect", "ellipse", "line"]

    /// "rect" | "ellipse" | "line".
    var artwork: ArtworkShapeStyle?
    var kind: String
    /// Shape box size in pixels — the SHAPE's box, not the raster's (the
    /// raster is the box plus `ShapeLayer.padding(for:)` on every side,
    /// mapped through the transform).
    var w: Double
    var h: Double
    /// Line only: false = the line runs from the box's top-left to its
    /// bottom-right; true = bottom-left to top-right. Meaningless (and always
    /// encoded false) for rect/ellipse.
    var flipped: Bool
    /// Straight alpha, "#RRGGBBAA" in the payload's ink (sRGB, or linear
    /// light at version 4); "" = no fill. Always "" for lines.
    var fill: String
    /// Straight alpha, "#RRGGBBAA" in the payload's ink; "" = no stroke.
    var stroke: String
    /// Stroke width in pixels, centered on the path (in SOURCE space: a
    /// scaling transform scales it too).
    var strokeWidth: Double
    /// Rect corner radius in pixels; ignored by ellipse/line.
    var radius: Double
    /// The 2×2 linear part of the layer's placement (DescribedLayer.swift).
    var transform: LinearMap = .identity
    /// The fraction of the anchor, each component in [0, 1); written only
    /// by the described-layer commit ops, never by a caller.
    var originFraction: CGPoint = .zero
    /// The stroke styles (ShapeStrokeStyle.swift). `strokeAlign` is
    /// always `.center` for a line.
    var strokeJoin: ShapeStrokeJoin = .miter
    var strokeDash: ShapeStrokeDash = .none
    var strokeAlign: ShapeStrokeAlign = .center

    enum CodingKeys: String, CodingKey {
        case type, version, kind, w, h, flipped, fill, stroke, strokeWidth, radius, transform, artwork
        case strokeJoin, strokeDash, strokeAlign
        case originFraction = "origin_frac"
    }

    /// Builds a payload with canonical bytes: `fill` is forced to "" for
    /// lines, `flipped` to false for rect/ellipse, `radius` to 0 for
    /// anything but a rect and `strokeAlign` to centre for a line, so
    /// re-encoding never carries fields the kind ignores.
    init(
        kind: String, w: Double, h: Double, flipped: Bool = false,
        fill: NSColor?, stroke: NSColor?, strokeWidth: Double, radius: Double = 0,
        transform: LinearMap = .identity, ink: ArtworkInk = .srgb,
        strokeJoin: ShapeStrokeJoin = .miter, strokeDash: ShapeStrokeDash = .none,
        strokeAlign: ShapeStrokeAlign = .center
    ) {
        self.kind = kind
        let isLine = kind == "line"
        let fillNumbers = isLine ? nil : fill.map { ArtworkColor.authored($0, in: ink) }
        let strokeNumbers = stroke.map { ArtworkColor.authored($0, in: ink) }
        self.artwork = ArtworkShapeStyle(fill: fillNumbers, stroke: strokeNumbers, in: ink)
        self.w = w
        self.h = h
        self.flipped = isLine ? flipped : false
        self.fill = fillNumbers.map(TextLayer.hex(numbers:)) ?? ""
        self.stroke = strokeNumbers.map(TextLayer.hex(numbers:)) ?? ""
        self.strokeWidth = strokeWidth
        self.radius = kind == "rect" ? radius : 0
        self.transform = transform
        self.strokeJoin = strokeJoin
        self.strokeDash = strokeDash
        self.strokeAlign = isLine ? .center : strokeAlign
    }

    /// Decoding validates `type` and `version` (neither is stored: encoding
    /// writes them back); a missing `flipped`, `transform` or `origin_frac`
    /// reads as its default, a malformed one throws — `decode` turns every
    /// throw into "not a shape layer".
    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let type = try container.decode(String.self, forKey: .type)
        let version = try container.decode(Int.self, forKey: .version)
        guard type == Self.typeName, (Self.legacyVersion...Self.currentVersion).contains(version)
        else {
            throw DecodingError.dataCorruptedError(
                forKey: .version, in: container, debugDescription: "not a shape payload")
        }
        kind = try container.decode(String.self, forKey: .kind)
        artwork = try container.decodeIfPresent(ArtworkShapeStyle.self, forKey: .artwork)
        // Version 5 takes its ink from `artwork.ink`; the version-4 equality
        // (4 = linear, 3 = sRGB) applies only to versions 3 and 4.
        guard (version >= 3) == (artwork != nil), artwork?.ink != .srgb,
              version >= 5 || (version == Self.linearVersion) == (artwork?.ink == .linearSRGB),
              (version == Self.cmykVersion) == (artwork?.ink == .cmyk)
        else {
            throw DecodingError.dataCorruptedError(forKey: .artwork, in: container,
                debugDescription: "native artwork semantics require version 3 (sRGB), 4 (linear) or 5")
        }
        if version >= 5 {
            strokeJoin = try container.decodeIfPresent(ShapeStrokeJoin.self, forKey: .strokeJoin) ?? .miter
            strokeDash = try container.decodeIfPresent(ShapeStrokeDash.self, forKey: .strokeDash) ?? .none
            strokeAlign = try container.decodeIfPresent(ShapeStrokeAlign.self, forKey: .strokeAlign)
                ?? .center
        }
        w = try container.decode(Double.self, forKey: .w)
        h = try container.decode(Double.self, forKey: .h)
        flipped = try container.decodeIfPresent(Bool.self, forKey: .flipped) ?? false
        fill = try container.decode(String.self, forKey: .fill)
        stroke = try container.decode(String.self, forKey: .stroke)
        strokeWidth = try container.decode(Double.self, forKey: .strokeWidth)
        radius = try container.decode(Double.self, forKey: .radius)
        if let elements = try container.decodeIfPresent([Double].self, forKey: .transform) {
            guard let map = LinearMap(array: elements) else {
                throw DecodingError.dataCorruptedError(
                    forKey: .transform, in: container, debugDescription: "malformed transform")
            }
            transform = map
        }
        if let pair = try container.decodeIfPresent([Double].self, forKey: .originFraction) {
            guard let frac = TextLayer.originFraction(from: pair) else {
                throw DecodingError.dataCorruptedError(
                    forKey: .originFraction, in: container,
                    debugDescription: "malformed origin_frac")
            }
            originFraction = frac
        }
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(Self.typeName, forKey: .type)
        let legacy = isLegacyEncodable
        // A styled stroke is version 5, which always carries `artwork`: a
        // payload that somehow has none takes it from its sRGB hex colours.
        let styled = hasStrokeStyle
        let style = self.artwork ?? (styled ? withStrokeStyle(
            join: strokeJoin, dash: strokeDash, align: strokeAlign).artwork : nil)
        let version = style?.ink == .cmyk ? Self.cmykVersion
            : styled ? Self.styledVersion
            : style.map { $0.ink == .linearSRGB ? Self.linearVersion : 3 }
                ?? (legacy ? Self.legacyVersion : 2)
        try container.encode(version, forKey: .version)
        try container.encodeIfPresent(style, forKey: .artwork)
        if strokeJoin != .miter { try container.encode(strokeJoin, forKey: .strokeJoin) }
        if strokeDash != .none { try container.encode(strokeDash, forKey: .strokeDash) }
        if strokeAlign != .center { try container.encode(strokeAlign, forKey: .strokeAlign) }
        try container.encode(kind, forKey: .kind)
        try container.encode(w, forKey: .w)
        try container.encode(h, forKey: .h)
        try container.encode(flipped, forKey: .flipped)
        try container.encode(fill, forKey: .fill)
        try container.encode(stroke, forKey: .stroke)
        try container.encode(strokeWidth, forKey: .strokeWidth)
        try container.encode(radius, forKey: .radius)
        guard !legacy else { return }
        try container.encode(transform.array, forKey: .transform)
        try container.encode(TextLayer.originFractionArray(originFraction), forKey: .originFraction)
    }

    /// True when the transform is the identity and the anchor whole-pixel,
    /// so the payload can be written as version 1.
    var isLegacyEncodable: Bool {
        artwork == nil && transform.isIdentity && originFraction == .zero && !hasStrokeStyle
    }

    /// The fill as a color, or nil when the shape has none ("" or an
    /// unparseable string — the latter never survives `decode`).
    var fillColor: NSColor? {
        if let artwork { return artwork.fill.flatMap { ArtworkColor.color($0, in: ink) } }
        return fill.isEmpty ? nil : TextLayer.color(fromHex: fill)
    }

    /// The stroke as a color, or nil when the shape has none.
    var strokeColor: NSColor? {
        if let artwork { return artwork.stroke.flatMap { ArtworkColor.color($0, in: ink) } }
        return stroke.isEmpty ? nil : TextLayer.color(fromHex: stroke)
    }

    /// What the colour numbers are: the style's ink, sRGB without one.
    var ink: ArtworkInk { artwork?.ink ?? .srgb }

    /// The payload with its colours re-coded into `target` so it looks the
    /// same (`ArtworkInk.converted`); the legacy hex follows the numbers. A
    /// payload with no style first takes one from its sRGB hex colours.
    func inInk(_ target: ArtworkInk) -> ShapeLayerPayload {
        // Ink converts through a profile (CmykArtwork.swift), never here.
        guard target != ink, ink != .cmyk, target != .cmyk else { return self }
        let style = artwork ?? ArtworkShapeStyle(
            fill: fillColor.map(ArtworkColor.authored), stroke: strokeColor.map(ArtworkColor.authored),
            in: .srgb)
        var result = self
        let converted = style.inInk(target)
        result.artwork = converted
        result.fill = converted.fill.map(TextLayer.hex(numbers:)) ?? ""
        result.stroke = converted.stroke.map(TextLayer.hex(numbers:)) ?? ""
        return result
    }

    func withOriginFraction(_ frac: CGPoint) -> ShapeLayerPayload {
        var updated = self
        updated.originFraction = frac
        return updated
    }

    /// The JSON to store as the layer's metadata; nil only if the payload
    /// somehow cannot be encoded. Keys are sorted so re-encoding an unchanged
    /// description produces identical bytes.
    func json() -> String? {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        guard let data = try? encoder.encode(self) else { return nil }
        return String(data: data, encoding: .utf8)
    }

    /// Strict, non-throwing decode: malformed JSON, a missing or unknown
    /// `type`, an unsupported `version`, an unknown `kind`, a non-finite or
    /// negative measurement, an unparseable color, an all-empty paint, a
    /// size the kind cannot render, a singular transform or an out-of-range
    /// fraction all mean "this is a plain raster layer", never an error and
    /// never a crash.
    ///
    /// Per-kind floors: rect/ellipse need a box at least 1 px each way; a
    /// line needs at least one axis ≥ 1 px (an axis-aligned line has a zero
    /// dimension), a stroke width ≥ 1, and a stroke color — a line with no
    /// stroke is nothing.
    static func decode(_ json: String) -> ShapeLayerPayload? {
        guard let data = json.data(using: .utf8),
              let payload = try? JSONDecoder().decode(ShapeLayerPayload.self, from: data),
              payload.artwork?.isValid != false,
              kinds.contains(payload.kind),
              payload.w.isFinite, payload.w >= 0,
              payload.h.isFinite, payload.h >= 0,
              payload.strokeWidth.isFinite, payload.strokeWidth >= 0, payload.strokeWidth <= 200,
              payload.radius.isFinite, payload.radius >= 0,
              payload.fill.isEmpty || TextLayer.color(fromHex: payload.fill) != nil,
              payload.stroke.isEmpty || TextLayer.color(fromHex: payload.stroke) != nil,
              !payload.fill.isEmpty || !payload.stroke.isEmpty,
              payload.transform.isInvertible,
              TextLayer.isValidFraction(payload.originFraction)
        else { return nil }
        if payload.kind == "line" {
            guard max(payload.w, payload.h) >= 1, payload.strokeWidth >= 1,
                  !payload.stroke.isEmpty, payload.strokeAlign == .center
            else { return nil }
        } else {
            guard payload.w >= 1, payload.h >= 1 else { return nil }
        }
        return payload
    }
}

/// Rendering and naming for shape layers — the one entry point both the shape
/// tools and the agent use, so a layer committed either way looks the same.
enum ShapeLayer {
    /// One source path shared by platform previews and native area rendering.
    static func vectorPath(_ payload: ShapeLayerPayload) -> CGPath? {
        guard payload.w.isFinite, payload.h.isFinite, payload.w >= 0, payload.h >= 0 else { return nil }
        let box = CGRect(x: 0, y: 0, width: payload.w, height: payload.h)
        switch payload.kind {
        case "line":
            let path = CGMutablePath()
            path.move(to: CGPoint(x: 0, y: payload.flipped ? box.maxY : 0))
            path.addLine(to: CGPoint(x: box.maxX, y: payload.flipped ? 0 : box.maxY))
            return path
        case "rect":
            let radius = min(max(payload.radius, 0), payload.w / 2, payload.h / 2)
            return radius > 0 ? CGPath(roundedRect: box, cornerWidth: radius, cornerHeight: radius, transform: nil)
                : CGPath(rect: box, transform: nil)
        case "ellipse": return CGPath(ellipseIn: box, transform: nil)
        default: return nil
        }
    }

    // MARK: - Rendering

    /// The padded source rect (DescribedLayer.swift): the box `(0, 0, w, h)`
    /// inset by −`padding(for:)` on every side.
    static func sourceRect(_ payload: ShapeLayerPayload) -> CGRect {
        let pad = CGFloat(padding(for: payload))
        return CGRect(x: 0, y: 0, width: CGFloat(payload.w), height: CGFloat(payload.h))
            .insetBy(dx: -pad, dy: -pad)
    }

    /// Rasterizes `payload` with its box's top-left at `anchor` (canvas
    /// space) through its transform, tightly: the raster is the
    /// outward-rounded box of the padded source rect under the map
    /// (`LinearMap.rasterRect`), the path drawn under the CTM — pure
    /// CoreGraphics, safe off the main thread — and the offset is
    /// `floor(anchor) + rect.origin`. Under the identity this is the
    /// pre-transform raster: the box drawn at `padding(for:)` inside
    /// `ceil(w) + 2·pad` × `ceil(h) + 2·pad`.
    ///
    /// Rect: a rounded-rect path over the box, radius clamped to half the
    /// shorter side (0 = square corners). Ellipse: the ellipse inscribed in
    /// the box. Both fill first, then stroke centered on the same path. Line:
    /// a single round-capped segment across the box's diagonal — top-left to
    /// bottom-right, or bottom-left to top-right when `flipped` — stroke
    /// only. The stroke width is a SOURCE-space width, so a scaling map
    /// scales it — exactly what the re-edit overlay previews.
    ///
    /// `space` is the space the fill and stroke colours land in — the
    /// DOCUMENT's, so an authored shape colour is converted into the
    /// document's numbers exactly once, by CoreGraphics, here.
    ///
    /// nil for a payload `decode` would refuse (degenerate size for the kind,
    /// nothing visible to paint — a stroke of width 0 counts as nothing), a
    /// non-finite anchor, or a raster beyond the core's pixel cap.
    static func render(
        _ payload: ShapeLayerPayload, anchor: CGPoint, space: CGColorSpace
    ) -> DescribedRaster? {
        drawingPlan(payload, anchor: anchor, fill: payload.fillColor, stroke: payload.strokeColor)?
            .render(space: space)
    }

    /// The same path, stroke order and transformed bounds used by the colored
    /// renderer. Typed callers draw opaque fill/stroke coverage separately.
    static func drawingPlan(_ payload: ShapeLayerPayload, anchor: CGPoint,
                            fill: NSColor?, stroke: NSColor?) -> DescribedDrawingPlan? {
        guard ShapeLayerPayload.kinds.contains(payload.kind),
              payload.w.isFinite, payload.w >= 0, payload.w <= 1e7,
              payload.h.isFinite, payload.h >= 0, payload.h <= 1e7,
              payload.strokeWidth.isFinite, payload.strokeWidth >= 0,
              anchor.x.isFinite, anchor.y.isFinite, abs(anchor.x) < 1e7, abs(anchor.y) < 1e7
        else { return nil }
        let fillColor = fill
        // A stroke needs both a color and a width to put ink down; treating
        // width 0 as "no stroke" here keeps `strokePath` and this guard in
        // agreement, so render never returns an all-transparent raster.
        let strokeColor = payload.strokeWidth > 0 ? stroke : nil
        if payload.kind == "line" {
            guard max(payload.w, payload.h) >= 1, payload.strokeWidth >= 1,
                  strokeColor != nil
            else { return nil }
        } else {
            guard payload.w >= 1, payload.h >= 1, fillColor != nil || strokeColor != nil
            else { return nil }
        }

        let frac = DescribedLayer.anchorFraction(anchor)
        guard let rect = payload.transform.rasterRect(of: sourceRect(payload), fraction: frac)
        else { return nil }

        return DescribedDrawingPlan(
            width: rect.width, height: rect.height,
            offsetX: Int(anchor.x.rounded(.down)) + rect.originX,
            offsetY: Int(anchor.y.rounded(.down)) + rect.originY, appKit: false
        ) { context in
            // Source space → raster: the anchor's fraction, less the rect's
            // origin (relative to the anchor's whole part), after the map.
            context.translateBy(
                x: frac.x - CGFloat(rect.originX), y: frac.y - CGFloat(rect.originY))
            context.concatenate(payload.transform.cgAffine)
            context.setShouldAntialias(true)
            guard let path = vectorPath(payload) else { return false }
            if payload.kind != "line", let fill = fillColor {
                context.setFillColor(fill.cgColor); context.addPath(path); context.fillPath()
            }
            if let stroke = strokeColor, let outline = strokeOutline(payload, path: path) {
                // A styled stroke is its filled outline, the same one
                // the native renderer and the canvas preview fill.
                context.setFillColor(stroke.cgColor); context.addPath(outline); context.fillPath()
            } else if let stroke = strokeColor {
                context.setStrokeColor(stroke.cgColor)
                context.setLineWidth(CGFloat(payload.strokeWidth))
                context.setLineCap(payload.kind == "line" ? .round : .butt)
                context.addPath(path); context.strokePath()
            }
            return true
        }
    }

    /// Slack kept around the shape box, in pixels: the stroke is centered on
    /// the path, so half its width lands OUTSIDE the box, plus 2 px so
    /// antialiased edges and round line caps are never clipped —
    /// `ceil(strokeWidth / 2) + 2`, clamped to [2, 128]. A stroke aligned
    /// outside puts its whole width out there, `ceil(strokeWidth) + 2`
    /// clamped to [2, 256]; one aligned inside none of it, 2.
    static func padding(for payload: ShapeLayerPayload) -> Int {
        guard payload.strokeWidth.isFinite, payload.strokeWidth > 0 else { return 2 }
        switch payload.kind == "line" ? ShapeStrokeAlign.center : payload.strokeAlign {
        case .inside: return 2
        case .outside:
            return min(max(Int(min(payload.strokeWidth, 300).rounded(.up)) + 2, 2), 256)
        case .center:
            let slack = Int((min(payload.strokeWidth, 300) / 2).rounded(.up)) + 2
            return min(max(slack, 2), 128)
        }
    }

    // MARK: - Naming and prompts

    /// Asks whether a destructive edit may drop a layer's shape
    /// description. App-modal for the same reason TextLayer's is: the
    /// asking edit paths are synchronous.
    static func confirmRasterize(layerName: String, reason: String? = nil) -> Bool {
        let alert = NSAlert()
        alert.messageText = "Rasterize shape layer?"
        var text =
            "This edit paints over “\(layerName)”, so the layer will no longer be editable "
            + "as a shape: the description it was rendered from (its kind, box, styling and "
            + "transform) is dropped. The pixels themselves are kept."
        if let reason = reason { text += "\n\n" + reason }
        alert.informativeText = text
        alert.addButton(withTitle: "Rasterize")
        alert.addButton(withTitle: "Cancel")
        return alert.runModal() == .alertFirstButtonReturn
    }

    /// The layer name a shape gets, from its kind alone — a shape's styling
    /// makes a poor label, its kind a good one.
    static func layerName(for payload: ShapeLayerPayload) -> String {
        switch payload.kind {
        case "ellipse": return "Ellipse"
        case "line": return "Line"
        default: return "Rectangle"
        }
    }
}

// MARK: - Reading and writing the payload on a document layer

extension RasterDocument {
    /// Layer `idx`'s shape description, or nil when it has none (a plain
    /// raster layer, or metadata this app does not recognize).
    /// Read in the document's ink (`artworkInk`).
    func shapePayload(_ idx: Int) -> ShapeLayerPayload? {
        guard let meta = layerMeta(idx) else { return nil }
        return ShapeLayerPayload.decode(meta)?.inInk(artworkInk)
    }

    /// Attaches `payload` to layer `idx` as its metadata (pure, like every
    /// other layer op — pixels and offset are the caller's to chain).
    func withShapePayload(_ idx: Int, _ payload: ShapeLayerPayload) -> RasterDocument? {
        guard let json = payload.json() else { return nil }
        return withLayerMeta(idx, json)
    }
}
