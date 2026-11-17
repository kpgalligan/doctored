import CoreGraphics

/// One dab a dynamics stamp resolved (`dr_brush_dynamics_dabs`): its centre, its
/// diameter, its deposit factor, which multiplies the tip's flow, and its tip shape
/// (angle 0 and roundness 1 when the request has no tip shape).
struct ResolvedDab: Equatable {
    var point: CGPoint
    var diameter: CGFloat
    var factor: CGFloat
    var angleDegrees: CGFloat
    var roundness: CGFloat
    /// The dab's ink with colour dynamics (the document's unit triple), nil without.
    var ink: [Double]? = nil
}

/// A parsed brush dynamics request (`dr_brush_dynamics_create`), immutable, so a
/// stroke's walk resolves every stamp without parsing again. The rule is the
/// core's (`brush_dynamics.rs`, `designs/brush-dynamics.md`).
final class RasterBrushDynamics {
    private let ptr: OpaquePointer

    init(request: String) throws {
        var err: UnsafeMutablePointer<CChar>?
        guard let ptr = request.withCString({ dr_brush_dynamics_create($0, &err) }) else {
            throw DoctoredCoreError(message: takeErrorMessage(err,
                fallback: "The brush dynamics could not be read."))
        }
        self.ptr = ptr
    }

    deinit { dr_brush_dynamics_free(ptr) }

    /// The nominal diameter the walk's spacing follows, for `size` and the pen
    /// sample; `size` when the core refuses the size.
    func nominalDiameter(size: CGFloat, pen: PenSample) -> CGFloat {
        var out = [Double](repeating: 0, count: 2)
        guard dr_brush_dynamics_nominal(ptr, Double(size), pen.pressure, pen.tiltX, pen.tiltY,
                                        &out) else { return size }
        return CGFloat(out[0])
    }

    /// Nominal stamp `index` at `point` resolved into its dabs; `direction` is the
    /// move from the stamp before (zero when there is none), which scatter reads, and
    /// `heading` the direction of travel the angle reads (zero until the stroke first
    /// moves). Empty when the core refuses the sample (a non-finite position or size).
    func dabs(index: UInt64, at point: CGPoint, size: CGFloat, pen: PenSample,
              direction: CGVector, heading: CGVector) -> [ResolvedDab] {
        let sample = [Double(point.x), Double(point.y), Double(size), pen.pressure, pen.tiltX,
                      pen.tiltY, Double(direction.dx), Double(direction.dy), pen.rotation,
                      Double(heading.dx), Double(heading.dy)]
        var out = [Double](repeating: 0, count: 6 * BrushDynamics.maximumCount)
        let n: Int = dr_brush_dynamics_dabs(ptr, index, sample, sample.count, &out, out.count)
        // Typed and indexed once: Swift 6.3 gives up type-checking the untyped closure.
        return (0..<n).map { (k: Int) -> ResolvedDab in
            let at = 6 * k
            return ResolvedDab(point: CGPoint(x: out[at], y: out[at + 1]),
                               diameter: CGFloat(out[at + 2]), factor: CGFloat(out[at + 3]),
                               angleDegrees: CGFloat(out[at + 4]), roundness: CGFloat(out[at + 5]))
        }
    }
}

/// A brush texture's core handle (`dr_brush_texture_create`): the straight RGBA8 tile's level
/// at its scale, anchored at the canvas origin, with its depth. Immutable; the textured
/// rasterizers read it (`RasterSelection.nativeBrush`, `nativeBrushOverlay`). The rule is the
/// core's (`brush_texture.rs`).
final class RasterBrushTexture {
    let ptr: OpaquePointer

    init(rgba: [UInt8], width: Int, height: Int, scale: Double, depth: Double) throws {
        guard width > 0, height > 0, rgba.count == width * height * 4,
              let w = UInt32(exactly: width), let h = UInt32(exactly: height) else {
            throw DoctoredCoreError(message: NativeBrushStroke.textureRefusal)
        }
        var err: UnsafeMutablePointer<CChar>?
        guard let ptr = rgba.withUnsafeBufferPointer({
            dr_brush_texture_create($0.baseAddress, w, h, scale, depth, &err)
        }) else {
            throw DoctoredCoreError(message: takeErrorMessage(err,
                fallback: "The brush texture could not be read."))
        }
        self.ptr = ptr
    }

