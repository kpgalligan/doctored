import AppKit

// **12F-1b2** DELETES the 16-bit straighten refusal in `DocumentPrecision.refusal` and ships the
// straighten at depth: `cropAngle` is read by BOTH depth gates AND by the `crop` handler itself
// (`AgentServer+Retouch.crop`, in place of `doubleArg(a, "angle") ?? 0`), so a
// present-but-unreadable `angle` is refused by the existing `abs(angle) <= 45` guard at 8 and 16
// bits instead of being read as zero — a disclosed 8-bit behaviour change; `coreRefusal` picks the
// family from the ANGLE (`geometry_straighten` when it is non-zero, in the DEFAULT `["document"]`
// branch); `DocumentPrecision.allows` admits `.crop` at 16 bits (S2 — the box and the rotated
// preview are display-only and the commit is the numeric typed op); and
// `RasterDocument.straightenedCrop` in this file is the ONE host composition both commit sites
// call, sending the typed `straighten` at depth and otherwise the byte `straighten` request of
// `dr_doc_geometry_edit`; both depths run as ONE compound background job with progress
// and Cancel/Esc — core geometry, artwork re-render, display preparation — returning a
// `PreparedDocumentEdit` the caller publishes through `preparedEdit` (`performingCanvasGeometry`,
// `CanvasGeometryJob.swift`; the filtered `rotatingCanvas` uses the same job for
// `rotate_canvas`). **Angle 0 never enters it at either depth** — the handler keeps its early
// `guard angle != 0` → `cropped` and `commitCropSession` its fall-through — because a plain crop
// needs no resample and at 16 bits the typed op bypasses `cropped`'s artwork-description
// capture/restore (a text layer would lose its description). A whole-canvas straighten that changes
// no pixel beeps `CropSession.unchangedStraightenSentence` at both depths (the agent's `crop` no-op
// says `unchangedStraighten`), and at 16 bits an `.unchanged` or `.refused` commit re-arms the kept
// session with a fresh gesture so later box edits preview.
//
// `CropSession.rect` is always the fit of `requested` — every box edit writes `requested` and calls
// `refit`, and the angle setter refits only, so stepping the angle back is exact. A box past the
// canvas (CropExtension.swift) is `requested` itself: its corners outside become new area.

/// What the canvas draws for a crop session: the box, the straighten angle
/// (the image rotates behind the box in the preview), and whether the
/// thirds grid shows. The session's geometry lives in
/// EditorViewController+Crop; this is only its picture.
struct CropOverlay {
    var rect: CGRect
    /// Straighten, degrees. Positive rotates the HORIZON clockwise — i.e.
    /// the image rotates counter-clockwise behind the fixed box.
    var angle: CGFloat
    var showsThirds: Bool
    /// The Straighten line being drawn, canvas coordinates.
    var straightenLine: (start: CGPoint, end: CGPoint)? = nil
}

/// The crop tool's session state and geometry: the box being adjusted, the
/// drag in flight, and the ratio it is constrained to. Owned by
/// EditorViewController (the +Crop extension drives it); pure math here.
struct CropSession {
    /// What a crop drag does, decided at mouse-down.
    enum Drag {
        /// Dragging a handle; the raw value indexes `CropSession.handles`.
        case handle(Int, start: CGRect)
        /// Dragging the interior: moves the whole box.
        case move(grab: CGPoint, start: CGRect)
        /// Dragging outside the box: rubber-bands a fresh one.
        case draw(anchor: CGPoint)
    }

    /// The box shown and committed: `CropFit.fitted(requested, …)` for a box
    /// inside the canvas, so a straightened crop never shows transparent
    /// corners; `requested` itself for one past the canvas.
    var rect: CGRect
    /// The box the user last drew, dragged, typed or chose; the angle
    /// re-fits from it, so 5° → 4° → 5° is exact and 0° restores it.
    var requested: CGRect
    var angle: Double = 0
    var drag: Drag?
    /// The aspect (width / height) the box is constrained to; nil is free.
    var ratio: Double?
    var precisionGesture: PrecisionGeometryGesture? = nil
    /// Straighten line mode: a press draws a line instead of a box.
    var drawsStraightenLine = false
    /// The line in flight, start fixed at the press.
    var straightenLine: (start: CGPoint, end: CGPoint)?

    /// A session over the whole canvas, angle 0.
    init(canvas: CGSize) {
        rect = CGRect(origin: .zero, size: canvas)
        requested = rect
    }

    /// Re-derives `rect` from `requested` at the current angle, scaling
    /// about `anchor` (default the box's centre). Only a box inside the
    /// canvas is fitted; one past it is kept as requested.
    mutating func refit(canvas: CGSize, anchor: CGPoint? = nil) {
        if Self.extends(Self.roundedWindow(requested, canvas: canvas), canvas: canvas) {
            rect = requested
        } else {
            rect = CropFit.fitted(requested, angle: angle, canvas: canvas, anchor: anchor)
        }
    }

