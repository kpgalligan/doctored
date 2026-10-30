import CoreGraphics
import Foundation

// `TextSessionPlacement` is pure: the layer map's baseline angle and √det (a mirrored or singular
// map is the identity), the editor quad and the convex-quad hit and overlap tests; the text tool's
// `topmostTextLayer` hit-tests a transformed layer on its quad.

/// How the on-canvas text editor is presented over a text layer:
/// the layer's baseline angle and uniform scale, read off its linear map, so
/// a rotated or scaled layer is edited in place instead of from an upright,
/// unscaled box. Pure geometry: the canvas host view applies it
/// (ImageCanvasView+TextSession.swift) and the text tool's hit test reads
/// the convex-quad test (EditorViewController+Text.swift).
///
/// Shear and anisotropic scale are approximated — the angle of the map's
/// x axis and √det — and a mirrored map (det < 0) has no rotation that
/// shows it, so it keeps the upright editor.
struct TextSessionPlacement: Equatable {
    /// Clockwise on the y-down canvas — the same number NSView's
    /// `frameRotation` takes inside the flipped canvas.
    let rotationDegrees: CGFloat
    let scale: CGFloat

    static let identity = TextSessionPlacement(rotationDegrees: 0, scale: 1)

    private init(rotationDegrees: CGFloat, scale: CGFloat) {
        self.rotationDegrees = rotationDegrees
        self.scale = scale
    }

    /// The layer's map as an angle and a uniform scale; the identity for a
    /// map that is not invertible or mirrors.
    init(map: LinearMap) {
        guard map.isInvertible, map.determinant >= 0 else {
            self = .identity
            return
        }
        let degrees = atan2(map.b, map.a) * 180 / Double.pi
        self.init(
            rotationDegrees: CGFloat(LinearMap.snap(degrees)),
            scale: CGFloat(LinearMap.snap(map.determinant.squareRoot())))
    }

    var isIdentity: Bool { rotationDegrees == 0 && scale == 1 }

    /// `origin + s·R(θ)·p` in CG order: x′ = cos·x − sin·y, y′ = sin·x + cos·y.
    func apply(_ p: CGPoint, origin: CGPoint) -> CGPoint {
        let radians = Double(rotationDegrees) * Double.pi / 180
        let c = cos(radians) * Double(scale)
        let s = sin(radians) * Double(scale)
        let x = Double(p.x)
        let y = Double(p.y)
        return CGPoint(x: Double(origin.x) + c * x - s * y, y: Double(origin.y) + s * x + c * y)
    }

    /// The corners of (0, 0, w, h) through `apply`: TL, TR, BR, BL.
    func quad(origin: CGPoint, size: CGSize) -> [CGPoint] {
        [
            CGPoint(x: 0, y: 0), CGPoint(x: size.width, y: 0),
            CGPoint(x: size.width, y: size.height), CGPoint(x: 0, y: size.height),
        ].map { apply($0, origin: origin) }
    }

    /// Whether a convex quad of either winding contains `point`; a point on
    /// an edge counts as inside.
    static func convexQuad(_ quad: [CGPoint], contains point: CGPoint) -> Bool {
        guard quad.count == 4 else { return false }
        var positive = false
        var negative = false
        for i in 0..<4 {
            let a = quad[i]
            let b = quad[(i + 1) % 4]
            let cross = Double(b.x - a.x) * Double(point.y - a.y)
                - Double(b.y - a.y) * Double(point.x - a.x)
            if cross > 1e-9 { positive = true }
            if cross < -1e-9 { negative = true }
            if positive && negative { return false }
        }
        return true
    }

    /// Whether a convex quad and `rect` share any area — a separating-axis
    /// test over the rect's two axes and the quad's four edge normals, so a
    /// rotated quad whose bounding box only grazes the rect does not count.
    /// Touching edges do not overlap.
    static func convexQuad(_ quad: [CGPoint], overlaps rect: CGRect) -> Bool {
        guard quad.count == 4, !rect.isEmpty else { return false }
        let corners = [
            CGPoint(x: rect.minX, y: rect.minY), CGPoint(x: rect.maxX, y: rect.minY),
            CGPoint(x: rect.maxX, y: rect.maxY), CGPoint(x: rect.minX, y: rect.maxY),
        ]
        var axes: [(Double, Double)] = [(1, 0), (0, 1)]
        for i in 0..<4 {
            let a = quad[i]
            let b = quad[(i + 1) % 4]
            axes.append((-Double(b.y - a.y), Double(b.x - a.x)))
        }
        func span(_ points: [CGPoint], _ axis: (Double, Double)) -> (Double, Double) {
            let values = points.map { Double($0.x) * axis.0 + Double($0.y) * axis.1 }
            return (values.min() ?? 0, values.max() ?? 0)
        }
        for axis in axes where axis.0 != 0 || axis.1 != 0 {
            let q = span(quad, axis)
            let r = span(corners, axis)
            if q.1 <= r.0 || r.1 <= q.0 { return false }
        }
        return true
    }
}