    deinit { dr_brush_texture_free(ptr) }
}

/// A stroke's colour dynamics (`dr_brush_colour_create`): the foreground, the background, the
/// seed and the four amounts, parsed once, so each laid dab's ink resolves without parsing again.
/// Immutable. The rule is the core's (`brush_colour.rs`). `lab` is whether the request's triples
/// are a Lab document's codes, which the canvas preview converts for display.
final class RasterBrushColour {
    private let ptr: OpaquePointer
    let lab: Bool

    init(request: String, lab: Bool = false) throws {
        var err: UnsafeMutablePointer<CChar>?
        guard let ptr = request.withCString({ dr_brush_colour_create($0, &err) }) else {
            throw DoctoredCoreError(message: takeErrorMessage(err,
                fallback: "The colour dynamics could not be read."))
        }
        self.ptr = ptr
        self.lab = lab
    }

    deinit { dr_brush_colour_free(ptr) }

    /// Dab `dab` of nominal stamp `index`'s ink, three values in 0…1; black when the core
    /// answers nothing (it answers every live handle).
    func ink(index: UInt64, dab: Int) -> [Double] {
        var out = [Double](repeating: 0, count: 3)
        guard let k = UInt32(exactly: dab), dr_brush_colour_ink(ptr, index, k, &out) else {
            return [0, 0, 0]
        }
        return out
    }
}

/// A dual brush's walk (`dr_brush_dual_create`): the second tip's size, spacing and scatter,
/// parsed once, so a stroke feeds every path point without parsing again. Immutable. The rule is
/// the core's (`brush_dual.rs`).
final class RasterBrushDual {
    private let ptr: OpaquePointer
    /// The stamps a `walk` call's buffer is sized for at the largest scatter count
    /// (`BrushDynamics.maximumCount` dabs a stamp). The core lays whole stamps while a stamp's
    /// most dabs fit, so a smaller count lays more a call: up to 1,024 at a count of 1. A longer
    /// move is continued by the caller.
    static let stampsPerCall = 64

    init(request: String) throws {
        var err: UnsafeMutablePointer<CChar>?
        guard let ptr = request.withCString({ dr_brush_dual_create($0, &err) }) else {
            throw DoctoredCoreError(message: takeErrorMessage(err,
                fallback: "The dual brush could not be read."))
        }
        self.ptr = ptr
    }

    deinit { dr_brush_dual_free(ptr) }

    /// One core call feeding the path point `to` to the walk whose anchor is `anchor` (nil: `to`
    /// is the stroke's first point) and whose next stamp is `index`: the dabs it laid (x, y,
    /// diameter, deposit quadruples), the anchor it left and the stamps laid, as many as the
    /// buffer holds (`stampsPerCall`'s doc). Zero stamps and the anchor unchanged (`to` for a nil
    /// one) when `to` lies within a step of the anchor or the core refuses the point.
    func walk(index: UInt64, anchor: CGPoint?, to: CGPoint) -> (dabs: [Double], anchor: CGPoint,
                                                               stamps: UInt64) {
        var out = [Double](repeating: 0,
                           count: 4 * BrushDynamics.maximumCount * Self.stampsPerCall)
        var step = [Double.nan, .nan, 0]
        let start = anchor.map { [Double($0.x), Double($0.y)] }
        let n: Int
        if let start {
            n = start.withUnsafeBufferPointer {
                dr_brush_dual_walk(ptr, index, $0.baseAddress, Double(to.x), Double(to.y), &out,
                                   out.count, &step)
            }
        } else {
            n = dr_brush_dual_walk(ptr, index, nil, Double(to.x), Double(to.y), &out, out.count,
                                   &step)
        }
        guard step[0].isFinite, step[1].isFinite else { return ([], anchor ?? to, 0) }
        return (Array(out[..<(4 * n)]), CGPoint(x: step[0], y: step[1]), UInt64(step[2]))
    }
}
