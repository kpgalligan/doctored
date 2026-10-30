import AppKit

// The Perspective Crop tool: drag a rectangle, pull its four corners
// onto the corners of something that should be square-on, and commit; the
// quadrilateral becomes the whole canvas, resampled flat. One core request
// (`perspective_crop`, core/src/perspective_crop.rs) over every layer, mask
// and saved channel, at 8 and 16 bits and in 32-bit float, as ONE
// "Perspective Crop" step. MCP `perspective_crop` is the twin.
//
// A perspective map cannot carry a text or shape description, so
// every described layer is rasterized: the tool asks first, the agent passes
// `rasterize_artwork`. A layer with a retained source (a camera RAW layer)
// refuses the whole crop by name, in the core.

/// One Perspective Crop: the source quadrilateral and the size it becomes.
struct PerspectiveCropRequest {
    /// TL, TR, BR, BL in source canvas pixels.
    var corners: [CGPoint]
    var width: Int
    var height: Int
    var sampler: DrResizeFilter

    /// The result size when none is given: each side is the longer of the
    /// quadrilateral's two opposite edges, rounded, at least 1 px.
    static func autoSize(_ corners: [CGPoint]) -> (width: Int, height: Int) {
        guard corners.count == 4 else { return (1, 1) }
        func length(_ a: Int, _ b: Int) -> Double {
            Double(hypot(corners[b].x - corners[a].x, corners[b].y - corners[a].y))
        }
        let width = max(length(0, 1), length(3, 2)).rounded()
        let height = max(length(0, 3), length(1, 2)).rounded()
        let bounded: (Double) -> Int = { $0.isFinite ? Int(min(max($0, 1), 1e7)) : 1 }
        return (bounded(width), bounded(height))
    }

    /// The core request.
    func command() throws -> PrecisionPixelCommand {
        guard corners.count == 4 else {
            throw DoctoredCoreError(message: "Perspective Crop needs four corners.")
        }
        return try PrecisionPixelCommand([
            "op": "perspective_crop",
            "quad": corners.flatMap { [Double($0.x), Double($0.y)] },
            "width": width, "height": height,
            "filter": try RasterDocument.precisionFilter(sampler).rawValue,
        ])
    }

    /// The recorded `perspective_crop` step and the tool's arguments.
    func arguments(rasterizeArtwork: Bool) -> [String: Any] {
        var arguments: [String: Any] = [
            "corners": corners.map { [Double($0.x), Double($0.y)] },
            "width": width, "height": height,
            "sampler": EditorViewController.samplerAgentName(sampler),
        ]
        if rasterizeArtwork { arguments["rasterize_artwork"] = true }
        return arguments
    }
}

/// The canvas's Perspective Crop state: the quadrilateral (nil until a
/// rectangle has been dragged), the drag in flight, and the options bar's
/// output size (0 = `PerspectiveCropRequest.autoSize`).
struct PerspectiveCropSession {
    /// What a drag does, decided at mouse-down.
    enum Drag {
        /// Dragging out the first rectangle from `anchor`.
        case creating(anchor: CGPoint)
        /// Dragging corner `index` (TL, TR, BR, BL).
        case corner(Int)
        /// Dragging the whole quadrilateral: every corner moves by the
        /// pointer's travel from `grab`.
        case move(grab: CGPoint, start: [CGPoint])
    }

    var corners: [CGPoint]?
    var drag: Drag?
    var width = 0
    var height = 0

    /// Whether `corners` is a convex quadrilateral with area: every turn
    /// goes the same way. A concave, crossed or collapsed one cannot be
    /// squared up, and the core refuses it.
    static func isConvex(_ corners: [CGPoint]) -> Bool {
        guard corners.count == 4 else { return false }
        var sign: CGFloat = 0
        for index in 0..<4 {
            let a = corners[index], b = corners[(index + 1) % 4], c = corners[(index + 2) % 4]
            let cross = (b.x - a.x) * (c.y - b.y) - (b.y - a.y) * (c.x - b.x)
            guard abs(cross) > 1e-6 else { return false }
            if sign == 0 { sign = cross } else if (cross > 0) != (sign > 0) { return false }
        }
        return true
    }

    /// The size a commit would produce: the bar's W and H where set, the
    /// automatic size otherwise.
    func outputSize() -> (width: Int, height: Int)? {
        guard let corners else { return nil }
        let auto = PerspectiveCropRequest.autoSize(corners)
        return (width > 0 ? width : auto.width, height > 0 ? height : auto.height)
    }
}

extension RasterDocument {
    /// Perspective Crop as ONE job with its display preparation
    /// (`performingCanvasGeometry`): the core resamples every layer, mask
    /// and channel onto the new canvas. No artwork is re-rendered — a
    /// perspective map carries no description — so at 8 bits every layer in
    /// `described` loses its description inside the same edit; at 16 bits
    /// and float the core drops each resampled raster's itself. nil when the
    /// request is the whole canvas at its own size; a refusal is thrown with
    /// the core's sentence. On an Indexed document an area the quad leaves
    /// uncovered takes the entry nearest `indexedBackground` when there is no
    /// transparent index (#547 Indexed phase 3b); nil is white.
    func perspectiveCropped(_ request: PerspectiveCropRequest, described: [Int],
                            indexedBackground: NSColor? = nil) throws -> PreparedDocumentEdit? {
        try performingCanvasGeometry(
            IndexedDocument.backgrounded(request.command(), indexedBackground, in: self),
            title: "Perspective Crop",
            artwork: ArtworkGeometryWork(noArtworkFor: self),
            dropping: isHighDepth ? [] : described)
    }
}
