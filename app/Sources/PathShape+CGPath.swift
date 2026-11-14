import AppKit

// The one converter between a path value (`PathShape`, PathModel.swift) and a CoreGraphics path.
// To CoreGraphics it follows the core's outline rule (`PathShape::commands`, path_geometry.rs) so
// what CoreGraphics strokes or fills is the outline the core rasterizes; back from CoreGraphics it
// reads the elements a geometric boolean returns (PathCombine.swift).

extension PathShape {
    /// The shape as CoreGraphics draws and strokes it: a segment whose two handles sit on their
    /// points is a line, any other a cubic; a closed subpath also has the segment from its last
    /// anchor back to its first (a closed subpath of one anchor only when a handle makes it a
    /// loop) and is closed; an OPEN subpath stays open, so a stroke leaves its ends apart while a
    /// fill closes it with a straight line, as the core does. The fill rule is the caller's.
    func cgPath() -> CGPath {
        let path = CGMutablePath()
        for subpath in subpaths {
            guard let first = subpath.anchors.first else { continue }
            path.move(to: first.point)
            func segment(_ a: PathAnchor, _ b: PathAnchor) {
                if a.handleOut == a.point, b.handleIn == b.point {
                    path.addLine(to: b.point)
                } else {
                    path.addCurve(to: b.point, control1: a.handleOut, control2: b.handleIn)
                }
            }
            for index in subpath.anchors.indices.dropFirst() {
                segment(subpath.anchors[index - 1], subpath.anchors[index])
            }
            if subpath.closed, let last = subpath.anchors.last {
                if subpath.anchors.count > 1 || last.handleOut != last.point
                    || last.handleIn != last.point {
                    segment(last, first)
                }
                path.closeSubpath()
            }
        }
        return path
    }

    /// The anchors of a CoreGraphics path, read element by element: a move starts a subpath, a
    /// line adds a corner, a cubic sets the previous anchor's handle out and adds an anchor with
    /// its handle in, a quadratic is raised to the cubic it is exactly, and a close closes the
    /// subpath. Every anchor is a corner (`smooth` false) whatever its handles: CoreGraphics
    /// keeps no editing hint. Three rules make the result the path a person would have drawn:
    /// a segment that returns to its subpath's first point before the close MERGES into that
    /// anchor (its handle in becomes the first anchor's) instead of leaving a second anchor on
    /// the same point; a line of zero length adds nothing; and a subpath left with fewer than two
    /// anchors is dropped, since it has no outline. Points closer than `PathShape.samePoint` count
    /// as equal, which absorbs the last-bit error a boolean leaves on a returning point.
    init(cgPath: CGPath, fillRule: PathFillRule) {
        var result: [PathSubpath] = []
        var current: [PathAnchor] = []
        var start = CGPoint.zero
        func finish(closed: Bool) {
            var anchors = current
            current = []
            if closed, anchors.count > 1, let first = anchors.first, let last = anchors.last,
               Self.samePoint(first.point, last.point) {
                anchors[0].handleIn = last.handleIn == last.point ? first.point : last.handleIn
                anchors.removeLast()
            }
            guard anchors.count >= 2 else { return }
            result.append(PathSubpath(anchors: anchors, closed: closed))
        }
        /// The anchor a segment leaves from: the current subpath's last, or, for a segment that
        /// follows a close with no move, a new subpath at the closed one's start (CoreGraphics'
        /// own rule for that case).
        func ensureOpen() {
            if current.isEmpty { current = [.corner(start)] }
        }
        cgPath.applyWithBlock { pointer in
            let element = pointer.pointee
            switch element.type {
            case .moveToPoint:
                finish(closed: false)
                start = element.points[0]
                current = [.corner(start)]
            case .addLineToPoint:
                ensureOpen()
                let point = element.points[0]
                if let last = current.last, last.handleOut == last.point,
                   Self.samePoint(last.point, point) {
                    return
                }
                current.append(.corner(point))
            case .addQuadCurveToPoint:
                ensureOpen()
                guard let from = current.last?.point else { return }
                let control = element.points[0], point = element.points[1]
                let c1 = CGPoint(x: from.x + 2 / 3 * (control.x - from.x),
                                 y: from.y + 2 / 3 * (control.y - from.y))
                let c2 = CGPoint(x: point.x + 2 / 3 * (control.x - point.x),
                                 y: point.y + 2 / 3 * (control.y - point.y))
                current[current.count - 1].handleOut = c1
                current.append(PathAnchor(point: point, handleIn: c2, handleOut: point, smooth: false))
            case .addCurveToPoint:
                ensureOpen()
                let point = element.points[2]
                current[current.count - 1].handleOut = element.points[0]
                current.append(PathAnchor(
                    point: point, handleIn: element.points[1], handleOut: point, smooth: false))
            case .closeSubpath:
                finish(closed: true)
            @unknown default:
                return
            }
        }
        finish(closed: false)
        self.init(subpaths: result, fillRule: fillRule)
    }

    /// Whether two points of a CoreGraphics result are the same point: within a millionth of a
    /// pixel on each axis, a hundredth of the model's four-decimal quantum, so no two points the
    /// stored path can tell apart are merged.
    static func samePoint(_ a: CGPoint, _ b: CGPoint) -> Bool {
        abs(a.x - b.x) <= 1e-6 && abs(a.y - b.y) <= 1e-6
    }
}
