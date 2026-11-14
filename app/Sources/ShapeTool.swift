import AppKit

/// The style a shape drags with — the options bar's fill, stroke, weight
/// and corner radius, resolved to native types and kept current on the
/// canvas by EditorViewController.
struct ShapeToolStyle {
    var fill: NSColor? = NSColor(srgbRed: 0xC9 / 255, green: 0x6F / 255, blue: 0x4A / 255, alpha: 1)
    var stroke: NSColor? = NSColor(srgbRed: 0xE8 / 255, green: 0xE2 / 255, blue: 0xD4 / 255, alpha: 1)
    var strokeWidth: CGFloat = 2
    var radius: CGFloat = 8
    /// The stroke styles (ShapeStrokeStyle.swift).
    var join: ShapeStrokeJoin = .miter
    var dash: ShapeStrokeDash = .none
    var align: ShapeStrokeAlign = .center
}

/// The shape options' two colours as colours. The store keeps each as
/// an sRGB hex plus stops (`ExtendedRangeColor.split`), so the options bar's
/// swatch can hold an above-white colour for a 32-bit float document while
/// the store stays app-wide and by appearance.
extension ShapeToolOptions {
    /// `color` as the store holds it: the hex of its SDR base and the stops
    /// that restore it; ("", 0) for none.
    static func stored(_ color: NSColor?) -> (hex: String, intensity: Double) {
        guard let color else { return ("", 0) }
        let split = ExtendedRangeColor.split(color)
        return (TextLayer.hex(split.base), split.intensity)
    }

    /// The colour of `hex` × 2^`intensity`; nil for none. Outside a float
    /// document each linear channel is clipped at 1, as a 32 → 16/8
    /// conversion clips an above-white colour (`ExtendedRangeColor`'s N-clip).
    private static func color(hex: String, intensity: Double, float: Bool) -> NSColor? {
        guard let base = TextLayer.color(fromHex: hex) else { return nil }
        guard intensity != 0,
              let combined = ExtendedRangeColor.combined(base, intensity: intensity)
        else { return base }
        guard !float else { return combined }
        guard let linear = combined.usingColorSpace(ExtendedRangeColor.space) else { return base }
        let components = [min(linear.redComponent, 1), min(linear.greenComponent, 1),
                          min(linear.blueComponent, 1), linear.alphaComponent]
        return NSColor(colorSpace: ExtendedRangeColor.space, components: components, count: 4)
    }

    /// The fill for a document that is (`float`) or is not 32-bit float.
    func fillColor(float: Bool) -> NSColor? {
        Self.color(hex: fill, intensity: fillIntensity, float: float)
    }

    /// The stroke, by `fillColor`'s rule.
    func strokeColor(float: Bool) -> NSColor? {
        Self.color(hex: stroke, intensity: strokeIntensity, float: float)
    }

    /// The fill, its picked ink (#1) cleared unless `ink` is given.
    mutating func setFill(_ color: NSColor?, ink: CmykInk? = nil) {
        (fill, fillIntensity) = Self.stored(color)
        fillInk = ink
    }

    mutating func setStroke(_ color: NSColor?, ink: CmykInk? = nil) {
        (stroke, strokeIntensity) = Self.stored(color)
        strokeInk = ink
    }

    /// The picked inks as matches for a CMYK commit (`CmykInkMatch`): each paired with the
    /// colour the store answers for it.
    var inkMatches: [CmykInkMatch] {
        [(fillColor(float: false), fillInk), (strokeColor(float: false), strokeInk)].compactMap { color, ink in
            guard let color, let ink else { return nil }
            let rgb = Array(ArtworkColor.authored(color, in: .srgb).prefix(3))
            return rgb.allSatisfy(\.isFinite) ? CmykInkMatch(srgb: rgb, ink: ink.unit) : nil
        }
    }
}