    /// One rounded window for display and publication alike: each edge
    /// rounded, clipped to the canvas whenever the UNROUNDED rect lies inside
    /// it (so such a box always commits today's window), unclipped only for
    /// a rect past an edge.
    static func roundedWindow(_ rect: CGRect, canvas: CGSize) -> CGRect {
        let rounded = CGRect(x: rect.minX.rounded(), y: rect.minY.rounded(),
                             width: rect.width.rounded(), height: rect.height.rounded())
        let bounds = CGRect(origin: .zero, size: canvas)
        return bounds.contains(rect) ? rounded.intersection(bounds) : rounded
    }

    /// Handle order: corners TL, TR, BR, BL, then edge midpoints T, R, B, L.
    static func handles(of rect: CGRect) -> [CGPoint] {
        [
            CGPoint(x: rect.minX, y: rect.minY), CGPoint(x: rect.maxX, y: rect.minY),
            CGPoint(x: rect.maxX, y: rect.maxY), CGPoint(x: rect.minX, y: rect.maxY),
            CGPoint(x: rect.midX, y: rect.minY), CGPoint(x: rect.maxX, y: rect.midY),
            CGPoint(x: rect.midX, y: rect.maxY), CGPoint(x: rect.minX, y: rect.midY),
        ]
    }

    /// Which handle a press at `point` grabs, within `slop` image pixels;
    /// nil for none.
    static func handleIndex(at point: CGPoint, rect: CGRect, slop: CGFloat) -> Int? {
        for (index, handle) in handles(of: rect).enumerated()
        where abs(point.x - handle.x) <= slop && abs(point.y - handle.y) <= slop {
            return index
        }
        return nil
    }

    /// The box after dragging handle `index` to `point`, constrained to
    /// `ratio` (corner handles preserve it exactly; edge handles resize
    /// their axis and re-derive the other about the box's center line).
    /// Never smaller than 1×1; the caller bounds it (`bounded`).
    static func resizing(
        _ start: CGRect, handle index: Int, to point: CGPoint, ratio: Double?
    ) -> CGRect {
        var minX = start.minX
        var minY = start.minY
        var maxX = start.maxX
        var maxY = start.maxY
        switch index {
        case 0: minX = point.x; minY = point.y
        case 1: maxX = point.x; minY = point.y
        case 2: maxX = point.x; maxY = point.y
        case 3: minX = point.x; maxY = point.y
        case 4: minY = point.y
        case 5: maxX = point.x
        case 6: maxY = point.y
        default: minX = point.x
        }
        var rect = CGRect(
            x: min(minX, maxX), y: min(minY, maxY),
            width: max(abs(maxX - minX), 1), height: max(abs(maxY - minY), 1))
        guard let ratio = ratio, ratio > 0 else { return rect }

        // Re-derive the dependent dimension, anchored so the dragged
        // handle's opposite edge (or the center line, for edge handles)
        // stays put.
        switch index {
        case 4, 6: // top/bottom edges drive height
            let width = rect.height * ratio
            rect.origin.x = rect.midX - width / 2
            rect.size.width = width
        case 5, 7: // left/right edges drive width
            let height = rect.width / ratio
            rect.origin.y = rect.midY - height / 2
            rect.size.height = height
        default: // corners: keep the dragged width, re-derive height
            let height = rect.width / ratio
            // Anchor vertically on whichever edge the corner did not move.
            if index == 0 || index == 1 {
                rect.origin.y = rect.maxY - height
            }
            rect.size.height = height
        }
        return rect
    }

}

extension CropSession {
    /// The old-canvas → new-canvas map of a crop to `window`, straightened
    /// by `angle` degrees about the window's centre: the window's origin
    /// moves to (0, 0), after the rotation the commit applies. The map the
    /// straighten hands `ArtworkGeometryWork`, and the one the selection is
    /// carried by (SelectionCanvasMap.swift).
    static func canvasMap(window: CGRect, angle: Double) -> CGAffineTransform {
        guard angle != 0 else {
            return CGAffineTransform(translationX: -window.minX, y: -window.minY)
        }
        let center = CGPoint(x: window.midX, y: window.midY)
        var map = CGAffineTransform(translationX: center.x, y: center.y)
            .rotated(by: CGFloat(-angle) * .pi / 180)
            .translatedBy(x: -center.x, y: -center.y)
        map.tx -= window.minX
        map.ty -= window.minY
        return map
    }

    /// Why a straighten over the whole canvas is a no-op: the core
    /// answers nil for it, and both depths and the agent's `crop` say this.
    static func unchangedStraighten(width: Int, height: Int, angle: Double) -> String {
        "straightening the whole \(width) × \(height) canvas by "
            + "\(AgentServer.transformNumber(angle))° changes no pixel."
    }

    /// The Crop tool's beep sentence for that no-op, at both depths.
    static func unchangedStraightenSentence(width: Int, height: Int, angle: Double) -> String {
        "Nothing changed: " + unchangedStraighten(width: width, height: height, angle: angle)
    }
}

