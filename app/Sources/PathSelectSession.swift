import AppKit

// Path Selection and Direct Selection (`designs/v1-paths.md`): the session both tools share. The
// canvas holds one (`ImageCanvasView.pathSelectSession`) and hands it events
// (`ImageCanvasView+PathSelect.swift`); the commit is the Pen's (`EditorViewController+Pen.swift`,
// `EditorViewController+PathSelect.swift`). The path itself is not held here: every press is
// handed the canvas's one cache (`PenSession.shape`) as `base`, a gesture builds the WHOLE next
// shape in `PathSelectDrag.shape`, and the release hands that over as one edit. The SELECTION is
// view state, like the current path: indices into the shape it was made on (`selectionBase`),
// dropped whenever that shape changes in a way this session did not make (an undo, an agent's
// edit) and whenever the tool leaves the two. Hit testing and the subpath under a point are the
// core's (`RasterPathCore`); the rest is moving points.

/// One anchor of a shape: its subpath and its index there.
struct PathAnchorRef: Hashable {
    var subpath: Int
    var index: Int
}

/// What the two tools have selected: Path Selection whole subpaths, Direct
/// Selection anchors. At most one of the two is non-empty.
struct PathSelection: Equatable {
    var subpaths: Set<Int> = []
    var anchors: Set<PathAnchorRef> = []

    var isEmpty: Bool { subpaths.isEmpty && anchors.isEmpty }
}

/// What a selection-tool gesture does while the button is down.
enum PathSelectDragKind: Equatable {
    /// The selected subpaths or anchors follow the pointer from `grab`, each
    /// moved from where it was in `start`.
    case move(grab: CGPoint, start: PathShape)
    /// One handle of a selected anchor follows the pointer (the Pen's
    /// ⌘-handle drag).
    case handle(subpath: Int, index: Int, out: Bool)
    /// A rectangle from `origin`: its release selects what lies inside —
    /// anchors when `anchors`, else subpaths; `extend` (⇧ at the press) adds
    /// to the selection instead of replacing it.
    case marquee(origin: CGPoint, extend: Bool, anchors: Bool)
}

/// One selection-tool gesture, from its press to its release.
struct PathSelectDrag: Equatable {
    var kind: PathSelectDragKind
    /// The whole shape as the gesture leaves it: what the canvas draws
    /// mid-gesture and what the release commits.
    var shape: PathShape
    /// The undo step's name.
    var name: String
    /// Where the press landed. Nothing follows the pointer until it leaves
    /// the click radius around it, and a press that never does commits
    /// nothing.
    var press: CGPoint
    var moved = false
    /// The marquee's rectangle so far.
    var marquee: CGRect?
    /// An ⌥-press's selection after it took the pressed subpath and before
    /// the copies replaced it: what a release that never moved selects,
    /// since no copy was placed.
    var originals: Set<Int>?
}

/// A finished gesture's whole shape and undo name.
struct PathSelectCommit: Equatable {
    var shape: PathShape
    var name: String
}

struct PathSelectSession: Equatable {
    /// A press that travels less than this, in screen points, is a click.
    static let dragScreenPoints: CGFloat = 3

    /// The target the selection's indices refer to.
    private(set) var target: PathTarget?
    /// The shape the selection's indices refer to.
    private(set) var selectionBase: PathShape?
    private(set) var selection = PathSelection()
    var drag: PathSelectDrag?
    /// The shape a commit in flight will make, so the document's answer is
    /// recognised as this session's own edit (it comes back with every
    /// coordinate quantized to the core's four decimals).
    private var expecting: PathShape?
    /// The selection before the gesture in flight or committing, put back
    /// when it commits nothing.
    private var selectionBefore: PathSelection?

    var hasSelection: Bool { !selection.isEmpty }

    /// The subpaths the selection names: Path Selection's own, or the
    /// subpaths of Direct Selection's anchors. Sorted.
    var selectedSubpaths: [Int] {
        Array(selection.subpaths.union(selection.anchors.map(\.subpath))).sorted()
    }

    // MARK: - Document changes and tool switches

    /// The document's answer after a change. A different target, or a shape
    /// this session did not make, drops the selection and any gesture: the
    /// indices were made on what is no longer there. The shape a commit of
    /// this session just made keeps the selection.
    mutating func adopt(target: PathTarget?, shape: PathShape?) {
        if target == self.target, shape == selectionBase { return }
        if target == self.target, let expecting, let shape, Self.matches(shape, expecting) {
            selectionBase = shape
            self.expecting = nil
            return
        }
        reset(target: target, shape: shape)
    }