/// A shape drag in flight: the normalized box, the line's diagonal
/// direction, and the style to draw with. Built fresh on every drag tick;
/// the commit hands the box (and `flipped`) to the editor, which renders
/// the real layer through ShapeLayer.
///
/// `box` is in the space `placement` maps FROM: a fresh drag's box is in
/// canvas coordinates under the identity, a reopened layer's is its LOCAL
/// box `(0, 0, w, h)` under the layer's placement (its 2×2 map with the
/// anchor as translation). Drawing the local path under the placement's
/// CTM is exactly what ShapeLayer.render does, stroke width included (a
/// source-space width scaled by the map), so the overlay is the commit.
struct ShapeToolPreview {
    let kind: EditorTool
    let box: CGRect
    /// Maps `box`'s space onto the canvas.
    let placement: CGAffineTransform
    /// Line only: true when it runs bottom-left → top-right.
    let flipped: Bool
    let style: ShapeToolStyle
    /// A path shape's outline in `box`'s space, already scaled to it (a reopened path shape's
    /// session, `ShapeEditSession.scaledOutline`); nil for the box kinds.
    var outline: ShapeOutline?

    /// Geometry from a drag: Shift constrains rect/ellipse to a square and
    /// a line to the nearer of horizontal / vertical / 45°.
    ///
    /// `snap` and `context` arrive because the drag becomes geometry HERE
    /// and nowhere else, and the call site is in the frozen canvas. The
    /// order is constrain → snap the driving axis → re-derive the other from
    /// the constraint: ⇧ wins, so a square stays square. Both default to
    /// inert, which is what keeps the second, non-drag initializer (the
    /// shape-edit overlay) untouched.
    init(kind: EditorTool, from anchor: CGPoint, to point: CGPoint,
         constrained: Bool, style: ShapeToolStyle,
         snap: SnapEngine = .inactive, context: SnapContext = .identity) {
        self.kind = kind
        self.style = style
        self.placement = .identity
        // BOTH corners snap, and the FIXED one is snapped on every tick
        // rather than once at mouse-down: the canvas's shape press carries no
        // modifier flags, so an anchor snapped there could not be suspended
        // with ⌃ — and the anchor does not move, so its own pull answers the
        // same on every tick. It is also what keeps a zero-length drag a
        // zero-length box when a line runs through the press point: both
        // corners land on that line instead of only the moving one.
        let base = CGPoint(
            x: Self.snapped(anchor.x, .vertical, snap, context),
            y: Self.snapped(anchor.y, .horizontal, snap, context))
        var dx = point.x - base.x
        var dy = point.y - base.y
        // Which axis the ⇧ constraint let the pointer DRIVE, decided before
        // the constraint rewrites both components: the one that set the side
        // length. A tie goes to x, matching `max`'s own answer below.
        let drivesX = abs(dx) >= abs(dy)
        if constrained {
            if kind == .shapeLine {
                // Snap to the nearest 45° step by flattening the smaller
                // component (or equalizing, for the diagonals).
                if abs(dx) > abs(dy) * 2 {
                    dy = 0
                } else if abs(dy) > abs(dx) * 2 {
                    dx = 0
                } else {
                    let side = max(abs(dx), abs(dy))
                    dx = dx < 0 ? -side : side
                    dy = dy < 0 ? -side : side
                }
            } else {
                let side = max(abs(dx), abs(dy))
                dx = dx < 0 ? -side : side
                dy = dy < 0 ? -side : side
            }
        }
        // CONSTRAIN → SNAP THE DRIVING AXIS → RE-DERIVE THE OTHER. ⇧ wins: a
        // square stays square and a 45° line stays at 45°, because the axis
        // the constraint tied to the other one is never given a coordinate of
        // its own. Unconstrained, the two axes are independent and each takes
        // its own pull; a line flattened to one axis has only that axis left
        // to drive.
        let corner = CGPoint(x: base.x + dx, y: base.y + dy)
        if !constrained {
            dx = Self.snapped(corner.x, .vertical, snap, context) - base.x
            dy = Self.snapped(corner.y, .horizontal, snap, context) - base.y
        } else if dy == 0 {
            // A line flattened to the horizontal (or a drag that has not left
            // the anchor) has only this axis left to drive.
            dx = Self.snapped(corner.x, .vertical, snap, context) - base.x
        } else if dx == 0 {
            dy = Self.snapped(corner.y, .horizontal, snap, context) - base.y
        } else {
            let side = drivesX
                ? abs(Self.snapped(corner.x, .vertical, snap, context) - base.x)
                : abs(Self.snapped(corner.y, .horizontal, snap, context) - base.y)
            dx = dx < 0 ? -side : side
            dy = dy < 0 ? -side : side
        }
        let end = CGPoint(x: base.x + dx, y: base.y + dy)
        box = CGRect(
            x: min(base.x, end.x), y: min(base.y, end.y),
            width: abs(dx), height: abs(dy))
        // Up-right and down-left drags run bottom-left → top-right.
        flipped = kind == .shapeLine && dx != 0 && dy != 0 && (dx < 0) != (dy < 0)
    }

