import AppKit

// Path booleans: Doctored's rule is that A PATH OPERATION PRODUCES A PATH. Two paths' fill regions
// are combined geometrically, by CoreGraphics' path booleans on the host (the ones
// `ShapeLayer.strokeOutline` already aligns a stroke with), and the result is an ordinary path of
// the list: it fills, strokes, selects and becomes a shape layer like any other, and nothing new
// is stored. Paths panel ▸ Combine with Path… and `combine_paths` run it
// (EditorViewController+PathCommands.swift, AgentServer+PathCombine.swift), and so do the
// selection tools' Combine Selected Subpaths and `combine_paths` with `subpath_indices`
// (`combineSubpaths`, EditorViewController+PathSelect.swift).

/// What `combine_paths` does to a path's fill region with another's; the raw value is the MCP
/// spelling.
enum PathCombineOperation: String, CaseIterable {
    /// Everything either fills.
    case unite
    /// The path's region less the other's.
    case subtract
    /// Only what both fill.
    case intersect
    /// What exactly one of the two fills.
    case exclude

    var title: String {
        switch self {
        case .unite: return "Unite"
        case .subtract: return "Subtract"
        case .intersect: return "Intersect"
        case .exclude: return "Exclude"
        }
    }
}

enum PathCombine {
    /// The refusal of a combine whose operand holds operations that leave it unbounded
    /// (`PathShape.isUnbounded`): its region covers everything outside its subpaths.
    static let unboundedRefusal =
        "This path's first subpath subtracts and none intersects, so its region has no outline: "
        + "start it with a unite subpath, or intersect it."

    /// `a`'s fill region combined with `b`'s by `operation`, as a path. Each operand is its fill
    /// region under its own fill rule, an open subpath closed by a straight line as a fill closes
    /// it, or the fold of the operations it holds (`foldedRegion`); the result fills by nonzero,
    /// holds no operations, its subpaths are closed and its anchors corners with CoreGraphics' own
    /// handles (`PathShape(cgPath:fillRule:)`). Empty when nothing is left; nil when an operand
    /// is unbounded (`unboundedRefusal`).
    static func combine(_ a: PathShape, _ b: PathShape, _ operation: PathCombineOperation)
        -> PathShape? {
        guard let first = region(a), let second = region(b) else { return nil }
        return PathShape(cgPath: boolean(first, second, operation), fillRule: .nonZero)
    }

    /// The SELECTED subpaths against the rest: the region `shape`'s unselected subpaths fill and
    /// the region its `selected` ones fill, each under `shape`'s own fill rule, combined by
    /// `operation` as `combine` combines two paths, the result replacing the whole shape.
    /// Nothing is special-cased: with every subpath selected the first operand is empty, so
    /// Unite and Exclude give the path's own region as one merged outline and Subtract and
    /// Intersect leave nothing. A shape that HOLDS operations is combined in its own order
    /// instead: the selected subpaths take `operation`, and the whole fold becomes the geometry,
    /// holding none; nil when that fold is unbounded. Indices past the shape are ignored; the
    /// callers refuse them.
    static func combineSubpaths(
        _ shape: PathShape, selected: Set<Int>, _ operation: PathCombineOperation
    ) -> PathShape? {
        if shape.ops != nil {
            let given = shape.settingOp(operation, at: selected.sorted())
            return foldedRegion(given).map { PathShape(cgPath: $0, fillRule: .nonZero) }
        }
        var rest = PathShape(fillRule: shape.fillRule)
        var chosen = PathShape(fillRule: shape.fillRule)
        for (index, subpath) in shape.subpaths.enumerated() {
            if selected.contains(index) {
                chosen.subpaths.append(subpath)
            } else {
                rest.subpaths.append(subpath)
            }
        }
        return combine(rest, chosen, operation)
    }

    /// The region a shape that holds operations fills, as a path that fills it by the winding
    /// rule (the core's fold, `artwork_ops_coverage`): each subpath's own region under the
    /// shape's fill rule, folded in order from nothing, or from everything when the first
    /// subtracts. The running region is tracked as `P` or as "everything less `P`", so no
    /// universe rectangle is needed: unite, subtract and exclude keep a complement a complement
    /// (everything less `P` united with `S` is everything less `P \ S`, less `S` is everything less
    /// `P ∪ S`, excluding `S` is everything less `P ⊕ S`) and an intersect ends it (`S \ P`). A
    /// subpath whose box has no width or height (one anchor without a loop handle, a horizontal
    /// or vertical line) is the empty region, so an intersect by it empties the fold, as in the
    /// core. nil without operations, and when
    /// the fold ends complemented: an unbounded region has no outline.
    static func foldedRegion(_ shape: PathShape) -> CGPath? {
        guard let ops = shape.ops else { return nil }
        var running: CGPath = CGMutablePath()
        var complemented = ops.first == .subtract
        for (subpath, op) in zip(shape.subpaths, ops) {
            var own = PathShape(fillRule: shape.fillRule)
            own.subpaths = [subpath]
            let region = plainRegion(own)
            switch (op, complemented) {
            case (_, false):
                running = boolean(running, region, op)
            case (.unite, true):
                running = boolean(running, region, .subtract)
            case (.subtract, true):
                running = boolean(running, region, .unite)
            case (.intersect, true):
                running = boolean(region, running, .subtract)
                complemented = false
            case (.exclude, true):
                running = boolean(running, region, .exclude)
            }
        }
        return complemented ? nil : running
    }

    /// CoreGraphics' boolean of two winding-rule regions.
    private static func boolean(
        _ first: CGPath, _ second: CGPath, _ operation: PathCombineOperation
    ) -> CGPath {
        switch operation {
        case .unite: return first.union(second, using: .winding)
        case .subtract: return first.subtracting(second, using: .winding)
        case .intersect: return first.intersection(second, using: .winding)
        case .exclude: return first.symmetricDifference(second, using: .winding)
        }
    }

    /// `shape`'s fill region as a winding-rule path: the fold of the operations it holds
    /// (`foldedRegion`, nil when unbounded), or its subpaths' own (`plainRegion`).
    private static func region(_ shape: PathShape) -> CGPath? {
        shape.ops != nil ? foldedRegion(shape) : plainRegion(shape)
    }

    /// `shape`'s fill region, its operations aside, as a path that fills it by the winding rule,
    /// which every boolean above runs under: an even-odd shape is normalized first
    /// (`normalized(using: .evenOdd)` keeps the region and makes its holes wind the other way),
    /// and an open subpath is closed by the straight line its fill already adds. A region with no
    /// area is the empty path.
    private static func plainRegion(_ shape: PathShape) -> CGPath {
        var closed = shape.droppingOps()
        for index in closed.subpaths.indices where !closed.subpaths[index].closed {
            var subpath = closed.subpaths[index]
            guard let last = subpath.anchors.indices.last else { continue }
            subpath.anchors[last].handleOut = subpath.anchors[last].point
            subpath.anchors[0].handleIn = subpath.anchors[0].point
            subpath.closed = true
            closed.subpaths[index] = subpath
        }
        let path = closed.cgPath()
        let box = path.boundingBoxOfPath
        guard !box.isNull, box.width > 0, box.height > 0 else { return CGMutablePath() }
        return shape.fillRule == .evenOdd ? path.normalized(using: .evenOdd) : path
    }
}
