import AppKit

// Edit ▸ Puppet Warp: the on-canvas session's model. It meshes the active layer once
// (`RasterPuppetMesh`, the core's mesh of the opaque pixels, or of the selection's float),
// keeps the pins in canvas pixels, and on every pin change re-solves the pose in the core and
// re-renders a PROXY of the layer (at most 2,048 px on its longest side, Liquify's rule)
// through it. The commit (`MeshWarpWork`) hands the same pins to the full-size edit, which
// builds the same mesh and solves the same pose: the solve is deterministic, so the preview and
// the commit agree. The canvas half is `ImageCanvasView+PuppetWarp.swift`, the controller half
// `EditorViewController+PuppetWarp.swift`.

/// What a held press is doing.
enum PuppetDrag {
    /// Moving the selected pins; `last` is the previous pointer position.
    case move(last: CGPoint)
    /// ⌥-dragging near a selected pin: its Fixed angle follows the pointer.
    case rotate(pin: Int, startAngle: Double, base: Double)
}

struct PuppetWarpSession {
    static let proxyMaxSide = MeshWarpWorkspace.proxyMaxSide
    /// `puppet_arap::SOLVER_BYTES` restated: the core solver's dense
    /// envelope at the 3,000-vertex cap (8 × 3000 × 3001 / 2) plus 4 MiB.
    static let solverBytes: UInt64 = 40_206_304
    /// `puppet_render::MAP_BYTES` restated: the pose map each preview builds
    /// (20 × 1,000,000 bucket bytes plus 2 MiB).
    static let mapBytes: UInt64 = 22_097_152
    /// A pin's hit radius and the ⌥-rotate reach, in screen points.
    static let pinRadius: CGFloat = 6
    static let rotateReach: CGFloat = 30

    /// The proxy, plates, coverage and lease (`MeshWarpWorkspace`).
    var workspace: MeshWarpWorkspace
    var base: RasterDocument { workspace.base }
    var source: RasterDocument { workspace.source }
    var layer: Int { workspace.layer }
    var layerRect: CGRect { workspace.layerRect }
    var opacity: CGFloat { workspace.opacity }
    var colorSpace: CGColorSpace { workspace.colorSpace }
    var proxy: RasterImage { workspace.proxy }
    var proxyImage: CGImage? { workspace.proxyImage }
    var below: CGImage? { workspace.below }
    var above: CGImage? { workspace.above }
    var coverage: [UInt16]? { workspace.coverage }
    var selectionBounds: CGRect? { workspace.selectionBounds }
    var proxyCoverage: [UInt8]? { workspace.proxyCoverage }
    var coverageMask: CGImage? { workspace.coverageMask }
    var selectionHistory: SelectionHistoryState { workspace.selectionHistory }
    /// The preview's memory lease; dropped while the commit runs.
    var reservation: SelectionWorkBudget.Reservation? {
        get { workspace.reservation }
        set { workspace.reservation = newValue }
    }

    private(set) var options = PuppetWarpOptions()
    private(set) var mesh: RasterPuppetMesh
    private(set) var pins: [PuppetPin] = []
    var selected: Set<Int> = []
    var drag: PuppetDrag?
    /// The deformed vertices (the rest ones with no pin).
    private(set) var positions: [CGPoint]
    /// The proxy through the pose, and the canvas rect it covers; nil with
    /// no pin, when the layer draws as it is.
    private(set) var preview: CGImage?
    private(set) var previewRect = CGRect.zero
    /// True while the session's own commit publishes (`isCommittingTransform`'s
    /// role), so the document change it causes does not end the session.
    var isCommitting = false

    var isFloat: Bool { coverage != nil }
    var isIdentity: Bool { PuppetWarp.isIdentity(pins) }

    /// Opens on layer `layer` of `source`; `base` is the document's handle
    /// and `plates` its stack around the layer. Throws when the layer has no
    /// pixels, the mesh is refused (no pixels at the expansion, a lock, the
    /// working limit), or the workspace does not fit.
    init(document: ImageDocument, base: RasterDocument, source: RasterDocument, layer: Int,
         plates: (below: CGImage?, above: CGImage?)) throws {
        workspace = try MeshWarpWorkspace(
            document: document, base: base, source: source, layer: layer, plates: plates,
            name: PuppetWarp.actionName, coreBytes: Self.solverBytes + Self.mapBytes)
        let built = try source.puppetMesh(layer: layer, coverage: workspace.coverage,
                                          options: PuppetWarpOptions())
        mesh = built
        positions = built.vertices
    }

    /// `MeshWarpWorkspace.workspaceBytes` with the solver and the pose map.
    static func workspaceBytes(canvas: (Int, Int), proxy: (Int, Int)) -> UInt64 {
        MeshWarpWorkspace.workspaceBytes(canvas: canvas, proxy: proxy,
                                         core: solverBytes + mapBytes)
    }

    // MARK: - Solve