    /// One coordinate pulled onto the nearest line, or left exactly where it
    /// was when nothing is within the engine's radius (which is also what an
    /// inactive engine and a ⌃-suspended tick answer).
    ///
    /// `.exact`, not `.wholePixels`: a shape commits its box as a `Double` in
    /// its own payload and its anchor through
    /// `DescribedLayer.placementAnchor`, so nothing downstream re-rounds a
    /// snapped edge off the line it landed on.
    private static func snapped(
        _ coordinate: CGFloat, _ axis: SnapAxis, _ snap: SnapEngine, _ context: SnapContext
    ) -> CGFloat {
        guard let offset = snap.pull([coordinate], axis: axis, in: context, quantize: .exact)
        else { return coordinate }
        return coordinate + offset
    }

    /// Direct geometry (no drag): the shape-edit session's overlay — the
    /// LOCAL box under the session's placement, and a path shape's outline.
    init(kind: EditorTool, box: CGRect, placement: CGAffineTransform, flipped: Bool,
         style: ShapeToolStyle, outline: ShapeOutline? = nil) {
        self.kind = kind
        self.box = box
        self.placement = placement
        self.flipped = kind == .shapeLine ? flipped : false
        self.style = style
        self.outline = outline
    }

    /// The payload a commit of this preview renders from, colours aside: the
    /// geometry and stroke styles `ShapeLayer.strokeOutline` reads.
    private var outlinePayload: ShapeLayerPayload {
        let name: String
        // No `default:` — a new shape tool must name its kind here.
        switch kind {
        case .shapeEllipse: name = "ellipse"
        case .shapeLine: name = "line"
        case .shapeRect, .select, .ellipseSelect, .lasso, .wand, .subject, .move, .brush,
            .eraser, .fill, .gradient, .text, .eyedropper, .crop, .clone, .dodge, .heal,
            .spotHeal, .patch, .redEye, .zoom, .hand, .freehandLasso, .quickSelect,
            .magneticLasso, .colorSampler, .ruler, .perspectiveCrop, .blurBrush, .sharpenBrush,
            .smudge, .sponge, .historyBrush, .objectSelect, .pen, .remove, .contentAwareMove:
            name = "rect"
        case .artHistoryBrush, .pencil:
            name = "rect"
        }
        return ShapeLayerPayload(
            kind: outline == nil ? name : "path", w: Double(box.width), h: Double(box.height),
            flipped: flipped,
            fill: nil, stroke: .black, strokeWidth: Double(style.strokeWidth),
            radius: Double(style.radius),
            strokeJoin: style.join, strokeDash: style.dash, strokeAlign: style.align,
            path: outline)
    }

    /// A path shape's outline as the preview fills and strokes it, in `box`'s space (moved to the
    /// box's corner) with its own fill rule and butt ends, as the commit renders it; nil for the
    /// box kinds.
    func outlinePath() -> NSBezierPath? {
        guard let outline else { return nil }
        var shift = CGAffineTransform(translationX: box.minX, y: box.minY)
        let local = outline.cgPath()
        let path = NSBezierPath(cgPath: local.copy(using: &shift) ?? local)
        path.windingRule = outline.evenOdd ? .evenOdd : .nonZero
        path.lineCapStyle = .butt
        return path
    }