// MARK: - Straighten, then crop

extension RasterDocument {
    /// The ONE straighten-then-crop both commit sites share: the Crop tool's
    /// `commitCropSession` and the agent's `crop` handler (Phase 12F-1b2).
    ///
    /// **PRECONDITION: `matrix` is a NON-ZERO-angle straighten.** Angle 0
    /// never comes here, at either depth, and both callers keep their own
    /// early route to `cropped` for it — a plain crop needs no resample, and
    /// at 16 bits the typed op bypasses `cropped`'s artwork-description
    /// capture/restore, so a text layer would silently lose its description.
    ///
    /// ONE core request over the whole stack, every mask and every saved
    /// channel, then the crop, run as ONE job with the artwork re-render and
    /// the display preparation (`performingCanvasGeometry`). It is
    /// lock-EXEMPT: straightening re-frames the picture rather than moving a
    /// layer within it, which is why `cropped` beside it consults no lock
    /// either. A full-canvas window is straighten-only in the core itself.
    ///
    /// Text and shape descriptions are captured before work and regenerated
    /// against the composed straighten/crop map. nil when the whole canvas
    /// is unchanged; the caller publishes the prepared edit. `deleteCropped`
    /// trims each plain raster to the new canvas at every depth.
    /// `background` is a window past the canvas's Background fill.
    func straightenedCrop(_ matrix: CGAffineTransform, sampler: DrResizeFilter,
                          x: Int, y: Int, w: Int, h: Int,
                          described: [Int], deleteCropped: Bool = false,
                          background: CropBackground? = nil) throws -> PreparedDocumentEdit? {
        var artworkMap = matrix
        artworkMap.tx -= CGFloat(x)
        artworkMap.ty -= CGFloat(y)
        let artwork = try ArtworkGeometryWork(source: self, affineMap: artworkMap,
                                              canvas: (w, h))
        // At 16 bits the core drops each resampled raster's description
        // itself, so there is nothing for the host to drop.
        let dropping = isHighDepth ? [] : described.filter { !hasArtworkDescription($0) }
        return try performingCanvasGeometry(
            .straighten(matrix: matrix, filter: Self.precisionFilter(sampler),
                        x: x, y: y, width: w, height: h, deleteCropped: deleteCropped,
                        background: background?.fields),
            title: "Crop", artwork: artwork, dropping: dropping,
            deletingCroppedPixels: deleteCropped)
    }
}

// MARK: - Canvas drawing

extension ImageCanvasView {
    /// The crop overlay: everything outside the box dimmed, a hairline box,
    /// eight handles (the transform box's style, so handles read the same
    /// everywhere), and the thirds grid when asked for. A box past the image
    /// leaves its outline and handles to `cropExtentLayer`, which is
    /// not clipped to the image.
    func drawCropOverlay(_ overlay: CropOverlay) {
        let rect = overlay.rect
        let extended = updateCropExtent()
        let dim = NSBezierPath(rect: bounds)
        dim.append(NSBezierPath(rect: rect))
        dim.windingRule = .evenOdd
        NSColor.black.withAlphaComponent(0.5).setFill()
        dim.fill()

        let scale = magnification
        if !extended {
            let box = NSBezierPath(rect: rect)
            box.lineWidth = 3 / scale
            NSColor.black.withAlphaComponent(0.45).setStroke()
            box.stroke()
            box.lineWidth = 1 / scale
            NSColor.white.withAlphaComponent(0.95).setStroke()
            box.stroke()
        }

        if overlay.showsThirds {
            let thirds = NSBezierPath()
            for third in [1.0, 2.0] {
                let x = rect.minX + rect.width * third / 3
                let y = rect.minY + rect.height * third / 3
                thirds.move(to: CGPoint(x: x, y: rect.minY))
                thirds.line(to: CGPoint(x: x, y: rect.maxY))
                thirds.move(to: CGPoint(x: rect.minX, y: y))
                thirds.line(to: CGPoint(x: rect.maxX, y: y))
            }
            thirds.lineWidth = 1 / scale
            NSColor.white.withAlphaComponent(0.35).setStroke()
            thirds.stroke()
        }

        if let line = overlay.straightenLine {
            let path = NSBezierPath()
            path.move(to: line.start)
            path.line(to: line.end)
            path.lineWidth = 3 / scale
            NSColor.black.withAlphaComponent(0.45).setStroke()
            path.stroke()
            path.lineWidth = 1 / scale
            NSColor.white.withAlphaComponent(0.95).setStroke()
            path.stroke()
        }

        let size = ImageCanvasView.transformHandleSize / scale
        for point in CropSession.handles(of: rect) where !extended {
            let square = NSBezierPath(
                rect: CGRect(
                    x: point.x - size / 2, y: point.y - size / 2, width: size, height: size))
            NSColor.white.setFill()
            square.fill()
            square.lineWidth = 1 / scale
            NSColor.black.withAlphaComponent(0.65).setStroke()
            square.stroke()
        }
    }
}
