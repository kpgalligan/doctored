import AppKit

// The Pen tool's session: what it is editing, which open subpath the next click extends,
// the gesture in flight and the pointer. The canvas holds one (`ImageCanvasView.penSession`) and
// points events at it (`ImageCanvasView+Pen.swift`); the commit is the controller's
// (`EditorViewController+Pen.swift`). Nothing here touches the document: a gesture builds the
// WHOLE next shape in `PenDrag.shape`, the canvas draws that while the button is down, and the
// release hands it over as one edit. Hit testing, inserting on a segment and converting an anchor
// are the core's (`RasterPathCore`); the rest is moving points.

/// One Pen gesture, from its press to its release.
struct PenDrag: Equatable {
    enum Kind: Equatable {
        /// A click whose result was decided at the press — close, delete,
        /// insert, convert. Dragging changes nothing.
        case fixed
        /// A new anchor. Dragging pulls a mirrored pair of handles out of it.
        case placing(subpath: Int, index: Int)
        /// ⌘ on an anchor: it moves, its handles with it.
        case anchor(subpath: Int, index: Int, grab: CGPoint, start: PathAnchor)
        /// ⌘ on a handle: it follows the pointer, and a smooth anchor's
        /// other handle stays in line with it.
        case handle(subpath: Int, index: Int, out: Bool)
    }

    var kind: Kind
    /// The whole shape as the gesture leaves it: what the canvas draws
    /// mid-gesture and what the release commits.
    var shape: PathShape
    /// The undo step's name.
    var name: String
    /// The subpath the next click extends once this gesture has committed.
    var activeAfter: Int?
}

struct PenSession: Equatable {
    /// How near an anchor or a handle a press must land, in screen points.
    static let anchorHitPoints: CGFloat = 6
    /// How near a segment a click must land to insert an anchor on it.
    /// Tighter than an anchor's reach, so a click beside a curve still
    /// places an anchor of its own.
    static let segmentHitPoints: CGFloat = 4
    /// A press that travels less than this, in screen points, is a click:
    /// the anchor it places stays a corner.
    static let dragScreenPoints: CGFloat = 2

    /// What the Pen edits: the document's `currentPath`, pushed in with
    /// `shape` on every document change.
    var target: PathTarget?
    /// The target's shape as the document holds it, or nil when there is no
    /// current path. A cache: reading it crosses the FFI boundary.
    var shape: PathShape?
    /// The open subpath of `shape` the next click extends; nil when the next
    /// click starts a new one.
    var activeSubpath: Int?
    var drag: PenDrag?
    /// The pointer, for the rubber band.
    var hover: CGPoint?
    /// The tool's Rubber Band option, pushed in beside `shape`.
    var rubberBand = true

    /// What the canvas draws: the gesture's shape while one is in flight,
    /// else the document's.
    var displayShape: PathShape? { drag?.shape ?? shape }

    /// The subpath of `displayShape` whose last anchor the next click
    /// continues from.
    var displayedActiveSubpath: Int? {
        if let drag { return drag.activeAfter }
        return shape.flatMap { openSubpath(in: $0) }
    }

    /// The segment the next click would add: from the open subpath's last
    /// anchor to the pointer. Nil while a gesture is in flight, with the
    /// option off, and with no open subpath.
    var rubberBandSegment: (from: PathAnchor, to: CGPoint)? {
        guard rubberBand, drag == nil, let hover, let shape,
              let index = openSubpath(in: shape), let last = shape.subpaths[index].anchors.last
        else { return nil }
        return (last, hover)
    }

    private func openSubpath(in shape: PathShape) -> Int? {
        guard let index = activeSubpath, shape.subpaths.indices.contains(index),
              !shape.subpaths[index].closed
        else { return nil }
        return index
    }

    private static func anchor(_ shape: PathShape, _ subpath: Int, _ index: Int) -> PathAnchor? {
        guard shape.subpaths.indices.contains(subpath),
              shape.subpaths[subpath].anchors.indices.contains(index)
        else { return nil }
        return shape.subpaths[subpath].anchors[index]
    }

