import AppKit

// Warp Text in the artwork capture (designs/warp-text.md WT-3): the envelope `TextArtworkLayout`
// (ArtworkTextCapture.swift) sends every glyph outline and decoration through while TextKit draws
// the block STRAIGHT, so every depth and colour mode bends the same way through the one capture.
// The envelope lives in the layer's SOURCE space, over the layout rect TextKit lays the block out
// in; `layoutToRaster` takes it to the capture's raster.

/// Where a captured block's outlines go under its warp.
struct TextWarpPlacement {
    /// The envelope over the left-aligned layout rect (`TextLayer.layout`), in source space.
    let envelope: OutlineEnvelope
    /// Layout (source) space to the capture's raster (pixels, y down): the layer's map, then
    /// the anchor's fraction less the raster rect's origin — the drawing plan's own numbers.
    let layoutToRaster: CGAffineTransform
    let rasterToLayout: CGAffineTransform
    /// The flatness in source px: 0.05 raster px over the map's largest scale (WT-3).
    let tolerance: Double

    /// `layoutRect` is the drawing plan's own (`DescribedDrawingPlan.layoutRect`), measured once
    /// for the plan's rect and reused here.
    init(payload: TextLayerPayload, layoutRect: CGRect,
         layoutToRaster: CGAffineTransform) throws {
        guard let warp = payload.warp else {
            throw DoctoredCoreError(message: "Warped text needs a laid-out block.")
        }
        envelope = warp.envelope(box: layoutRect)
        self.layoutToRaster = layoutToRaster
        rasterToLayout = layoutToRaster.inverted()
        let largest = payload.transform.largestScale
        guard largest.isFinite, largest > 0 else {
            throw DoctoredCoreError(message: "Warped text has no finite placement.")
        }
        tolerance = 0.05 / largest
    }

    /// A glyph outline or decoration captured in the raster, bent: back to the layout, through
    /// the core's envelope map, and back to the raster.
    func outline(_ rasterPath: CGPath) throws -> CGPath {
        var toLayout = rasterToLayout
        guard let layout = rasterPath.copy(using: &toLayout) else {
            throw DoctoredCoreError(message: "Could not warp the text outline.")
        }
        guard !layout.isEmpty else { return rasterPath }
        let bent = try RasterPathCore.mapOutline(layout, envelope: envelope, tolerance: tolerance)
        var toRaster = layoutToRaster
        guard let placed = bent.copy(using: &toRaster) else {
            throw DoctoredCoreError(message: "Could not warp the text outline.")
        }
        return placed
    }
}

extension LinearMap {
    /// The 2×2 map's largest singular value: how far it stretches a unit vector at most.
    var largestScale: Double {
        let sum = a * a + b * b + c * c + d * d
        let det = determinant
        return ((sum + max(sum * sum - 4 * det * det, 0).squareRoot()) / 2).squareRoot()
    }
}
