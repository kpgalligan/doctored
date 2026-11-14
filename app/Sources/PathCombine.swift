import AppKit

// Path booleans: Doctored's rule is that A PATH OPERATION PRODUCES A PATH. Two paths' fill regions
// are combined geometrically, by CoreGraphics' path booleans on the host (the ones
// `ShapeLayer.strokeOutline` already aligns a stroke with), and the result is an ordinary path of
// the list: it fills, strokes, selects and becomes a shape layer like any other, and nothing new
// is stored. Paths panel ▸ Combine with Path… and `combine_paths` run it
// (EditorViewController+PathCommands.swift, AgentServer+PathCombine.swift).

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
    /// `a`'s fill region combined with `b`'s by `operation`, as a path. Each operand is its fill
    /// region under its own fill rule, an open subpath closed by a straight line as a fill closes
    /// it; the result fills by nonzero, its subpaths are closed and its anchors corners with
    /// CoreGraphics' own handles (`PathShape(cgPath:fillRule:)`). Empty when nothing is left.
    static func combine(_ a: PathShape, _ b: PathShape, _ operation: PathCombineOperation)
        -> PathShape {
        let first = region(a), second = region(b)
        let result: CGPath
        switch operation {
        case .unite: result = first.union(second, using: .winding)
        case .subtract: result = first.subtracting(second, using: .winding)
        case .intersect: result = first.intersection(second, using: .winding)
        case .exclude: result = first.symmetricDifference(second, using: .winding)
        }
        return PathShape(cgPath: result, fillRule: .nonZero)
    }

    /// `shape`'s fill region as a path that fills it by the winding rule, which every boolean
    /// above runs under: an even-odd shape is normalized first (`normalized(using: .evenOdd)`
    /// keeps the region and makes its holes wind the other way), and an open subpath is closed
    /// by the straight line its fill already adds.
    private static func region(_ shape: PathShape) -> CGPath {
        var closed = shape
        for index in closed.subpaths.indices where !closed.subpaths[index].closed {
            var subpath = closed.subpaths[index]
            guard let last = subpath.anchors.indices.last else { continue }
            subpath.anchors[last].handleOut = subpath.anchors[last].point
            subpath.anchors[0].handleIn = subpath.anchors[0].point
            subpath.closed = true
            closed.subpaths[index] = subpath
        }
        let path = closed.cgPath()
        return shape.fillRule == .evenOdd ? path.normalized(using: .evenOdd) : path
    }
}
