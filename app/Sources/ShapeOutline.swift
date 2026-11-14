import AppKit

// A shape layer's `path` kind (payload version 7, ShapeLayer.swift): its outline is a Pen path's
// anchors in the shape box's LOCAL space, the box's top-left at (0, 0), so the layer's offset and
// transform place it like every other shape. Paths panel ▸ Convert to Shape Layer and
// `add_shape_layer` with kind "path" make one (EditorViewController+PathCommands.swift,
// AgentServer+Shapes.swift).

/// A path shape's outline: `PathShape` in box-local coordinates. Each anchor is the compact array
/// `[x, y, inX, inY, outX, outY, smooth]` (smooth 0 or 1): the payload is this app's own schema,
/// and the array is about half the size of the path-list JSON's objects at a few thousand anchors.
struct ShapeOutline: Codable, Equatable {
    struct Subpath: Codable, Equatable {
        var closed: Bool
        var anchors: [[Double]]
    }

    /// The path model's caps (`core/src/path_geometry.rs`): one outline holds at most what one
    /// path may. A local coordinate is a canvas one less a whole-pixel origin inside the same
    /// ±1,000,000 wall, so it lies within twice that.
    static let maxSubpaths = 4096
    static let maxAnchors = 32_768
    static let maxLocalCoordinate = 2_000_000.0

    var fillRule: String
    var subpaths: [Subpath]

    enum CodingKeys: String, CodingKey {
        case fillRule = "fill_rule"
        case subpaths
    }

    var evenOdd: Bool { fillRule == PathFillRule.evenOdd.rawValue }

    var anchorCount: Int { subpaths.reduce(0) { $0 + $1.anchors.count } }

    /// `shape` (canvas coordinates) relative to `origin`.
    init(_ shape: PathShape, origin: CGPoint) {
        fillRule = shape.fillRule.rawValue
        subpaths = shape.subpaths.map { subpath in
            Subpath(closed: subpath.closed, anchors: subpath.anchors.map { anchor in
                [Double(anchor.point.x - origin.x), Double(anchor.point.y - origin.y),
                 Double(anchor.handleIn.x - origin.x), Double(anchor.handleIn.y - origin.y),
                 Double(anchor.handleOut.x - origin.x), Double(anchor.handleOut.y - origin.y),
                 anchor.smooth ? 1 : 0]
            })
        }
    }

    /// What a decoded outline must be to describe pixels: a known fill rule, one subpath or more
    /// within the caps, every subpath with an anchor, every anchor seven finite numbers inside the
    /// local wall with `smooth` 0 or 1.
    var isValid: Bool {
        PathFillRule(rawValue: fillRule) != nil && !subpaths.isEmpty
            && subpaths.count <= Self.maxSubpaths && anchorCount <= Self.maxAnchors
            && subpaths.allSatisfy { subpath in
                !subpath.anchors.isEmpty && subpath.anchors.allSatisfy { anchor in
                    anchor.count == 7 && (anchor[6] == 0 || anchor[6] == 1)
                        && anchor.allSatisfy { $0.isFinite && abs($0) <= Self.maxLocalCoordinate }
                }
            }
    }

    /// The outline as a path value with the box's top-left at `origin`.
    func shape(origin: CGPoint) -> PathShape {
        PathShape(subpaths: subpaths.map { subpath in
            PathSubpath(anchors: subpath.anchors.map { a in
                PathAnchor(
                    point: CGPoint(x: a[0] + origin.x, y: a[1] + origin.y),
                    handleIn: CGPoint(x: a[2] + origin.x, y: a[3] + origin.y),
                    handleOut: CGPoint(x: a[4] + origin.x, y: a[5] + origin.y), smooth: a[6] != 0)
            }, closed: subpath.closed)
        }, fillRule: PathFillRule(rawValue: fillRule) ?? .nonZero)
    }

    /// The outline as CoreGraphics draws it, in box-local space (`PathShape.cgPath`'s rule).
    func cgPath() -> CGPath { shape(origin: .zero).cgPath() }

    /// The outline scaled about the box's origin by `sx` × `sy` — a re-boxed shape, the box and
    /// its outline resized alike.
    func scaled(x sx: Double, y sy: Double) -> ShapeOutline {
        var result = self
        result.subpaths = subpaths.map { subpath in
            Subpath(closed: subpath.closed, anchors: subpath.anchors.map { a in
                [a[0] * sx, a[1] * sy, a[2] * sx, a[3] * sy, a[4] * sx, a[5] * sy, a[6]]
            })
        }
        return result
    }