    /// Re-solves the pins and re-renders the proxy; with no pin the layer
    /// draws as it is.
    private mutating func refresh() throws {
        guard !pins.isEmpty else {
            positions = mesh.vertices
            preview = nil
            return
        }
        let pose = try mesh.solving(mode: options.mode, pins: pins)
        let rendered = try pose.rendering(proxy, source: layerRect, coverage: proxyCoverage)
        positions = pose.positions
        preview = rendered.image.makeCGImage(in: colorSpace)
        previewRect = rendered.rect
    }

    /// `change` applied to a copy, re-solved, then kept: a refusal leaves
    /// the session as it was.
    private mutating func transaction(_ change: (inout PuppetWarpSession) throws -> Void) throws {
        var next = self
        try change(&next)
        try next.refresh()
        self = next
    }

    // MARK: - Pins

    /// The topmost pin within `radius` canvas pixels of `point`.
    func pin(at point: CGPoint, radius: CGFloat) -> Int? {
        pins.indices.reversed().first { hypot(pins[$0].to.x - point.x, pins[$0].to.y - point.y) <= radius }
    }

    /// The vertex of the deformed triangle under `point` nearest it (the
    /// last such triangle in index order), or nil outside the mesh.
    func vertex(at point: CGPoint) -> Int? {
        for triangle in mesh.triangles.reversed() {
            let p = triangle.map { positions[$0] }
            let d = (p[1].x - p[0].x) * (p[2].y - p[0].y) - (p[2].x - p[0].x) * (p[1].y - p[0].y)
            guard d != 0 else { continue }
            let l1 = ((point.x - p[0].x) * (p[2].y - p[0].y) - (p[2].x - p[0].x) * (point.y - p[0].y)) / d
            let l2 = ((p[1].x - p[0].x) * (point.y - p[0].y) - (point.x - p[0].x) * (p[1].y - p[0].y)) / d
            guard l1 >= 0, l2 >= 0, l1 + l2 <= 1 else { continue }
            return triangle.min { hypot(positions[$0].x - point.x, positions[$0].y - point.y)
                < hypot(positions[$1].x - point.x, positions[$1].y - point.y) }
        }
        return nil
    }

    private static func rounded(_ point: CGPoint) -> CGPoint {
        CGPoint(x: ActionArgs.number(point.x), y: ActionArgs.number(point.y))
    }

    private static func degrees(_ point: CGPoint, about centre: CGPoint) -> Double {
        atan2(Double(point.y - centre.y), Double(point.x - centre.x)) * 180 / .pi
    }

    /// A press at `point` (canvas pixels; `scale` the view's magnification):
    /// on a pin it selects it (Shift toggles) or ⌥ removes it; ⌥ near a
    /// selected pin starts rotating it; elsewhere inside the mesh it adds a
    /// pin at the nearest vertex, or selects the pin already there; outside
    /// it deselects. A press that selects starts a drag of the selection.
    mutating func press(at point: CGPoint, modifiers: NSEvent.ModifierFlags, scale: CGFloat) throws {
        let shift = modifiers.contains(.shift)
        let option = modifiers.contains(.option)
        if let hit = pin(at: point, radius: Self.pinRadius / scale) {
            if option {
                try transaction { session in
                    session.pins.remove(at: hit)
                    session.selected = Set(session.selected.compactMap {
                        $0 == hit ? nil : ($0 > hit ? $0 - 1 : $0)
                    })
                }
                return
            }
            if shift {
                if selected.contains(hit) { selected.remove(hit) } else { selected.insert(hit) }
            } else if !selected.contains(hit) {
                selected = [hit]
            }
            drag = selected.isEmpty ? nil : .move(last: point)
            return
        }
        if option, let near = selected.filter({ hypot(pins[$0].to.x - point.x, pins[$0].to.y - point.y)
                <= Self.rotateReach / scale }).min(by: {
                    hypot(pins[$0].to.x - point.x, pins[$0].to.y - point.y)
                        < hypot(pins[$1].to.x - point.x, pins[$1].to.y - point.y) }) {
            drag = .rotate(pin: near, startAngle: Self.degrees(point, about: pins[near].to),
                           base: pins[near].rotate ?? 0)
            return
        }
        guard let vertex = vertex(at: point) else {
            if !shift { selected = [] }
            drag = nil
            return
        }
        let rest = mesh.vertices[vertex]
        if let existing = pins.firstIndex(where: { $0.from == rest }) {
            if shift { selected.insert(existing) } else { selected = [existing] }
            drag = .move(last: point)
            return
        }
        guard pins.count < PuppetWarp.maxPins else {
            throw DoctoredCoreError(message: "Puppet Warp takes at most 256 pins.")
        }
        try transaction { session in
            session.pins.append(PuppetPin(from: rest, to: Self.rounded(session.positions[vertex])))
            let added = session.pins.count - 1
            if shift { session.selected.insert(added) } else { session.selected = [added] }
        }
        drag = .move(last: point)
    }