    /// The stroke's filled outline in `box`'s space when a style is off its
    /// default (`ShapeLayer.strokeOutline`, the commit's own); nil for a
    /// default stroke, which previews as it always did.
    func styledStrokeOutline() -> CGPath? {
        let payload = outlinePayload
        guard payload.hasStrokeStyle, let path = ShapeLayer.vectorPath(payload),
              let outline = ShapeLayer.strokeOutline(payload, path: path)
        else { return nil }
        var shift = CGAffineTransform(translationX: box.minX, y: box.minY)
        return outline.copy(using: &shift)
    }
}

/// A shape layer reopened for editing (the layers panel's double-click):
/// which layer, the box being adjusted, and the drag in flight. Owned by
/// EditorViewController (the +Shapes extension drives it); the canvas
/// draws `shapeEditOverlay` and routes the gesture, the crop session's
/// division of labor. Styling is deliberately NOT here — the session reads
/// the options bar's live shape style, which editShapeLayer seeds from the
/// layer's own payload on open.
///
/// The box lives in the shape's OWN space: `size` is the payload's
/// `w × h`, `transform` its 2×2 map, and `anchor` the exact canvas point
/// the box's top-left sits at (the layer's described anchor,
/// DescribedLayer.swift). The canvas quad is `anchor + transform·corner`,
/// so a rotated rectangle shows a rotated box, and a handle drag is
/// inverse-mapped into local space so the rectangle resizes along its own
/// axes (EditorViewController+Shapes.swift). A re-edit never changes the
/// map — only a Free Transform does.
struct ShapeEditSession {
    /// What a drag on the session does, decided at mouse-down. Both cases
    /// snapshot the geometry at mouse-down, so every tick recomputes from
    /// the snapshot rather than accumulating deltas (the crop and Free
    /// Transform rule).
    enum Drag {
        /// Dragging a handle; the raw value indexes
        /// `ImageCanvasView.transformHandlePoints(quad)` — the same
        /// TL, TR, BR, BL, T, R, B, L order as `CropSession.handles`, so
        /// `CropSession.resizing`'s index switch applies in local space.
        case handle(Int, start: (anchor: CGPoint, size: CGSize))
        /// Dragging the interior: moves the anchor by the canvas delta.
        case move(grab: CGPoint, start: CGPoint)
    }

    let layer: Int
    /// One of ShapeLayerPayload.kinds; a re-edit never changes it.
    let kind: String
    /// The exact canvas position of the local box's top-left.
    var anchor: CGPoint
    /// The anchor the session opened with — the layer's own, exact. A
    /// commit whose drags never moved it keeps it byte-for-byte
    /// (`DescribedLayer.placementAnchor(from:to:transform:)`).
    let openedAnchor: CGPoint
    /// The shape box's size in its own space (the payload's `w`, `h`).
    var size: CGSize
    /// The layer's 2×2 map, fixed for the session.
    let transform: LinearMap
    var flipped: Bool
    var drag: Drag?
    /// Where the current drag pressed down, for the click-vs-drag slop
    /// test (jitter inside a double-click must not count as a drag).
    var pressPoint: CGPoint?
    /// True once the box or the style changed; an untouched session
    /// commits nothing (no phantom undo step).
    var dirty = false
    /// A path shape's outline as the session opened it, and the box size it
    /// was opened at; nil for the box kinds. A re-box scales it with the box.
    var outline: ShapeOutline? = nil
    var openedSize: CGSize = .zero
    /// The axes a re-box scales the outline along
    /// (`ShapeOutline.scalingAxes(in: openedSize)`), taken once at open; a
    /// handle drag keeps the size of any other. Both for the box kinds.
    var outlineAxes: (x: Bool, y: Bool) = (true, true)

    /// The local box: the origin is the anchor by construction.
    var localBox: CGRect {
        CGRect(origin: .zero, size: size)
    }

    /// The outline scaled to the current box (`ShapeOutline.reboxed`, the rule
    /// `edit_shape_layer` applies too).
    var scaledOutline: ShapeOutline? {
        outline?.reboxed(from: openedSize, to: size, axes: outlineAxes)
    }