    private mutating func reset(target: PathTarget?, shape: PathShape?) {
        self.target = target
        selectionBase = shape
        selection = PathSelection()
        drag = nil
        expecting = nil
        selectionBefore = nil
    }

    /// Whether `a` is `b` as the core stores it: the same structure, every
    /// coordinate within the core's four-decimal quantization.
    static func matches(_ a: PathShape, _ b: PathShape) -> Bool {
        guard a.fillRule == b.fillRule, a.subpaths.count == b.subpaths.count else { return false }
        func near(_ p: CGPoint, _ q: CGPoint) -> Bool {
            abs(p.x - q.x) <= 1e-3 && abs(p.y - q.y) <= 1e-3
        }
        return zip(a.subpaths, b.subpaths).allSatisfy { s, t in
            s.closed == t.closed && s.anchors.count == t.anchors.count
                && zip(s.anchors, t.anchors).allSatisfy { p, q in
                    p.smooth == q.smooth && near(p.point, q.point)
                        && near(p.handleIn, q.handleIn) && near(p.handleOut, q.handleOut)
                }
        }
    }

    /// Drops the selection and any gesture: Escape, a command that replaced
    /// the path.
    mutating func clearSelection() {
        selection = PathSelection()
        drag = nil
        selectionBefore = nil
    }

    /// The tool changed from `old` to `new`. Between the two tools the
    /// selection carries over (a subpath becomes all its anchors, an anchor
    /// its subpath); any other tool drops it, so no command acts on a
    /// selection nobody can see.
    mutating func toolChanged(from old: EditorTool, to new: EditorTool) {
        guard old != new else { return }
        drag = nil
        selectionBefore = nil
        switch (old, new) {
        case (.pathSelect, .directSelect):
            var anchors: Set<PathAnchorRef> = []
            for subpath in selection.subpaths {
                guard let base = selectionBase, base.subpaths.indices.contains(subpath) else {
                    continue
                }
                for index in base.subpaths[subpath].anchors.indices {
                    anchors.insert(PathAnchorRef(subpath: subpath, index: index))
                }
            }
            selection = PathSelection(anchors: anchors)
        case (.directSelect, .pathSelect):
            selection = PathSelection(subpaths: Set(selection.anchors.map(\.subpath)))
        default:
            clearSelection()
        }
    }

    // MARK: - Gestures

    /// A press at `point` (canvas coordinates) on `base`, the current path
    /// `target`, with `tool` one of the two.
    ///
    /// Path Selection: a press on a subpath's outline or inside its fill
    /// (`RasterPathCore.subpath`) takes that subpath — alone, unless it is
    /// already selected, so a selected group drags together; ⇧ toggles it;
    /// ⌥ drags copies of the selected subpaths, appended last and selected.
    /// Direct Selection: a handle of a selected anchor drags that handle; an
    /// anchor is taken as a subpath is; a segment takes its two ends. A press
    /// on nothing starts a marquee.
    mutating func press(
        at point: CGPoint, modifiers: NSEvent.ModifierFlags, magnification: CGFloat,
        base: PathShape, target: PathTarget, tool: EditorTool
    ) {
        if target != self.target || base != selectionBase { reset(target: target, shape: base) }
        drag = nil
        selectionBefore = selection
        let scale = Double(magnification > 0 ? magnification : 1)
        let tolerance = Double(PenSession.anchorHitPoints) / scale
        let extend = modifiers.contains(.shift)
        if tool == .directSelect {
            pressAnchor(at: point, base: base, tolerance: tolerance, extend: extend)
        } else {
            pressSubpath(
                at: point, base: base, tolerance: tolerance, extend: extend,
                copy: modifiers.contains(.option))
        }
    }

