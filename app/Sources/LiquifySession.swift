import AppKit

// Filters ▸ Liquify…: the workspace's model. The session previews on a PROXY of the active
// layer (at most 2,048 px on its longest side) through the core's live field
// (`RasterLiquifyField`), one segment of a drag at a time, and keeps the strokes as plain geometry
// in canvas pixels. Apply hands that list to the full-size edit (`EditorViewController.commitLiquify`),
// which builds the same field from it: a stroke applied whole equals its first point followed by
// its segments (`liquify.rs`), so the preview and the commit agree.
// In a 32-bit float document the proxy is the layer through the document's tone
// map (`RasterDocument.layerThumbnail`'s float arm, the bytes the Layers panel shows): the field
// and the overlays work on that RGBA8 picture as at every depth, values above white preview as
// the tone map shows them, and Apply moves the layer's own linear samples.

/// The Liquify workspace's state: the proxy, the strokes made so far, the
/// field they build and the preview rendered through it.
struct LiquifySession {
    /// The longest side of the preview proxy, and of the core's field grid
    /// (`liquify::FIELD_MAX_SIDE`).
    static let proxyMaxSide = 2048

    /// The layer being liquified, its raster's size and its canvas offset.
    let layer: Int
    let width: Int
    let height: Int
    let origin: (x: Int, y: Int)
    /// The layer's own pixels at 8 bits, reduced to `proxyMaxSide`; a float
    /// layer's through the document's tone map.
    let proxy: RasterImage
    /// The selection's coverage at the proxy's size; nil with no selection.
    let clipMask: [UInt8]?

    var tool = LiquifyTool.forwardWarp
    /// The brush diameter in layer pixels.
    var size: Double
    /// Percentages, 0…100: a stroke's `strength` and `density`.
    var pressure = 50.0
    var density = 50.0

    /// The committed strokes, in canvas pixels.
    private(set) var strokes: [LiquifyStroke] = []
    /// The stroke being dragged.
    private(set) var draft: LiquifyStroke?
    /// The proxy through the field as it stands, draft included.
    private(set) var preview: RasterImage

    /// The committed strokes' field; nil while suspended.
    private var field: RasterLiquifyField?
    /// The committed field with the draft's segments applied.
    private var draftField: RasterLiquifyField?
    /// The preview as it stood when the draft began, for `cancelStroke`.
    private var settled: RasterImage?
    private var reservation: SelectionWorkBudget.Reservation?

    /// True between `suspend` and `resume`: the strokes are kept, the fields,
    /// the preview and the memory reservation are not.
    var isSuspended: Bool { field == nil }

    /// Captures the active layer of `document`. Throws when it has no pixels,
    /// when the workspace does not fit the working limit, or while another
    /// background operation holds the worker slot.
    init(document: ImageDocument) throws {
        guard !document.quickMaskActive, let doc = document.doc,
              let placement = doc.layerPlacement(document.activeLayerIndex),
              placement.width > 0, placement.height > 0 else {
            throw DoctoredCoreError(message: "Liquify needs a raster layer with pixels.")
        }
        let index = document.activeLayerIndex
        let longest = max(placement.width, placement.height)
        let image = !doc.isHighDepth && longest <= Self.proxyMaxSide
            ? doc.layerImage(index)
            : doc.layerThumbnail(index, maxSide: min(Self.proxyMaxSide, longest))
        guard let image, image.width > 0, image.height > 0 else {
            throw DoctoredCoreError(message: "Liquify could not read the layer's pixels.")
        }
        var mask: [UInt8]?
        if let selection = document.canvasSelection?.maskBytes() {
            _ = try RasterResources.shared.admit(scratch: UInt64(placement.width) * UInt64(placement.height))
            guard let plane = placement.layerPlane(from: selection) else {
                throw DoctoredCoreError(message: "Liquify could not map the selection onto the layer.")
            }
            if image.width == placement.width, image.height == placement.height {
                mask = plane
            } else {
                mask = try RasterImage.resizingSelection(plane, width: placement.width, height: placement.height,
                                                         toWidth: image.width, toHeight: image.height)
            }
        }
        let lease = try SelectionWorkBudget.reservePreview(
            bytes: Self.workspaceBytes(width: placement.width, height: placement.height,
                                       proxyWidth: image.width, proxyHeight: image.height),
            outputBytes: 0)
        layer = index
        width = placement.width
        height = placement.height
        origin = (placement.x, placement.y)
        proxy = image
        clipMask = mask
        size = min(2000, max(10, (Double(longest) / 10).rounded()))
        preview = image
        field = try RasterLiquifyField(width: placement.width, height: placement.height)
        reservation = lease
    }

