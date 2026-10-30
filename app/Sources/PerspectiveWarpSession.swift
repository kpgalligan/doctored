import AppKit

// Edit ▸ Perspective Warp: the on-canvas session's model. In LAYOUT the user draws quads
// over the planes of the picture (a press-drag outside every quad), drags their corners or
// whole quads, and a corner released within 10 pt of another quad's corner joins it, so the
// quads share that corner and, along a shared side, an edge. In WARP the user drags the pins
// (the corners), Shift-clicks an edge to straighten it, and the proxy re-renders through the
// core's warp on every change. Every change is resolved by the core (`dr_perspective_warp_layout`:
// joins, straightening, the refusals); a refused change leaves the session as it was and beeps
// once per gesture. The canvas half is `ImageCanvasView+PerspectiveWarp.swift`, the controller
// half `EditorViewController+PerspectiveWarp.swift`.

/// What a held press is doing.
enum PerspectiveDrag {
    /// Layout: moving one corner (its `from` and `to` by the same delta).
    case corner(vertex: Int, last: CGPoint)
    /// Layout: moving a whole quad.
    case quad(index: Int, last: CGPoint)
    /// Layout: dragging out a new quad.
    case create(start: CGPoint, current: CGPoint)
    /// Warp: moving the selected pins; `anchor` is the pressed one.
    case pins(anchor: Int, last: CGPoint)
}

struct PerspectiveWarpSession {
    /// `puppet_arap::SOLVER_BYTES`, `puppet_render::MAP_BYTES`,
    /// `perspective_warp::GRID_BYTES` and its 2 MiB bookkeeping restated: the
    /// core's working memory for each preview's extension and map. The last
    /// term is the worst-case plane mesh, `perspective_warp::TRIANGLE_BYTES`
    /// (256) × 2·n² triangles per quad at `perspective_mesh::MAX_SUBDIVISIONS`
    /// (n = 64) × `perspective_plane::MAX_QUADS` (64), because each preview
    /// prepares and maps uncapped.
    static let planeBytes: UInt64 = 134_217_728 // 256 × 2 × 64 × 64 × 64
    static let coreBytes: UInt64 = 40_206_304 + 22_097_152 + 1_048_576 + 2_097_152 + planeBytes
    /// Hit radii in screen points: a corner, a join, an edge; and the
    /// smallest side a drawn quad may have.
    static let cornerRadius: CGFloat = 6
    static let joinRadius: CGFloat = 10
    static let edgeRadius: CGFloat = 5
    static let minimumSide: CGFloat = 4

    /// The proxy, plates, coverage and lease (`MeshWarpWorkspace`).
    var workspace: MeshWarpWorkspace
    var base: RasterDocument { workspace.base }
    var source: RasterDocument { workspace.source }
    var layer: Int { workspace.layer }
    var coverage: [UInt16]? { workspace.coverage }
    var isFloat: Bool { workspace.isFloat }
    var reservation: SelectionWorkBudget.Reservation? {
        get { workspace.reservation }
        set { workspace.reservation = newValue }
    }

    private(set) var mode = PerspectiveWarpMode.layout
    private(set) var layout = PerspectiveLayout()
    /// Layout: the quad Delete removes. Warp: the selected pins (vertices).
    var selectedQuad: Int?
    var selectedPins: Set<Int> = []
    var drag: PerspectiveDrag?
    /// The proxy through the warp, and the canvas rect it covers; nil in
    /// Layout and for an unwarped layout, when the layer draws as it is.
    private(set) var preview: CGImage?
    private(set) var previewRect = CGRect.zero
    /// True while the session's own commit publishes, so the document change
    /// it causes does not end the session.
    var isCommitting = false
    /// A refusal already beeped in the current gesture.
    private var refusedInGesture = false

    /// No quad, or every corner unmoved: a commit changes nothing.
    var isIdentity: Bool { layout.quads.isEmpty || layout.identity }

    /// Opens on layer `layer` of `source`; `base` is the document's handle
    /// and `plates` its stack around the layer.
    init(document: ImageDocument, base: RasterDocument, source: RasterDocument, layer: Int,
         plates: (below: CGImage?, above: CGImage?)) throws {
        workspace = try MeshWarpWorkspace(
            document: document, base: base, source: source, layer: layer, plates: plates,
            name: PerspectiveWarp.actionName, coreBytes: Self.coreBytes)
    }

    // MARK: - Resolve

