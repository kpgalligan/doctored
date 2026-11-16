import AppKit
import CoreText

// Text on a path (designs/type-on-a-path.md, TP-1..TP-9): a one-line text layer whose glyphs
// follow one subpath. The description is `TextLayerPayload.onPath` (version 6, TextLayer.swift);
// TextKit still lays the line out STRAIGHT, and the artwork capture places each glyph rigidly on
// the path (ArtworkTextOnPath.swift). This file holds the value, the TP-6 source rect, the
// straightened layout and the metrics the replies report. Layer ▸ Place Text on Path is
// EditorViewController+TextOnPath.swift; the MCP argument is AgentServer+TextOnPath.swift.

/// The subpath a text layer's one line follows, in the layer's SOURCE space: relative to the
/// anchor and before the layer's `transform`, so Move, Free Transform and Image Size place it
/// through the anchor and the map with no code of their own. JSON keys `outline`, `offset`,
/// `side`.
struct TextOnPath: Codable, Equatable {
    /// The most anchors the one subpath may hold.
    static let maxAnchors = 1024
    /// "left": the text runs along the subpath's direction, on its left as it travels (above a
    /// path drawn left to right); "right": from the other end, on the other side.
    static let sides = ["left", "right"]

    /// Exactly one subpath (`ShapeOutline`'s compact anchors).
    var outline: ShapeOutline
    /// Where the line's alignment point sits along the subpath, in source px.
    var offset: Double
    var side: String

    init(outline: ShapeOutline, offset: Double, side: String) {
        self.outline = outline
        self.offset = offset
        self.side = side
    }

    /// What a decoded value must be to describe pixels: a valid outline of ONE subpath of two
    /// to `maxAnchors` anchors (one anchor has no length), a finite offset within ±10⁷ px and a
    /// known side.
    var isValid: Bool {
        outline.isValid && outline.subpaths.count == 1 && outline.anchorCount <= Self.maxAnchors
            && outline.subpaths.first?.anchors.count ?? 0 >= 2
            && offset.isFinite && abs(offset) <= 1e7 && Self.sides.contains(side)
    }

    /// The frame runs from the subpath's other end (`dr_path_frames`' `reversed`).
    var reversed: Bool { side == "right" }

    /// A closed subpath wraps; an open one hides what runs past its ends.
    var closed: Bool { outline.subpaths.first?.closed ?? false }

    /// The subpath as a path in source space.
    var sourceShape: PathShape { outline.shape(origin: .zero) }

    /// The box of every anchor and handle in source space — the control hull, which holds the
    /// curve; nil for an empty outline.
    var controlBox: CGRect? {
        let coordinates = outline.subpaths.flatMap(\.anchors)
        guard !coordinates.isEmpty else { return nil }
        var low = CGPoint(x: Double.infinity, y: Double.infinity)
        var high = CGPoint(x: -Double.infinity, y: -Double.infinity)
        for anchor in coordinates where anchor.count >= 6 {
            for k in stride(from: 0, to: 6, by: 2) {
                low = CGPoint(x: min(low.x, anchor[k]), y: min(low.y, anchor[k + 1]))
                high = CGPoint(x: max(high.x, anchor[k]), y: max(high.y, anchor[k + 1]))
            }
        }
        guard low.x.isFinite, low.y.isFinite, high.x.isFinite, high.y.isFinite else { return nil }
        return CGRect(x: low.x, y: low.y, width: high.x - low.x, height: high.y - low.y)
    }

    /// A CANVAS subpath taken into the source space of a layer anchored at `anchor` under
    /// `transform`: `T⁻¹·(p − anchor)` for every point and handle. nil when the map is singular
    /// or the result is not a valid value.
    init?(canvas subpath: PathSubpath, anchor: CGPoint, transform: LinearMap, offset: Double,
          side: String) {
        guard let inverse = transform.inverted() else { return nil }
        func source(_ p: CGPoint) -> CGPoint {
            inverse.apply(CGPoint(x: p.x - anchor.x, y: p.y - anchor.y))
        }
        let mapped = PathSubpath(anchors: subpath.anchors.map { a in
            PathAnchor(point: source(a.point), handleIn: source(a.handleIn),
                       handleOut: source(a.handleOut), smooth: a.smooth)
        }, closed: subpath.closed)
        let value = TextOnPath(
            outline: ShapeOutline(PathShape(subpaths: [mapped]), origin: .zero),
            offset: offset, side: side)
        guard value.isValid else { return nil }
        self = value
    }

    /// The subpath on the canvas: `anchor + T·source`.
    func canvasShape(anchor: CGPoint, transform: LinearMap) -> PathShape {
        var shape = sourceShape
        func canvas(_ p: CGPoint) -> CGPoint {
            let q = transform.apply(p)
            return CGPoint(x: q.x + anchor.x, y: q.y + anchor.y)
        }
        for s in shape.subpaths.indices {
            for a in shape.subpaths[s].anchors.indices {
                var anchor = shape.subpaths[s].anchors[a]
                anchor.point = canvas(anchor.point)
                anchor.handleIn = canvas(anchor.handleIn)
                anchor.handleOut = canvas(anchor.handleOut)
                shape.subpaths[s].anchors[a] = anchor
            }
        }
        return shape
    }