    // MARK: - Geometry (the core's, `liquify.rs`)

    /// The field grid's cell in layer pixels: ⌈longest side / 2,048⌉.
    private static func cellSize(width: Int, height: Int) -> Int {
        max(1, (max(width, height) + proxyMaxSide - 1) / proxyMaxSide)
    }

    /// A brush's radius in layer pixels: half its size, and never under one
    /// and a half cells.
    private func radius(size: Double) -> Double {
        max(size / 2, 1.5 * Double(Self.cellSize(width: width, height: height)))
    }

    /// The current brush's radius in proxy pixels, for the pointer outline.
    var proxyBrushRadius: CGFloat {
        CGFloat(radius(size: size) * Double(proxy.width) / Double(width))
    }

    /// What the workspace holds at once, as an arithmetic bound over buffers
    /// of known size (not a fitted plan): three fields (the committed one, the
    /// draft's and the one being built) at `liquify.rs`'s 20 bytes a node, the
    /// node count by its formulas (`cell = ⌈longest side / 2048⌉`, `⌈w / cell⌉ ×
    /// ⌈h / cell⌉`), and six proxy-sized RGBA8 images (the proxy, the preview,
    /// the preview a draft began from, the one being rendered, the mesh grid
    /// and its render; the two coverage overlays are a byte a pixel and fit in
    /// the margin), with a quarter over and 1 MiB.
    static func workspaceBytes(width: Int, height: Int, proxyWidth: Int, proxyHeight: Int) -> UInt64 {
        let cell = cellSize(width: width, height: height)
        let nodes = UInt64((width + cell - 1) / cell) * UInt64((height + cell - 1) / cell)
        let buffers = 3 * nodes * 20 + 6 * UInt64(proxyWidth) * UInt64(proxyHeight) * 4
        return buffers + buffers / 4 + (1 << 20)
    }

    /// A point of the preview (proxy pixels, as the canvas view reports it) in
    /// canvas pixels, rounded to 4 decimals (`ActionArgs.number`). The rounded
    /// value is what the field, the commit and the recorded step all use.
    func canvasPoint(proxy point: CGPoint) -> CGPoint {
        CGPoint(
            x: ActionArgs.number(Double(point.x) * Double(width) / Double(proxy.width) + Double(origin.x)),
            y: ActionArgs.number(Double(point.y) * Double(height) / Double(proxy.height) + Double(origin.y)))
    }

    // MARK: - Strokes

    private var pointCount: Int {
        strokes.reduce(draft?.points.count ?? 0) { $0 + $1.points.count }
    }

    /// Starts a stroke at `point` (canvas pixels) with the current tool, or
    /// its `reversed` one, and applies its first dab. Throws when the list is
    /// full, when the session is suspended or a stroke is already in progress,
    /// and with the core's own refusal; nothing changes then.
    mutating func begin(at point: CGPoint, reverse: Bool) throws {
        guard draft == nil, let field else {
            throw DoctoredCoreError(message: "Liquify is not ready for a stroke.")
        }
        guard strokes.count < LiquifyStroke.maxStrokes else {
            throw DoctoredCoreError(message: "Liquify holds at most 2,048 strokes. Apply these, or undo some, before adding more.")
        }
        guard pointCount < LiquifyStroke.maxPoints else {
            throw DoctoredCoreError(message: "Liquify holds at most 131,072 points in all. Apply these strokes, or undo some, before adding more.")
        }
        let stroke = LiquifyStroke(
            tool: reverse ? tool.reversed : tool,
            size: ActionArgs.number(min(5000, max(1, size))),
            strength: ActionArgs.number(min(100, max(0, pressure))),
            density: ActionArgs.number(min(100, max(0, density))),
            points: [point])
        let next = try field.applying(LiquifyStroke.kernelParameters([stroke], origin: origin))
        let image = try rendering(next, from: point, to: point, radius: radius(size: stroke.size))
        settled = preview
        draft = stroke
        draftField = next
        if let image { preview = image }
    }