    private mutating func pressSubpath(
        at point: CGPoint, base: PathShape, tolerance: Double, extend: Bool, copy: Bool
    ) {
        let taken: Int?
        switch RasterPathCore.hit(base, at: point, tolerance: tolerance, handles: false) {
        case .anchor(let subpath, _), .segment(let subpath, _, _): taken = subpath
        case .handleIn, .handleOut, .none: taken = RasterPathCore.subpath(base, at: point)
        }
        guard let subpath = taken else {
            drag = PathSelectDrag(
                kind: .marquee(origin: point, extend: extend, anchors: false), shape: base,
                name: "", press: point)
            return
        }
        if extend, selection.subpaths.contains(subpath) {
            selection.subpaths.remove(subpath)
            selectionBefore = nil
            return
        }
        if extend {
            selection.subpaths.insert(subpath)
        } else if !selection.subpaths.contains(subpath) {
            selection.subpaths = [subpath]
        }
        var start = base
        var name = "Move Subpaths"
        var originals: Set<Int>?
        if copy {
            // The copies go last, in their order, and become the selection.
            originals = selection.subpaths
            var copies: Set<Int> = []
            for index in selection.subpaths.sorted() {
                copies.insert(start.subpaths.count)
                start.subpaths.append(base.subpaths[index])
            }
            selection.subpaths = copies
            name = "Duplicate Subpaths"
        }
        drag = PathSelectDrag(
            kind: .move(grab: point, start: start), shape: start, name: name, press: point,
            originals: originals)
    }

    private mutating func pressAnchor(
        at point: CGPoint, base: PathShape, tolerance: Double, extend: Bool
    ) {
        var hit = RasterPathCore.hit(base, at: point, tolerance: tolerance, handles: true)
        // Only a selected anchor shows its handles, so only its handles can
        // be grabbed; a press near anyone else's is asked again without.
        switch hit {
        case .handleIn(let subpath, let index), .handleOut(let subpath, let index):
            if !selection.anchors.contains(PathAnchorRef(subpath: subpath, index: index)) {
                hit = RasterPathCore.hit(base, at: point, tolerance: tolerance, handles: false)
            }
        case .anchor, .segment, .none:
            break
        }
        let taken: Set<PathAnchorRef>
        switch hit {
        case .handleIn(let subpath, let index):
            drag = PathSelectDrag(
                kind: .handle(subpath: subpath, index: index, out: false), shape: base,
                name: "Move Handle", press: point)
            return
        case .handleOut(let subpath, let index):
            drag = PathSelectDrag(
                kind: .handle(subpath: subpath, index: index, out: true), shape: base,
                name: "Move Handle", press: point)
            return
        case .anchor(let subpath, let index):
            taken = [PathAnchorRef(subpath: subpath, index: index)]
        case .segment(let subpath, let index, _):
            let count = max(base.subpaths[subpath].anchors.count, 1)
            taken = [
                PathAnchorRef(subpath: subpath, index: index),
                PathAnchorRef(subpath: subpath, index: (index + 1) % count),
            ]
        case .none:
            drag = PathSelectDrag(
                kind: .marquee(origin: point, extend: extend, anchors: true), shape: base,
                name: "", press: point)
            return
        }
        if extend, taken.isSubset(of: selection.anchors) {
            selection.anchors.subtract(taken)
            selectionBefore = nil
            return
        }
        if extend {
            selection.anchors.formUnion(taken)
        } else if !taken.isSubset(of: selection.anchors) {
            selection.anchors = taken
        }
        drag = PathSelectDrag(
            kind: .move(grab: point, start: base), shape: base, name: "Move Anchor Points",
            press: point)
    }

    /// A drag tick: the gesture's shape, or its marquee, follows the pointer
    /// once it has left the click radius.
    mutating func dragged(to point: CGPoint, magnification: CGFloat) {
        guard var drag else { return }
        let travel = hypot(point.x - drag.press.x, point.y - drag.press.y)
            * (magnification > 0 ? magnification : 1)
        if !drag.moved, travel < Self.dragScreenPoints { return }
        drag.moved = true
        switch drag.kind {
        case .move(let grab, let start):
            let dx = point.x - grab.x, dy = point.y - grab.y
            var shape = start
            for subpath in selection.subpaths where start.subpaths.indices.contains(subpath) {
                shape.subpaths[subpath].anchors = start.subpaths[subpath].anchors.map {
                    $0.moved(dx: dx, dy: dy)
                }
            }
            for ref in selection.anchors where start.subpaths.indices.contains(ref.subpath)
                && start.subpaths[ref.subpath].anchors.indices.contains(ref.index) {
                shape.subpaths[ref.subpath].anchors[ref.index] =
                    start.subpaths[ref.subpath].anchors[ref.index].moved(dx: dx, dy: dy)
            }
            drag.shape = shape
        case .handle(let subpath, let index, let out):
            guard drag.shape.subpaths.indices.contains(subpath),
                  drag.shape.subpaths[subpath].anchors.indices.contains(index)
            else { return }
            drag.shape.subpaths[subpath].anchors[index] =
                drag.shape.subpaths[subpath].anchors[index].movingHandle(out: out, to: point)
        case .marquee(let origin, _, _):
            drag.marquee = CGRect(
                x: min(origin.x, point.x), y: min(origin.y, point.y),
                width: abs(point.x - origin.x), height: abs(point.y - origin.y))
        }
        self.drag = drag
    }