    /// `next` resolved by the core (with `anchor` the dragged corner), and
    /// the preview re-rendered; throws the core's refusal.
    private func resolved(_ next: PerspectiveLayout, anchor: Int? = nil) throws -> PerspectiveLayout {
        guard !next.quads.isEmpty else { return next }
        return try RasterPerspectiveWarp.layout(fields: next.fields(anchor: anchor))
    }

    /// Re-renders the preview in Warp; Layout draws the layer as it is.
    private mutating func refreshPreview() throws {
        guard mode == .warp, !layout.quads.isEmpty, !layout.identity else {
            preview = nil
            return
        }
        let rendered = try RasterPerspectiveWarp.preview(
            fields: ["quads": layout.quadsJSON], proxy: workspace.proxy,
            source: workspace.layerRect, coverage: workspace.proxyCoverage)
        preview = rendered.image.makeCGImage(in: workspace.colorSpace)
        previewRect = rendered.rect
    }

    /// Adopts `next` resolved; a refusal leaves the session as it was and
    /// answers true the first time in a gesture (the caller beeps).
    private mutating func adopt(_ next: PerspectiveLayout, anchor: Int? = nil) -> Bool {
        tryAdopt(next, anchor: anchor).beep
    }

    /// `adopt`, also answering whether `next` was adopted, so a drag keeps
    /// its last point on a refused step and the corner catches up.
    private mutating func tryAdopt(_ next: PerspectiveLayout,
                                   anchor: Int? = nil) -> (adopted: Bool, beep: Bool) {
        var copy = self
        do {
            copy.layout = try resolved(next, anchor: anchor)
            try copy.refreshPreview()
        } catch {
            let beep = !refusedInGesture
            refusedInGesture = true
            return (false, beep)
        }
        copy.refusedInGesture = refusedInGesture
        self = copy
        return (true, false)
    }

    // MARK: - Hit tests

    private static func distance(_ a: CGPoint, _ b: CGPoint) -> CGFloat { hypot(a.x - b.x, a.y - b.y) }

    /// The vertex whose corner (rest in Layout, warped in Warp) lies within
    /// `radius` of `point`, nearest first.
    func vertex(at point: CGPoint, radius: CGFloat) -> Int? {
        let warped = mode == .warp
        return layout.vertices.indices
            .filter { Self.distance(warped ? layout.vertices[$0].to : layout.vertices[$0].from, point) <= radius }
            .min { a, b in
                let pa = warped ? layout.vertices[a].to : layout.vertices[a].from
                let pb = warped ? layout.vertices[b].to : layout.vertices[b].from
                return Self.distance(pa, point) < Self.distance(pb, point)
            }
    }

    /// Is `point` inside (closed) the convex polygon `corners`?
    private static func contains(_ corners: [CGPoint], _ point: CGPoint) -> Bool {
        var sign: CGFloat = 0
        for k in corners.indices {
            let a = corners[k], b = corners[(k + 1) % corners.count]
            let z = (b.x - a.x) * (point.y - a.y) - (b.y - a.y) * (point.x - a.x)
            if z == 0 { continue }
            if sign == 0 { sign = z > 0 ? 1 : -1 } else if (z > 0) != (sign > 0) { return false }
        }
        return true
    }

    /// The topmost quad holding `point` (rest corners).
    func quad(at point: CGPoint) -> Int? {
        layout.quads.indices.reversed().first { Self.contains(layout.corners($0, warped: false), point) }
    }

    /// The warped edge within `radius` of `point`: `(quad, edge)`.
    func edge(at point: CGPoint, radius: CGFloat) -> (Int, Int)? {
        var best: (quad: Int, edge: Int, distance: CGFloat)?
        for q in layout.quads.indices {
            let c = layout.corners(q, warped: true)
            for k in 0..<4 {
                let a = c[k], b = c[(k + 1) % 4]
                let dx = b.x - a.x, dy = b.y - a.y
                let length = dx * dx + dy * dy
                guard length > 0 else { continue }
                let t = max(0, min(1, ((point.x - a.x) * dx + (point.y - a.y) * dy) / length))
                let d = Self.distance(point, CGPoint(x: a.x + t * dx, y: a.y + t * dy))
                if d <= radius, best.map({ d < $0.distance }) ?? true { best = (q, k, d) }
            }
        }
        return best.map { ($0.quad, $0.edge) }
    }

    /// Layout's rubber band while a quad is being drawn.
    var creatingRect: CGRect? {
        guard case let .create(start, current) = drag else { return nil }
        return CGRect(x: min(start.x, current.x), y: min(start.y, current.y),
                      width: abs(current.x - start.x), height: abs(current.y - start.y))
    }