    /// Extends the stroke to `point` (canvas pixels), or with `dwell` repeats
    /// its last point, which the core reads as one more dab in place. A point
    /// closer than an eighth of the brush radius (and half a pixel) to the last
    /// kept one is dropped. The new segment is applied to the draft field as
    /// one continued two-point stroke and only its bounding box is rendered
    /// again. True when the preview changed. Throws at a limit (8,192 points
    /// in the stroke, 131,072 in all) and with the core's work-limit refusal;
    /// the stroke then stands at its last accepted point.
    mutating func append(_ point: CGPoint, dwell: Bool) throws -> Bool {
        guard var stroke = draft, let current = draftField, let last = stroke.points.last else { return false }
        let reach = radius(size: stroke.size)
        let target = dwell ? last : point
        if !dwell, hypot(Double(point.x - last.x), Double(point.y - last.y)) < max(0.5, reach / 8) { return false }
        guard stroke.points.count < LiquifyStroke.maxStrokePoints else {
            throw DoctoredCoreError(message: "A liquify stroke holds at most 8,192 points. This stroke ends here.")
        }
        guard pointCount < LiquifyStroke.maxPoints else {
            throw DoctoredCoreError(message: "Liquify holds at most 131,072 points in all. This stroke ends here.")
        }
        let segment = LiquifyStroke(tool: stroke.tool, size: stroke.size, strength: stroke.strength,
                                    density: stroke.density, points: [last, target])
        let next = try current.applying(
            LiquifyStroke.kernelParameters([segment], origin: origin, continued: true))
        let image = try rendering(next, from: last, to: target, radius: reach)
        stroke.points.append(target)
        draft = stroke
        draftField = next
        if let image { preview = image }
        return image != nil
    }

    /// The preview with the box of the segment `a`…`b` (canvas pixels) rendered
    /// again through `field`: the box grown by the brush radius and two cells
    /// (a dab moves only nodes inside its radius, and a pixel reads the nodes
    /// around it), in proxy pixels rounded outward. nil when the box misses the
    /// proxy.
    private func rendering(_ field: RasterLiquifyField, from a: CGPoint, to b: CGPoint,
                           radius: Double) throws -> RasterImage? {
        let grow = radius + 2 * Double(Self.cellSize(width: width, height: height))
        let scaleX = Double(proxy.width) / Double(width), scaleY = Double(proxy.height) / Double(height)
        let left = (Double(min(a.x, b.x)) - Double(origin.x) - grow) * scaleX
        let right = (Double(max(a.x, b.x)) - Double(origin.x) + grow) * scaleX
        let top = (Double(min(a.y, b.y)) - Double(origin.y) - grow) * scaleY
        let bottom = (Double(max(a.y, b.y)) - Double(origin.y) + grow) * scaleY
        guard left.isFinite, right.isFinite, top.isFinite, bottom.isFinite,
              right > 0, bottom > 0, left < Double(proxy.width), top < Double(proxy.height) else { return nil }
        let x0 = max(0, Int(left.rounded(.down))), x1 = min(proxy.width, Int(right.rounded(.up)))
        let y0 = max(0, Int(top.rounded(.down))), y1 = min(proxy.height, Int(bottom.rounded(.up)))
        guard x0 < x1, y0 < y1 else { return nil }
        return try field.rendering(proxy, over: preview, rect: (x: x0, y: y0, width: x1 - x0, height: y1 - y0))
    }

    /// Commits the stroke being dragged; false when there was none.
    mutating func finish() -> Bool {
        guard let stroke = draft, let next = draftField else { return false }
        strokes.append(stroke)
        field = next
        draft = nil
        draftField = nil
        settled = nil
        return true
    }

    /// Drops the stroke being dragged and shows the preview it began from.
    mutating func cancelStroke() {
        guard draft != nil else { return }
        draft = nil
        draftField = nil
        if let settled { preview = settled }
        settled = nil
    }