    /// The release: the gesture to commit, or nil when it changed nothing (a
    /// click, a marquee) or there was none. A marquee's release selects every
    /// subpath with an anchor inside it (Path Selection) or every anchor
    /// inside it (Direct Selection); a click on nothing clears the selection
    /// (⇧ keeps it).
    mutating func release() -> PathSelectCommit? {
        guard let finished = drag else { return nil }
        drag = nil
        if case .marquee(_, let extend, let anchors) = finished.kind {
            selectInside(finished.moved ? finished.marquee : nil, extend: extend, anchors: anchors)
            selectionBefore = nil
            return nil
        }
        guard finished.moved, finished.shape != selectionBase else {
            // A click. An ⌥-click's copies were never placed, so the
            // selection they took goes back to the originals the press took.
            if let originals = finished.originals {
                selection = PathSelection(subpaths: originals)
            }
            selectionBefore = nil
            return nil
        }
        expecting = finished.shape
        return PathSelectCommit(shape: finished.shape, name: finished.name)
    }

    private mutating func selectInside(_ rect: CGRect?, extend: Bool, anchors: Bool) {
        guard let rect, let base = selectionBase else {
            if !extend { selection = PathSelection() }
            return
        }
        var inside = PathSelection()
        for (subpath, path) in base.subpaths.enumerated() {
            for (index, anchor) in path.anchors.enumerated() where rect.contains(anchor.point) {
                if anchors {
                    inside.anchors.insert(PathAnchorRef(subpath: subpath, index: index))
                } else {
                    inside.subpaths.insert(subpath)
                }
            }
        }
        guard extend else {
            selection = inside
            return
        }
        selection.subpaths.formUnion(inside.subpaths)
        selection.anchors.formUnion(inside.anchors)
    }

    /// The commit of the gesture `release` or `deleteSelection` returned has
    /// ended: `accepted` when the document took it. A refused one puts the
    /// selection back.
    mutating func commitEnded(accepted: Bool) {
        if accepted, let expecting {
            // Not adopted yet (no notification reached the session): the
            // indices already refer to the committed shape.
            selectionBase = expecting
        } else if !accepted, let before = selectionBefore {
            selection = before
        }
        expecting = nil
        selectionBefore = nil
    }

    /// Escape mid-gesture: the gesture is dropped and the selection it began
    /// with comes back.
    mutating func cancelDrag() {
        guard drag != nil else { return }
        drag = nil
        if let before = selectionBefore { selection = before }
        selectionBefore = nil
    }

    /// ⌫: the selected subpaths (Path Selection) or anchors (Direct
    /// Selection) removed from `base`, as the edit to commit. An anchor goes
    /// by the Pen's rule: a subpath left with no anchor goes with its last
    /// one. Nil with nothing selected, mid-gesture, or on a shape the
    /// selection was not made on.
    mutating func deleteSelection(base: PathShape) -> PathSelectCommit? {
        guard drag == nil, base == selectionBase, hasSelection else { return nil }
        var next = base
        let name: String
        if !selection.anchors.isEmpty {
            for subpath in Set(selection.anchors.map(\.subpath)).sorted(by: >)
            where next.subpaths.indices.contains(subpath) {
                let doomed = selection.anchors.filter { $0.subpath == subpath }.map(\.index)
                for index in doomed.sorted(by: >)
                where next.subpaths[subpath].anchors.indices.contains(index) {
                    next.subpaths[subpath].anchors.remove(at: index)
                }
                if next.subpaths[subpath].anchors.isEmpty { next.subpaths.remove(at: subpath) }
            }
            name = "Delete Anchor Points"
        } else {
            for subpath in selection.subpaths.sorted(by: >)
            where next.subpaths.indices.contains(subpath) {
                next.subpaths.remove(at: subpath)
            }
            name = "Delete Subpaths"
        }
        guard next != base else { return nil }
        selectionBefore = selection
        selection = PathSelection()
        expecting = next
        return PathSelectCommit(shape: next, name: name)
    }
}