    private static func rounded(_ point: CGPoint) -> CGPoint {
        CGPoint(x: ActionArgs.number(point.x), y: ActionArgs.number(point.y))
    }

    // MARK: - Gestures

    /// A press at `point` (canvas pixels; `scale` the view's magnification).
    /// Answers true when the caller should beep.
    mutating func press(at point: CGPoint, modifiers: NSEvent.ModifierFlags, scale: CGFloat) -> Bool {
        refusedInGesture = false
        let shift = modifiers.contains(.shift)
        switch mode {
        case .layout:
            if let v = vertex(at: point, radius: Self.cornerRadius / scale) {
                selectedQuad = layout.quads.firstIndex { $0.contains(v) }
                drag = .corner(vertex: v, last: point)
            } else if let q = quad(at: point) {
                selectedQuad = q
                drag = .quad(index: q, last: point)
            } else {
                selectedQuad = nil
                drag = .create(start: point, current: point)
            }
            return false
        case .warp:
            if let v = vertex(at: point, radius: Self.cornerRadius / scale) {
                if shift {
                    if selectedPins.contains(v) { selectedPins.remove(v) } else { selectedPins.insert(v) }
                } else if !selectedPins.contains(v) {
                    selectedPins = [v]
                }
                drag = selectedPins.contains(v) ? .pins(anchor: v, last: point) : nil
                return false
            }
            drag = nil
            if shift, let (q, k) = edge(at: point, radius: Self.edgeRadius / scale) {
                var next = layout
                let pair = layout.edge(q, k)
                let same = { (s: PerspectiveStraighten) -> Bool in
                    guard next.quads.indices.contains(s.quad) else { return false }
                    let (a, b) = next.edge(s.quad, s.edge)
                    return (a, b) == pair || (b, a) == pair
                }
                if next.straighten.contains(where: same) {
                    next.straighten.removeAll(where: same)
                } else {
                    next.straighten.append(PerspectiveStraighten(quad: q, edge: k, direction: .auto))
                }
                return adopt(next)
            }
            if !shift { selectedPins = [] }
            return false
        }
    }

    /// The held press moved to `point`. Answers true when the caller should beep.
    mutating func dragged(to point: CGPoint) -> Bool {
        guard let drag else { return false }
        switch drag {
        case let .corner(v, last):
            let moved = move([v], by: CGPoint(x: point.x - last.x, y: point.y - last.y))
            if moved.adopted { self.drag = .corner(vertex: v, last: point) }
            return moved.beep
        case let .quad(q, last):
            guard layout.quads.indices.contains(q) else { return false }
            let moved = move(Array(Set(layout.quads[q])),
                             by: CGPoint(x: point.x - last.x, y: point.y - last.y))
            if moved.adopted { self.drag = .quad(index: q, last: point) }
            return moved.beep
        case let .create(start, _):
            self.drag = .create(start: start, current: point)
            return false
        case let .pins(anchor, last):
            let dx = point.x - last.x, dy = point.y - last.y
            guard dx != 0 || dy != 0 else { return false }
            var next = layout
            for v in selectedPins where next.vertices.indices.contains(v) {
                let to = next.vertices[v].to
                next.vertices[v].to = Self.rounded(CGPoint(x: to.x + dx, y: to.y + dy))
            }
            let result = tryAdopt(next, anchor: anchor)
            if result.adopted { self.drag = .pins(anchor: anchor, last: point) }
            return result.beep
        }
    }

    /// Layout: moves `vertices`' corners (rest and warped) by `delta`.
    private mutating func move(_ vertices: [Int], by delta: CGPoint) -> (adopted: Bool, beep: Bool) {
        guard delta.x != 0 || delta.y != 0 else { return (false, false) }
        var next = layout
        for v in vertices where next.vertices.indices.contains(v) {
            let from = next.vertices[v].from, to = next.vertices[v].to
            next.vertices[v].from = Self.rounded(CGPoint(x: from.x + delta.x, y: from.y + delta.y))
            next.vertices[v].to = Self.rounded(CGPoint(x: to.x + delta.x, y: to.y + delta.y))
        }
        return tryAdopt(next)
    }