    /// The identity field with `list` applied, and the proxy through it.
    private func replay(_ list: [LiquifyStroke]) throws -> (field: RasterLiquifyField, preview: RasterImage) {
        let identity = try RasterLiquifyField(width: width, height: height)
        guard !list.isEmpty else { return (identity, proxy) }
        let rebuilt = try identity.applying(LiquifyStroke.kernelParameters(list, origin: origin))
        return (rebuilt, try rebuilt.rendering(proxy))
    }

    /// Removes the last committed stroke and rebuilds the field from the
    /// rest; false when there is none.
    mutating func undoStroke() throws -> Bool {
        cancelStroke()
        guard !isSuspended, !strokes.isEmpty else { return false }
        let kept = Array(strokes.dropLast())
        let rebuilt = try replay(kept)
        strokes = kept
        field = rebuilt.field
        preview = rebuilt.preview
        return true
    }

    /// Removes every stroke: the identity field, and the proxy as the preview.
    mutating func restoreAll() throws -> Bool {
        cancelStroke()
        guard !isSuspended, !strokes.isEmpty else { return false }
        field = try RasterLiquifyField(width: width, height: height)
        strokes = []
        preview = proxy
        return true
    }

    /// Before Apply's full-size job, and on Cancel: drops the fields, the
    /// preview and the reservation, so the job is admitted without them.
    /// The strokes are kept.
    mutating func suspend() {
        cancelStroke()
        field = nil
        preview = proxy
        reservation = nil
    }

    /// After an Apply that changed nothing: reserves again and rebuilds the
    /// field and the preview from the strokes.
    mutating func resume() throws {
        guard isSuspended else { return }
        let lease = try SelectionWorkBudget.reservePreview(
            bytes: Self.workspaceBytes(width: width, height: height,
                                       proxyWidth: proxy.width, proxyHeight: proxy.height),
            outputBytes: 0)
        let rebuilt = try replay(strokes)
        reservation = lease
        field = rebuilt.field
        preview = rebuilt.preview
    }

    // MARK: - Overlays

    /// Show Mesh: a grid drawn over a transparent proxy-sized image, through
    /// the field. nil while suspended.
    func meshImage() throws -> RasterImage? {
        guard let field = draftField ?? field else { return nil }
        return try field.rendering(try Self.meshGrid(width: proxy.width, height: proxy.height))
    }

    /// Show Mask: the field's freeze at the proxy's size, 0…255. nil while
    /// suspended and when no Freeze Mask stroke has been made, which leaves
    /// the freeze zero everywhere.
    func freezeMask() -> [UInt8]? {
        guard let field = draftField ?? field,
              draft?.tool == .freeze || strokes.contains(where: { $0.tool == .freeze }) else { return nil }
        return field.freeze(width: proxy.width, height: proxy.height)
    }

    /// One-pixel grid lines, about 32 across the longer side. CoreGraphics
    /// draws them with antialiasing off, so every pixel is either the opaque
    /// line colour or fully transparent and the premultiplied bitmap is also
    /// the straight RGBA the core takes.
    private static func meshGrid(width: Int, height: Int) throws -> RasterImage {
        let spacing = max(8, max(width, height) / 32)
        guard let context = CGContext(
            data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4,
            space: CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
        else { throw DoctoredCoreError(message: "Could not draw the liquify mesh.") }
        context.setShouldAntialias(false)
        context.setFillColor(DS.forest.cgColor)
        for x in stride(from: 0, to: width, by: spacing) {
            context.fill(CGRect(x: x, y: 0, width: 1, height: height))
        }
        for y in stride(from: 0, to: height, by: spacing) {
            context.fill(CGRect(x: 0, y: y, width: width, height: 1))
        }
        guard let data = context.data,
              let image = RasterImage.from(
                rgba: [UInt8](UnsafeBufferPointer(start: data.assumingMemoryBound(to: UInt8.self),
                                                  count: width * height * 4)),
                width: width, height: height)
        else { throw DoctoredCoreError(message: "Could not draw the liquify mesh.") }
        return image
    }
}