    // MARK: - Document changes

    /// The document's answer after a change. A different target, or a shape
    /// that moved under a gesture (an undo, an agent's edit), drops the
    /// gesture: its shape was built on what is no longer there. The active
    /// subpath survives while it is still an open subpath of the new shape,
    /// so an undo of the last anchor leaves the Pen drawing.
    mutating func adopt(target: PathTarget?, shape: PathShape?, rubberBand: Bool) {
        if target != self.target {
            activeSubpath = nil
            drag = nil
        }
        if shape != self.shape { drag = nil }
        self.target = target
        self.shape = shape
        self.rubberBand = rubberBand
        guard let shape, openSubpath(in: shape) != nil else {
            activeSubpath = nil
            return
        }
    }

    /// Ends the open subpath and any gesture: Escape, Return, a tool change.
    mutating func end() {
        activeSubpath = nil
        drag = nil
        hover = nil
    }

    // MARK: - Gestures

    /// A press at `point` (canvas coordinates). False for a press the path
    /// refuses — an anchor that cannot be converted, a shape too full to
    /// take another — which the caller answers with a beep. A press on
    /// nothing a modifier applies to is taken and does nothing.
    ///
    /// ⌘ moves the anchor or handle under the pointer; ⌥ converts the anchor
    /// under it. Otherwise, in this order: the open subpath's first anchor
    /// closes it (from two anchors up); any other anchor is deleted; a
    /// segment takes a new anchor; anywhere else places an anchor at the end
    /// of the open subpath, or starts a subpath.
    mutating func press(
        at point: CGPoint, modifiers: NSEvent.ModifierFlags, magnification: CGFloat
    ) -> Bool {
        drag = nil
        let base = shape ?? PathShape()
        let active = openSubpath(in: base)
        let scale = Double(magnification > 0 ? magnification : 1)
        // Only ⌘ can grab a handle. Every other press asks for anchors and
        // segments alone, so a handle tip lying beside a segment does not
        // hide the segment from a click that means to insert on it.
        let hit = RasterPathCore.hit(
            base, at: point, tolerance: Double(Self.anchorHitPoints) / scale,
            handles: modifiers.contains(.command))

        if modifiers.contains(.command) {
            switch hit {
            case .anchor(let subpath, let index):
                guard let start = Self.anchor(base, subpath, index) else { return true }
                drag = PenDrag(
                    kind: .anchor(subpath: subpath, index: index, grab: point, start: start),
                    shape: base, name: "Move Anchor Point", activeAfter: active)
            case .handleIn(let subpath, let index):
                drag = PenDrag(
                    kind: .handle(subpath: subpath, index: index, out: false),
                    shape: base, name: "Move Handle", activeAfter: active)
            case .handleOut(let subpath, let index):
                drag = PenDrag(
                    kind: .handle(subpath: subpath, index: index, out: true),
                    shape: base, name: "Move Handle", activeAfter: active)
            case .segment, .none:
                break
            }
            return true
        }

        if modifiers.contains(.option) {
            guard case .anchor(let subpath, let index) = hit,
                  let anchor = Self.anchor(base, subpath, index)
            else { return true }
            // A corner becomes smooth; anything with a handle out, a corner.
            let isCorner = !anchor.smooth && anchor.handleIn == anchor.point
                && anchor.handleOut == anchor.point
            guard let converted = try? RasterPathCore.convertingAnchor(
                    base, subpath: subpath, anchor: index, smooth: isCorner),
                  converted != base
            else { return false }
            drag = PenDrag(
                kind: .fixed, shape: converted, name: "Convert Anchor Point", activeAfter: active)
            return true
        }

        if case .anchor(let subpath, let index) = hit, Self.anchor(base, subpath, index) != nil {
            var next = base
            if subpath == active, index == 0, base.subpaths[subpath].anchors.count >= 2 {
                next.subpaths[subpath].closed = true
                drag = PenDrag(kind: .fixed, shape: next, name: "Close Path", activeAfter: nil)
                return true
            }
            next.subpaths[subpath].anchors.remove(at: index)
            var after = active
            // The core refuses a subpath with no anchor: its last one takes
            // the subpath with it, and the ones after it move down.
            if next.subpaths[subpath].anchors.isEmpty {
                next.subpaths.remove(at: subpath)
                if let current = active {
                    after = current == subpath ? nil : (current > subpath ? current - 1 : current)
                }
            }
            drag = PenDrag(
                kind: .fixed, shape: next, name: "Delete Anchor Point", activeAfter: after)
            return true
        }

        // Asked again at the segment's own reach rather than read off `hit`:
        // that answer used the anchors' wider one. `.none` there is `.none`
        // here too.
        if hit != .none,
           case .segment(let subpath, let index, let t) = RasterPathCore.hit(
               base, at: point, tolerance: Double(Self.segmentHitPoints) / scale,
               handles: false) {
            guard let next = try? RasterPathCore.insertingAnchor(
                base, subpath: subpath, segment: index, t: t)
            else { return false }
            drag = PenDrag(kind: .fixed, shape: next, name: "Add Anchor Point", activeAfter: active)
            return true
        }

        var next = base
        let subpath: Int
        if let active {
            next.subpaths[active].anchors.append(.corner(point))
            subpath = active
        } else {
            next.subpaths.append(PathSubpath(anchors: [.corner(point)], closed: false))
            subpath = next.subpaths.count - 1
        }
        drag = PenDrag(
            kind: .placing(subpath: subpath, index: next.subpaths[subpath].anchors.count - 1),
            shape: next, name: "New Anchor Point", activeAfter: subpath)
        return true
    }