    /// The press ended: a drawn quad is added, and in Layout the moved
    /// corners join any other quad's corner within the join radius. Answers
    /// true when the caller should beep.
    mutating func released(scale: CGFloat) -> Bool {
        defer { drag = nil }
        guard let drag else { return false }
        switch drag {
        case let .create(start, current):
            let side = Self.minimumSide / scale
            guard abs(current.x - start.x) >= side, abs(current.y - start.y) >= side else {
                return false
            }
            guard layout.quads.count < PerspectiveWarp.maxQuads else { return !refusedInGesture }
            let x0 = min(start.x, current.x), x1 = max(start.x, current.x)
            let y0 = min(start.y, current.y), y1 = max(start.y, current.y)
            var next = layout
            let first = next.vertices.count
            for corner in [CGPoint(x: x0, y: y0), CGPoint(x: x1, y: y0), CGPoint(x: x1, y: y1),
                           CGPoint(x: x0, y: y1)] {
                let p = Self.rounded(corner)
                next.vertices.append(PerspectiveVertex(from: p, to: p))
            }
            next.quads.append(Array(first..<first + 4))
            Self.join(&next, moved: Array(first..<first + 4), radius: Self.joinRadius / scale)
            let beep = adopt(next)
            if !beep, layout.quads.count > 0 { selectedQuad = layout.quads.count - 1 }
            return beep
        case let .corner(v, _):
            var next = layout
            Self.join(&next, moved: [v], radius: Self.joinRadius / scale)
            return next == layout ? false : adopt(next)
        case let .quad(q, _):
            guard layout.quads.indices.contains(q) else { return false }
            var next = layout
            Self.join(&next, moved: layout.quads[q], radius: Self.joinRadius / scale)
            return next == layout ? false : adopt(next)
        case .pins:
            return false
        }
    }

    /// Each moved corner within `radius` of another quad's corner becomes
    /// that corner (never two corners of one quad), then unused vertices go.
    private static func join(_ next: inout PerspectiveLayout, moved: [Int], radius: CGFloat) {
        var changed = false
        for m in moved {
            let holders = next.quads.indices.filter { next.quads[$0].contains(m) }
            guard !holders.isEmpty else { continue }
            let target = next.vertices.indices.filter { t in
                t != m && next.quads.contains { $0.contains(t) }
                    && !holders.contains { next.quads[$0].contains(t) }
                    && distance(next.vertices[t].from, next.vertices[m].from) <= radius
            }.min { distance(next.vertices[$0].from, next.vertices[m].from)
                < distance(next.vertices[$1].from, next.vertices[m].from) }
            guard let target else { continue }
            for q in holders {
                next.quads[q] = next.quads[q].map { $0 == m ? target : $0 }
            }
            changed = true
        }
        if changed { next.compact() }
    }

    /// Delete: Layout removes the selected quad and its straighten entries.
    mutating func removeSelected() -> Bool {
        guard mode == .layout, let q = selectedQuad, layout.quads.indices.contains(q) else {
            return false
        }
        var next = layout
        next.quads.remove(at: q)
        next.straighten = next.straighten.compactMap { entry in
            guard entry.quad != q else { return nil }
            var moved = entry
            if moved.quad > q { moved.quad -= 1 }
            return moved
        }
        next.compact()
        selectedQuad = nil
        refusedInGesture = false
        return adopt(next)
    }

    // MARK: - Options

    /// Layout or Warp; Warp needs a quad.
    mutating func setMode(_ next: PerspectiveWarpMode) throws {
        guard next != mode else { return }
        guard next == .layout || !layout.quads.isEmpty else {
            throw DoctoredCoreError(message: "Perspective Warp needs a quad before Warp.")
        }
        var copy = self
        copy.mode = next
        copy.drag = nil
        copy.selectedPins = []
        try copy.refreshPreview()
        self = copy
    }

    /// Straighten Vertical / Level Horizontal / Straighten Both: one entry
    /// of each of `directions` for every edge, which the core keeps only
    /// where the edge lies near that axis.
    mutating func straightenAll(_ directions: [PerspectiveDirection]) throws {
        guard mode == .warp, !layout.quads.isEmpty else { return }
        var next = layout
        for q in layout.quads.indices {
            for k in 0..<4 {
                for direction in directions {
                    next.straighten.append(PerspectiveStraighten(quad: q, edge: k, direction: direction))
                }
            }
        }
        var copy = self
        copy.layout = try resolved(next)
        try copy.refreshPreview()
        self = copy
    }

    /// Remove Warp: every corner back where it stands, straightening cleared.
    mutating func removeWarp() throws {
        var next = layout
        for v in next.vertices.indices { next.vertices[v].to = next.vertices[v].from }
        next.straighten = []
        var copy = self
        copy.layout = try resolved(next)
        try copy.refreshPreview()
        self = copy
    }
}
