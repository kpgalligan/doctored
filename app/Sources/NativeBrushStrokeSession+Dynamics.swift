import CoreGraphics

/// Brush dynamics in the one dab session (`designs/brush-dynamics.md`): a nominal stamp
/// goes to the core (`RasterBrushDynamics`), and each dab it resolves is laid through the
/// session's own `stamp(at:diameter:deposit:)`, so the caps, the stop-ink rule and the
/// Pencil's snap apply to every resolved dab exactly as to a plain one. The canvas walk
/// (`ImageCanvasView+BrushDynamics.swift`) and the MCP walk (`generated`) both stamp here.
extension NativeBrushStrokeSession {
    /// The diameter the walk spaces by at `pen`, or nil without dynamics.
    func nominalDiameter(pen: PenSample) -> CGFloat? {
        dynamics?.nominalDiameter(size: size, pen: pen)
    }

    /// One nominal stamp at `point`: its index is the count of stamps before it and its
    /// direction the move from the last one (none for the first), which scatter reads.
    /// The angle reads the HEADING: with `heading` nil (every canvas stamp, every spacing
    /// stamp of the MCP walk) a non-zero move updates `lastHeading`, which is passed, so a
    /// stamp at rest keeps the travel's angle; a given `heading` (an MCP airbrush tick,
    /// whose move is a jump back along the path) is passed as is and leaves `lastHeading`
    /// alone. Returns the dabs actually laid, as recorded (a Pencil dab snapped), each with
    /// its deposit factor and shape; a laid dab's shape joins `shapes` when
    /// `dynamicsShaped`. Empty without dynamics or once the session takes no more ink.
    @discardableResult
    mutating func stamp(at point: CGPoint, pen: PenSample, heading: CGVector? = nil) -> [ResolvedDab] {
        guard let dynamics, acceptsInk else { return [] }
        let direction = lastNominalPoint.map { CGVector(dx: point.x - $0.x, dy: point.y - $0.y) }
            ?? CGVector(dx: 0, dy: 0)
        lastNominalPoint = point
        if heading == nil, direction.dx != 0 || direction.dy != 0 { lastHeading = direction }
        let resolved = dynamics.dabs(index: dynamicsIndex, at: point, size: size, pen: pen,
                                     direction: direction, heading: heading ?? lastHeading)
        dynamicsIndex += 1
        var laid: [ResolvedDab] = []
        for dab in resolved {
            let before = dabs.count
            stamp(at: dab.point, diameter: dab.diameter, deposit: tip.flow * dab.factor)
            if dabs.count > before {
                if dynamicsShaped {
                    shapes.append(contentsOf: [Double(dab.angleDegrees), Double(dab.roundness)])
                }
                laid.append(ResolvedDab(point: CGPoint(x: dabs[before], y: dabs[before + 1]),
                                        diameter: CGFloat(dabs[before + 2]), factor: dab.factor,
                                        angleDegrees: dab.angleDegrees, roundness: dab.roundness))
            }
            guard acceptsInk else { break }
        }
        return laid
    }
}