    /// The held press moved to `point`: the selected pins follow it, or the
    /// rotated pin's Fixed angle turns with it.
    mutating func dragged(to point: CGPoint) throws {
        guard let drag else { return }
        switch drag {
        case .move(let last):
            let dx = point.x - last.x, dy = point.y - last.y
            guard dx != 0 || dy != 0 else { return }
            try transaction { session in
                for index in session.selected where session.pins.indices.contains(index) {
                    let to = session.pins[index].to
                    session.pins[index].to = Self.rounded(CGPoint(x: to.x + dx, y: to.y + dy))
                }
            }
            self.drag = .move(last: point)
        case let .rotate(index, start, base):
            guard pins.indices.contains(index) else { return }
            let angle = Self.degrees(point, about: pins[index].to)
            try transaction { session in
                session.pins[index].rotate = ActionArgs.number(base + angle - start)
            }
        }
    }

    mutating func released() { drag = nil }

    /// Delete: removes the selected pins.
    mutating func removeSelected() throws {
        guard !selected.isEmpty else { return }
        try transaction { session in
            session.pins = session.pins.indices.filter { !session.selected.contains($0) }
                .map { session.pins[$0] }
            session.selected = []
        }
    }

    /// Remove All Pins.
    mutating func removeAll() throws {
        try transaction { session in
            session.pins = []
            session.selected = []
        }
    }

    // MARK: - Options

    mutating func setMode(_ mode: PuppetMode) throws {
        guard mode != options.mode else { return }
        try transaction { $0.options.mode = mode }
    }

    mutating func setShowMesh(_ show: Bool) { options.showMesh = show }

    /// Density and Expansion rebuild the mesh and re-snap each pin's `from`
    /// to it (the core's snap, through the solve's own refusals): a pin that
    /// now lands outside the mesh is dropped, and pins that now share a
    /// vertex merge, the lowest index kept.
    mutating func setMesh(density: PuppetDensity, expansion: Int) throws {
        var next = options
        next.density = density
        next.expansion = min(PuppetWarp.expansionRange.upperBound,
                             max(PuppetWarp.expansionRange.lowerBound, expansion))
        guard next != options else { return }
        let rebuilt = try source.puppetMesh(layer: layer, coverage: coverage, options: next)
        var kept = Array(pins.indices)
        var snapped: [(vertex: Int, from: CGPoint)] = []
        while !kept.isEmpty {
            do {
                snapped = try rebuilt.solving(mode: options.mode, pins: kept.map { pins[$0] }).snapped
                break
            } catch {
                guard let drop = Self.droppedPin(error.localizedDescription), kept.indices.contains(drop) else {
                    throw error
                }
                kept.remove(at: drop)
            }
        }
        let moved: [PuppetPin] = zip(kept, snapped).map { index, snap in
            var pin = pins[index]
            pin.to = CGPoint(x: snap.from.x + (pin.to.x - pin.from.x),
                             y: snap.from.y + (pin.to.y - pin.from.y))
            pin.from = snap.from
            return pin
        }
        let selection = Set(kept.enumerated().filter { selected.contains($0.element) }.map(\.offset))
        try transaction { session in
            session.options = next
            session.mesh = rebuilt
            session.pins = moved
            session.selected = selection
        }
    }

    /// The pin a re-snap refusal names for dropping: "pin i is outside the
    /// mesh" drops i, "pins i and j share a mesh vertex" drops j.
    private static func droppedPin(_ sentence: String) -> Int? {
        let numbers = sentence.split(whereSeparator: { !$0.isNumber }).compactMap { Int($0) }
        if sentence.hasSuffix("is outside the mesh."), let index = numbers.first { return index }
        if sentence.hasSuffix("share a mesh vertex."), numbers.count == 2 { return numbers[1] }
        return nil
    }

    /// Pin Depth: the selected pins move above (+1) or below (−1) every pin.
    mutating func shiftDepth(_ step: Int) throws {
        guard !selected.isEmpty, !pins.isEmpty else { return }
        let depths = pins.map(\.depth)
        let target = step > 0 ? (depths.max() ?? 0) + 1 : (depths.min() ?? 0) - 1
        let depth = min(PuppetWarp.depthRange.upperBound, max(PuppetWarp.depthRange.lowerBound, target))
        try transaction { session in
            for index in session.selected where session.pins.indices.contains(index) {
                session.pins[index].depth = depth
            }
        }
    }

    /// The one selected pin, when exactly one is.
    var singleSelection: Int? {
        selected.count == 1 ? selected.first.flatMap { pins.indices.contains($0) ? $0 : nil } : nil
    }

    /// Rotate Auto (nil) or Fixed at `degrees` on the one selected pin.
    mutating func setRotate(_ degrees: Double?) throws {
        guard let index = singleSelection else { return }
        try transaction { $0.pins[index].rotate = degrees.map { ActionArgs.number($0) } }
    }
}
