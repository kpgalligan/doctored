import AppKit

// Text on a path in the artwork capture (designs/type-on-a-path.md TP-2..TP-5): the geometry
// `TextArtworkLayout` (ArtworkTextCapture.swift) asks while TextKit draws the STRAIGHTENED line,
// so every depth and colour mode bends the same way through the one capture. A glyph is placed
// rigidly with the centre of its advance on the path; a decoration is bent through the core's
// outline map (`dr_outline_map`). All path geometry is in the layer's SOURCE space, the layout
// space TextKit lays the line out in; `layoutToRaster` takes it to the capture's raster.

/// Where a captured line goes on its path.
struct TextPathPlacement {
    /// The refusal for a glyph with no outline to place: a colour or bitmap glyph (emoji).
    static let outlineGlyphsRefusal = "text on a path needs outline glyphs: remove the colour or "
        + "bitmap glyphs (emoji), or remove the text from the path"

    /// The one subpath, in source space.
    let shape: PathShape
    let reversed: Bool
    let closed: Bool
    let length: Double
    /// The distance along the subpath where the straightened line's x = 0 lands.
    let start: Double
    /// The line's baseline y in the layout.
    let baseline: Double
    /// The baseline shift, moved along the path's normal.
    let shift: Double
    /// Layout (source) space to the capture's raster (pixels, y down): the layer's map, then
    /// the anchor's fraction less the raster rect's origin — the drawing plan's own numbers.
    let layoutToRaster: CGAffineTransform
    let rasterToLayout: CGAffineTransform
    /// A decoration's flatness in source px: 0.05 raster px over the map's largest scale.
    let decorationTolerance: Double

    /// The placement of `payload`'s line, laid out straight in `stack` (the capture's own
    /// layout manager, after `TextLayer.drawingPlan`).
    init(payload: TextLayerPayload, stack: TextLayer.TypesetStack,
         layoutToRaster: CGAffineTransform) throws {
        guard let onPath = payload.onPath, stack.manager.numberOfGlyphs > 0 else {
            throw DoctoredCoreError(message: "Text on a path needs a laid-out line.")
        }
        shape = onPath.sourceShape
        reversed = onPath.reversed
        closed = onPath.closed
        length = try RasterPathCore.pathLength(shape, subpath: 0)
        let used = stack.manager.usedRect(for: stack.container)
        start = TextLayer.onPathStart(payload, usedWidth: Double(used.width))
        let fragment = stack.manager.lineFragmentRect(forGlyphAt: 0, effectiveRange: nil)
        baseline = Double(fragment.minY + stack.manager.location(forGlyphAt: 0).y)
        shift = payload.baselineShift
        self.layoutToRaster = layoutToRaster
        rasterToLayout = layoutToRaster.inverted()
        let largest = payload.transform.largestScale
        guard [start, baseline, length, largest].allSatisfy({ $0.isFinite }), largest > 0 else {
            throw DoctoredCoreError(message: "Text on a path has no finite placement.")
        }
        decorationTolerance = 0.05 / largest
    }

    /// Each glyph's map from glyph space to the raster on the path, or nil for a glyph whose
    /// centre has no frame (past an open subpath's end: not drawn). `glyphMaps` are the maps
    /// TextKit's straight draw gives the glyphs (`ArtworkTextCapture.glyphMap`), `advances`
    /// their advances in glyph space. A glyph's map is its straight one taken back to the
    /// layout, then the rigid motion taking its advance's centre on the baseline to the frame
    /// there (raised by the shift along the normal) and the layout's +x to the tangent, then
    /// back to the raster. One frames call for the list.
    func glyphMaps(_ glyphMaps: [CGAffineTransform], advances: [CGFloat]) throws -> [CGAffineTransform?] {
        let toLayout = glyphMaps.map { $0.concatenating(rasterToLayout) }
        let centres = zip(toLayout, advances).map { map, advance in
            Double(CGPoint(x: advance / 2, y: 0).applying(map).x)
        }
        let frames = try RasterPathCore.pathFrames(
            shape, subpath: 0, reversed: reversed, distances: centres.map { start + $0 })
        return zip(zip(toLayout, centres), frames).map { glyph, frame in
            guard let frame else { return nil }
            let rigid = rigidMotion(from: CGPoint(x: glyph.1, y: baseline), to: frame)
            return glyph.0.concatenating(rigid).concatenating(layoutToRaster)
        }
    }

    /// The rotation taking +x to the frame's tangent (y down, so +y goes to the tangent turned
    /// a quarter clockwise) about `origin`, moved onto the frame's point raised by the shift.
    private func rigidMotion(from origin: CGPoint, to frame: PathFrame) -> CGAffineTransform {
        let t = frame.tangent
        let target = CGPoint(x: frame.point.x + t.dy * shift, y: frame.point.y - t.dx * shift)
        return CGAffineTransform(translationX: -origin.x, y: -origin.y)
            .concatenating(CGAffineTransform(a: t.dx, b: t.dy, c: -t.dy, d: t.dx, tx: 0, ty: 0))
            .concatenating(CGAffineTransform(translationX: target.x, y: target.y))
    }

    /// A decoration (underline, strikethrough) captured in the raster, bent along the path:
    /// back to the layout, cut on an open subpath to the band of x whose distance lies on it
    /// (`start + x ∈ [0, length]`, so it ends where the glyphs stop), mapped through the
    /// core's outline map, and back to the raster.
    func decoration(_ path: CGPath) throws -> CGPath {
        var toLayout = rasterToLayout
        guard var layout = path.copy(using: &toLayout) else {
            throw DoctoredCoreError(message: "Could not place the text decoration on the path.")
        }
        if !closed {
            let bounds = layout.boundingBoxOfPath
            guard !bounds.isNull else { return layout }
            let band = CGPath(rect: CGRect(x: -start, y: bounds.minY - 1, width: length,
                                           height: bounds.height + 2), transform: nil)
            layout = layout.intersection(band)
        }
        guard !layout.isEmpty else { return layout }
        let bent = try RasterPathCore.mapOutline(
            layout, along: shape, subpath: 0, reversed: reversed, start: start,
            baseline: baseline, shift: shift, tolerance: decorationTolerance)
        var toRaster = layoutToRaster
        guard let placed = bent.copy(using: &toRaster) else {
            throw DoctoredCoreError(message: "Could not place the text decoration on the path.")
        }
        return placed
    }
}