    /// Local space → canvas: the map with the anchor as its translation —
    /// the CTM the overlay draws under and the one ShapeLayer.render
    /// effectively uses.
    var placement: CGAffineTransform {
        var placement = transform.cgAffine
        placement.tx = anchor.x
        placement.ty = anchor.y
        return placement
    }

    /// The local box's corners on the canvas (TL, TR, BR, BL).
    var quad: [CGPoint] {
        transform.quad(of: localBox, anchor: anchor)
    }
}

// MARK: - Canvas drawing

extension ImageCanvasView {
    /// The live drag preview: the exact fill and stroke the commit will
    /// render, so what is dragged is what lands. The path is built in the
    /// preview's own space and drawn under its placement, so a reopened
    /// rotated shape previews rotated — and its stroke width scales with
    /// the map exactly as the renderer's does.
    func drawShapePreview(_ preview: ShapeToolPreview) {
        guard let context = NSGraphicsContext.current?.cgContext else { return }
        context.saveGState()
        defer { context.restoreGState() }
        context.concatenate(preview.placement)

        let path: NSBezierPath
        if let outline = preview.outlinePath() {
            path = outline
        } else {
            path = Self.boxPreviewPath(preview)
        }
        if preview.kind != .shapeLine, let fill = preview.style.fill {
            fill.setFill()
            path.fill()
        }
        if let stroke = preview.style.stroke, preview.style.strokeWidth > 0,
           let outline = preview.styledStrokeOutline() {
            // A styled stroke previews as the outline the commit fills.
            context.setFillColor(stroke.cgColor)
            context.addPath(outline)
            context.fillPath()
        } else if let stroke = preview.style.stroke, preview.style.strokeWidth > 0 {
            path.lineWidth = preview.style.strokeWidth
            // A path shape's ends are butt, as its commit strokes them.
            if preview.outline == nil { path.lineCapStyle = .round }
            stroke.setStroke()
            path.stroke()
        }
    }

    /// A box kind's preview path: the ellipse, the line across the box, or the rounded rect.
    private static func boxPreviewPath(_ preview: ShapeToolPreview) -> NSBezierPath {
        let path: NSBezierPath
        switch preview.kind {
        case .shapeEllipse:
            path = NSBezierPath(ovalIn: preview.box)
        case .shapeLine:
            path = NSBezierPath()
            let box = preview.box
            if preview.flipped {
                path.move(to: CGPoint(x: box.minX, y: box.maxY))
                path.line(to: CGPoint(x: box.maxX, y: box.minY))
            } else {
                path.move(to: CGPoint(x: box.minX, y: box.minY))
                path.line(to: CGPoint(x: box.maxX, y: box.maxY))
            }
        default:
            let radius = min(preview.style.radius, min(preview.box.width, preview.box.height) / 2)
            path = NSBezierPath(roundedRect: preview.box, xRadius: radius, yRadius: radius)
        }
        return path
    }

    /// A reopened shape's editing chrome: the live preview plus a hairline
    /// box through the placed quad and the eight handles at its corners and
    /// edge midpoints (the Free Transform box's layout and style, so
    /// handles read the same everywhere). The chrome is drawn in CANVAS
    /// space, not under the placement, so the handle squares keep their
    /// screen size whatever the map.
    func drawShapeEditOverlay(_ preview: ShapeToolPreview) {
        drawShapePreview(preview)
        let corners = preview.placement.quad(of: preview.box)
        guard corners.count == 4 else { return }
        let scale = magnification
        let box = NSBezierPath()
        box.move(to: corners[0])
        for corner in corners.dropFirst() {
            box.line(to: corner)
        }
        box.close()
        box.lineWidth = 3 / scale
        NSColor.black.withAlphaComponent(0.45).setStroke()
        box.stroke()
        box.lineWidth = 1 / scale
        NSColor.white.withAlphaComponent(0.95).setStroke()
        box.stroke()

        let size = ImageCanvasView.transformHandleSize / scale
        for point in ImageCanvasView.transformHandlePoints(corners) {
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