    /// The axes a re-box of a box `size` scales the outline along: each one the outline as drawn
    /// spans, in a box with a size along it. A straight horizontal or vertical path spans none
    /// along one axis whatever its box's size there — `placed` floors the box's origin to a whole
    /// pixel, so a vertical line at x = 100.5 sits 0.5 px into a box 0.5 px wide — and has nothing
    /// to scale along it; scaling that fraction would move the line.
    func scalingAxes(in size: CGSize) -> (x: Bool, y: Bool) {
        let bounds = cgPath().boundingBoxOfPath
        guard !bounds.isNull else { return (false, false) }
        return (bounds.width > 0 && size.width > 0, bounds.height > 0 && size.height > 0)
    }

    /// The outline re-boxed from a box `old` to one `new`: scaled about the box's origin by the
    /// new size over the old along each axis of `axes` (`scalingAxes(in: old)`), unchanged along
    /// the others. The callers keep the size of an axis it does not scale.
    func reboxed(from old: CGSize, to new: CGSize, axes: (x: Bool, y: Bool)) -> ShapeOutline {
        guard new != old else { return self }
        return scaled(x: axes.x ? Double(new.width / old.width) : 1,
                      y: axes.y ? Double(new.height / old.height) : 1)
    }

    /// A canvas path placed as a shape: the outline and the box it spans. Every coordinate is
    /// first quantized to four decimals by the path model's rule (the master is the core's
    /// `style_json::q4_f64`, which `path_geometry` stores every coordinate through), so a path
    /// read back from the list, a recorded step's `subpaths` and the path itself all place the
    /// same outline bits. The box is the outline's tight bounding box with its origin moved DOWN
    /// to a whole pixel and the fractions kept in the outline: an untransformed anchor is placed
    /// on whole pixels (`DescribedLayer.placementAnchor`), so a box on the path's own fractional
    /// corner would move the shape. nil for an empty or non-finite shape, one past the caps or
    /// the ±1,000,000 coordinate wall, or one whose outline has no extent.
    static func placed(_ path: PathShape) -> (outline: ShapeOutline, box: CGRect)? {
        guard !path.isEmpty, path.isFinite, path.subpaths.count <= maxSubpaths,
              path.anchorCount <= maxAnchors,
              path.subpaths.allSatisfy({ subpath in
                  !subpath.anchors.isEmpty && subpath.anchors.allSatisfy { anchor in
                      [anchor.point, anchor.handleIn, anchor.handleOut].allSatisfy {
                          abs($0.x) <= 1_000_000 && abs($0.y) <= 1_000_000
                      }
                  }
              })
        else { return nil }
        func q4(_ v: CGFloat) -> CGFloat {
            let q = (Double(v) * 1e4).rounded() / 1e4
            return CGFloat(q == 0 ? 0 : q)
        }
        func q4(_ p: CGPoint) -> CGPoint { CGPoint(x: q4(p.x), y: q4(p.y)) }
        var shape = path
        for s in shape.subpaths.indices {
            for a in shape.subpaths[s].anchors.indices {
                var anchor = shape.subpaths[s].anchors[a]
                anchor.point = q4(anchor.point)
                anchor.handleIn = q4(anchor.handleIn)
                anchor.handleOut = q4(anchor.handleOut)
                shape.subpaths[s].anchors[a] = anchor
            }
        }
        let bounds = shape.cgPath().boundingBoxOfPath
        guard !bounds.isNull, [bounds.minX, bounds.minY, bounds.maxX, bounds.maxY]
            .allSatisfy({ $0.isFinite && abs($0) <= 1_000_000 })
        else { return nil }
        let origin = CGPoint(x: bounds.minX.rounded(.down), y: bounds.minY.rounded(.down))
        let box = CGRect(x: origin.x, y: origin.y,
                         width: bounds.maxX - origin.x, height: bounds.maxY - origin.y)
        return (ShapeOutline(shape, origin: origin), box)
    }

    // MARK: - The source rect