    /// Whether `string` breaks its line anywhere: text on a path is one line.
    static func hasLineBreak(_ string: String) -> Bool {
        string.contains { $0.isNewline }
    }
}

extension TextLayerPayload {
    /// The payload on `value`'s path (nil takes it off), with no ink outset either way: the
    /// next render measures its own (TextInkRect.swift, TI-5). Setting one makes the text point
    /// text, and a payload with no style first takes one from its sRGB hex colour
    /// (`withArtwork`), since a version-6 payload always carries `artwork`.
    func withOnPath(_ value: TextOnPath?) -> TextLayerPayload {
        var result = self
        result.onPath = value
        result.inkOutset = nil
        guard value != nil else { return result }
        result.box = .point
        return result.withArtwork()
    }

    /// The line as TextKit lays it out before it is placed on the path: left-aligned point
    /// text with no baseline shift (the shift moves along the path's normal instead) and no
    /// path. Alignment moves the line's START along the path (`TextLayer.onPathStart`).
    var straightened: TextLayerPayload {
        var result = self
        result.alignment = "left"
        result.box = .point
        result.baselineShift = 0
        result.onPath = nil
        return result
    }
}

extension TextLayer {
    /// TP-6, the source rect of text on a path: the outline's control box grown on every side
    /// by `hypot(A/2, ascender − descender) + |baseline shift| + padding(forSize:)`, `A` the
    /// widest glyph advance of the straightened line. A glyph is placed rigidly with the
    /// centre of its advance on the path, so its box reaches at most half its advance along
    /// the tangent and its height across it — that hypotenuse — from a point of the curve,
    /// which lies in the control box; the shift moves it along the normal, and the padding
    /// covers ink past a glyph's box as it does for straight text. A function of the payload
    /// and the faces its layout resolves, as the straight rule is. MAIN THREAD (TextKit); nil
    /// without a path, for an empty string or a failed layout.
    static func onPathSourceRect(_ payload: TextLayerPayload) -> CGRect? {
        guard payload.onPath != nil, let layout = measure(payload.straightened) else { return nil }
        return onPathSourceRect(payload, stack: layout.stack)
    }

    /// `onPathSourceRect` over the straightened line's stack already laid out.
    static func onPathSourceRect(_ payload: TextLayerPayload, stack: TypesetStack) -> CGRect? {
        guard let box = payload.onPath?.controlBox else { return nil }
        let widest = glyphAdvances(of: stack).map(\.advance).max() ?? 0
        let font = payload.style.nsFont
        let reach = hypot(widest / 2, Double(font.ascender - font.descender))
            + abs(payload.baselineShift) + Double(padding(forSize: payload.size))
        guard reach.isFinite, reach >= 0 else { return nil }
        return box.insetBy(dx: -reach, dy: -reach)
    }

    /// Where the straightened line, `usedWidth` wide, starts along the path: at the offset
    /// (left), centred on it (center) or ending at it (right).
    static func onPathStart(_ payload: TextLayerPayload, usedWidth: Double) -> Double {
        let offset = payload.onPath?.offset ?? 0
        switch payload.nsAlignment {
        case .center: return offset - usedWidth / 2
        case .right: return offset - usedWidth
        default: return offset
        }
    }

    /// The subpath's length in source px and how many glyphs of the straightened line have
    /// no frame (TP-3: the centre of the glyph's advance past an open subpath's end; none on
    /// a closed one, which wraps). MAIN THREAD (TextKit); nil without a path or a layout.
    static func onPathMetrics(_ payload: TextLayerPayload) -> (length: Double, hiddenGlyphs: Int)? {
        guard let onPath = payload.onPath, let layout = measure(payload.straightened),
              let length = try? RasterPathCore.pathLength(onPath.sourceShape, subpath: 0)
        else { return nil }
        guard !onPath.closed else { return (length, 0) }
        let start = onPathStart(payload, usedWidth: Double(layout.rect.width))
        let centres = glyphAdvances(of: layout.stack).map { start + $0.x + $0.advance / 2 }
        guard let frames = try? RasterPathCore.pathFrames(
            onPath.sourceShape, subpath: 0, reversed: onPath.reversed, distances: centres)
        else { return nil }
        return (length, frames.filter { $0 == nil }.count)
    }

    /// Each shown glyph of a one-line layout (`shownGlyphs`, TextInkRect.swift): its x in the
    /// layout and its advance, read from the face its run was typeset in.
    private static func glyphAdvances(of stack: TypesetStack) -> [(x: Double, advance: Double)] {
        shownGlyphs(of: stack).compactMap { shown -> (x: Double, advance: Double)? in
            var glyph = shown.glyph
            var advance = CGSize.zero
            CTFontGetAdvancesForGlyphs(shown.font, .horizontal, &glyph, &advance, 1)
            guard advance.width.isFinite else { return nil }
            return (Double(shown.origin.x), Double(advance.width))
        }
    }
}
