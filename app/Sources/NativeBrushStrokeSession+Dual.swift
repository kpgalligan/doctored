import CoreGraphics

/// The dual brush in the one dab session (`designs/brush-dynamics.md`, slice 4; the rule is the
/// core's, `brush_dual.rs`): each spacing stamp's nominal point is fed to the second tip's walk
/// (`stamp(at:pen:heading:tick:)` does it, never for an airbrush tick), and each second dab the
/// core lays goes through `chargeDual`, so the dab cap, the shared work budget and the stop-ink
/// rule hold for the second list exactly as for the first.
extension NativeBrushStrokeSession {
    /// Feeds `point` to the second tip's walk, calling the core again with the same point while
    /// a call lays stamps (each call lays at most `RasterBrushDual.stampsPerCall`), and appends
    /// every second dab the caps admit to `dualDabs`. Returns the second-dab indices laid (an
    /// empty range without a dual brush, or once the session takes no more ink).
    @discardableResult
    mutating func walkDual(to point: CGPoint) -> Range<Int> {
        let start = dualDabs.count / 4
        guard let dual else { return start..<start }
        while acceptsInk {
            let step = dual.handle.walk(index: dualIndex, anchor: dualAnchor, to: point)
            // No stamp: the point lies within a step of the anchor (or the core refused it), and
            // the anchor stays where it was.
            guard step.stamps > 0 else { break }
            dualAnchor = step.anchor
            dualIndex += step.stamps
            for k in stride(from: 0, to: step.dabs.count, by: 4) {
                guard chargeDual(diameter: CGFloat(step.dabs[k + 2])) else {
                    return start..<dualDabs.count / 4
                }
                dualDabs.append(contentsOf: step.dabs[k..<k + 4])
            }
        }
        return start..<dualDabs.count / 4
    }
}