    /// The padded source rect of a path shape whose box is `w` × `h` (DescribedLayer.swift's
    /// rule): the box grown by the stroke's reach, with every miter tip the stroke can put
    /// down, then 2 px for antialiasing, rounded out to whole pixels. It is computed from the
    /// payload's numbers alone with `+ − × ÷ √`, never from CoreGraphics' stroker, so the anchor
    /// recovery that reads it back on every reopen (`LayerDescription.anchor`) answers the same
    /// on every machine; a rect taken from the stroked outline's box could move by a pixel where
    /// another system's stroker approximates a curve differently.
    ///
    /// Why it holds the stroke: a stroke `W` wide lies within `W / 2` of the outline except at a
    /// mitered join, and the outline lies in its tight box, so the box grown by `W / 2` holds
    /// everything but the miter tips; a tip lies on the outward bisector of the join at
    /// `(W / 2) / sin(θ / 2)`, θ the angle between the two segments, and only where that ratio
    /// is within the miter limit 10 (beyond it the join is bevelled, inside the grown box). A
    /// centred stroke is `W = strokeWidth`; an outside one is stroked at `2 · strokeWidth` and
    /// keeps the half beyond the outline; an inside one keeps the half within the fill, which the
    /// box already holds. A dash or a dot covers part of the solid stroke, so it never reaches
    /// further. Butt and round ends reach `W / 2`.
    func sourceRect(w: Double, h: Double, strokeWidth: Double, join: ShapeStrokeJoin,
                    align: ShapeStrokeAlign) -> CGRect {
        let stroked = strokeWidth.isFinite && strokeWidth > 0 ? strokeWidth : 0
        let width: Double
        switch align {
        case .center: width = stroked
        case .outside: width = 2 * stroked
        case .inside: width = 0
        }
        var minX = -width / 2, minY = -width / 2
        var maxX = w + width / 2, maxY = h + width / 2
        if width > 0, join == .miter {
            for tip in miterTips(strokeWidth: width) {
                minX = min(minX, tip.x); minY = min(minY, tip.y)
                maxX = max(maxX, tip.x); maxY = max(maxY, tip.y)
            }
        }
        let x0 = (minX - 2).rounded(.down), y0 = (minY - 2).rounded(.down)
        let x1 = (maxX + 2).rounded(.up), y1 = (maxY + 2).rounded(.up)
        return CGRect(x: x0, y: y0, width: x1 - x0, height: y1 - y0)
    }

    /// Where a stroke `strokeWidth` wide with miter joins (limit 10) puts each join's tip: one
    /// per join between two segments of non-zero length whose miter is within the limit. A join
    /// is where two consecutive segments of a subpath meet, and where a closed subpath's last
    /// segment meets its first; a zero-length segment has no direction and is skipped, as a
    /// stroker skips it. A segment's direction at an end is its first non-zero difference of
    /// control points (a cubic whose handle sits on its point takes the next one).
    private func miterTips(strokeWidth: Double) -> [(x: Double, y: Double)] {
        typealias Vector = (x: Double, y: Double)
        func direction(_ candidates: [Vector]) -> Vector? {
            for v in candidates where v.x != 0 || v.y != 0 {
                let length = (v.x * v.x + v.y * v.y).squareRoot()
                return (v.x / length, v.y / length)
            }
            return nil
        }
        var tips: [Vector] = []
        for subpath in subpaths {
            let anchors = subpath.anchors
            let n = anchors.count
            let loop = n == 1 && (anchors[0][2] != anchors[0][0] || anchors[0][3] != anchors[0][1]
                || anchors[0][4] != anchors[0][0] || anchors[0][5] != anchors[0][1])
            let count = subpath.closed ? (n > 1 ? n : (loop ? 1 : 0)) : n - 1
            var segments: [(end: Vector, start: Vector, arrive: Vector)] = []
            for index in 0..<max(count, 0) {
                let a = anchors[index], b = anchors[(index + 1) % n]
                let p0: Vector = (a[0], a[1]), c1: Vector = (a[4], a[5])
                let c2: Vector = (b[2], b[3]), p3: Vector = (b[0], b[1])
                let line = c1 == p0 && c2 == p3
                let chord: Vector = (p3.x - p0.x, p3.y - p0.y)
                let leave = line ? direction([chord])
                    : direction([(c1.x - p0.x, c1.y - p0.y), (c2.x - p0.x, c2.y - p0.y), chord])
                let arrive = line ? direction([chord])
                    : direction([(p3.x - c2.x, p3.y - c2.y), (p3.x - c1.x, p3.y - c1.y), chord])
                guard let leave, let arrive else { continue }
                segments.append((p3, leave, arrive))
            }
            let joins = subpath.closed ? segments.count : segments.count - 1
            for k in 0..<max(joins, 0) {
                let incoming = segments[k], outgoing = segments[(k + 1) % segments.count]
                let u = incoming.arrive, v = outgoing.start
                // sin²(θ/2) = (1 + u·v) / 2 for the angle θ between the two segments; the miter
                // is 1 / sin(θ/2) stroke half-widths long and is bevelled past 10. The bound
                // admits a hair more than the limit, so a join the stroker mitres at the limit
                // is never missed.
                let sine2 = (1 + (u.x * v.x + u.y * v.y)) / 2
                let bisector: Vector = (u.x - v.x, u.y - v.y)
                let length = (bisector.x * bisector.x + bisector.y * bisector.y).squareRoot()
                guard sine2 >= 0.0099, sine2 < 1, length > 0 else { continue }
                let reach = strokeWidth / 2 / sine2.squareRoot()
                tips.append((incoming.end.x + bisector.x / length * reach,
                             incoming.end.y + bisector.y / length * reach))
            }
        }
        return tips
    }
}