    /// A drag tick: the gesture's shape follows the pointer.
    mutating func dragged(to point: CGPoint, magnification: CGFloat) {
        guard var drag else { return }
        switch drag.kind {
        case .fixed:
            return
        case .placing(let subpath, let index):
            guard var anchor = Self.anchor(drag.shape, subpath, index) else { return }
            let origin = anchor.point
            if hypot(point.x - origin.x, point.y - origin.y) * magnification
                < Self.dragScreenPoints {
                anchor = .corner(origin)
            } else {
                anchor.handleOut = point
                anchor.handleIn = CGPoint(x: 2 * origin.x - point.x, y: 2 * origin.y - point.y)
                anchor.smooth = true
            }
            drag.shape.subpaths[subpath].anchors[index] = anchor
        case .anchor(let subpath, let index, let grab, let start):
            guard Self.anchor(drag.shape, subpath, index) != nil else { return }
            let dx = point.x - grab.x, dy = point.y - grab.y
            func moved(_ from: CGPoint) -> CGPoint { CGPoint(x: from.x + dx, y: from.y + dy) }
            drag.shape.subpaths[subpath].anchors[index] = PathAnchor(
                point: moved(start.point), handleIn: moved(start.handleIn),
                handleOut: moved(start.handleOut), smooth: start.smooth)
        case .handle(let subpath, let index, let out):
            guard var anchor = Self.anchor(drag.shape, subpath, index) else { return }
            let origin = anchor.point
            var other = out ? anchor.handleIn : anchor.handleOut
            // A smooth anchor's other handle turns to stay in line, keeping
            // its own length; one sitting on the point has no length to keep.
            let reach = hypot(point.x - origin.x, point.y - origin.y)
            let length = hypot(other.x - origin.x, other.y - origin.y)
            if anchor.smooth, reach > 0, length > 0 {
                other = CGPoint(
                    x: origin.x - (point.x - origin.x) / reach * length,
                    y: origin.y - (point.y - origin.y) / reach * length)
            }
            anchor.handleOut = out ? point : other
            anchor.handleIn = out ? other : point
            drag.shape.subpaths[subpath].anchors[index] = anchor
        }
        self.drag = drag
    }

    /// The release: the gesture to commit, or nil when it changed nothing
    /// (a ⌘ press that never moved) or there was none.
    mutating func release() -> PenDrag? {
        guard let finished = drag else { return nil }
        drag = nil
        return finished.shape == (shape ?? PathShape()) ? nil : finished
    }
}
